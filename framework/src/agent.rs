//! Running a command for an AI: the one call path behind the MCP server
//! (RFC 0003) and the in-app assistant (RFC 0011).
//!
//! A [`Caller`] takes a tool from the [catalog](crate::mcp::catalog) and the
//! arguments a model gave by name, and runs the command the way the shell
//! would: through the middleware pipeline, on its own task (a panic is an
//! error), under a rate limit, with `started` / `finished` activity for the
//! windows and an [`AgentCalled`](crate::mcp::AgentCalled) for an audit log.
//! What differs per caller is the grant, how the user confirms, the
//! [`Origin`], and whether the signed-in user's abilities (RFC 0010) bound
//! the call: an external MCP agent's grants are the app's, and an assistant
//! acting for the person at the keyboard has no more than they do.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Map, Value};

use crate::command::CommandRegistry;
use crate::container::Ctx;
use crate::mcp::{AgentActivity, AgentCalled, Tool};
use crate::middleware::Origin;
use crate::ratelimit::RateLimiter;

/// Runs commands for one kind of AI caller. See the [module docs](self).
pub(crate) struct Caller {
    ctx: Ctx,
    registry: Arc<CommandRegistry>,
    /// Calls per tool — this caller's own count.
    limiter: RateLimiter,
    /// The log's target: `elyra::mcp`, `elyra::assistant`.
    target: &'static str,
    /// Where the activity goes.
    channel: &'static str,
    /// Whether the signed-in user must have the ability too.
    for_the_user: bool,
}

impl Caller {
    /// A caller logging under `target` and publishing its activity on
    /// `channel`. The app's grant alone decides what may run, as for an MCP
    /// agent.
    pub(crate) fn new(
        ctx: Ctx,
        registry: Arc<CommandRegistry>,
        target: &'static str,
        channel: &'static str,
    ) -> Self {
        Self {
            ctx,
            registry,
            limiter: RateLimiter::new(crate::cache::Cache::new()),
            target,
            channel,
            for_the_user: false,
        }
    }

    /// Bound every call by the signed-in user's abilities too (RFC 0010):
    /// one they lack is refused, `denied`, without running.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn for_the_user(mut self) -> Self {
        self.for_the_user = true;
        self
    }

    /// Why the signed-in user may not run a command needing `ability`, when
    /// this caller acts for them and the backend says they lack it. Without
    /// `Backend::abilities()` the app's grant is the whole answer.
    pub(crate) fn denied(&self, ability: &str) -> Option<String> {
        if !self.for_the_user {
            return None;
        }
        #[cfg(feature = "backend")]
        if let Some(user) = self.ctx.try_get::<crate::abilities::Abilities>() {
            if !crate::security::UserAbilities::user_allows(&*user, ability) {
                return Some(format!(
                    "The user may not do this: it takes the `{ability}` ability."
                ));
            }
        }
        let _ = ability;
        None
    }

    /// Why `tool` can't run now, if it's over `limit`. With `hit`, a run that
    /// can goes on the count.
    pub(crate) fn throttled(
        &self,
        tool: &Tool,
        limit: (u32, Duration),
        hit: bool,
    ) -> Option<String> {
        let (max, per) = limit;
        let key = &tool.name;
        let over = if hit {
            self.limiter
                .attempt(key, i64::from(max), per, || ())
                .is_none()
        } else {
            self.limiter.too_many_attempts(key, i64::from(max))
        };
        over.then(|| {
            format!(
                "`{}` is rate limited to {max} calls per {}s; try again later.",
                tool.name,
                per.as_secs()
            )
        })
    }

    /// Run `tool` with `body` (its MessagePack arguments) — refused first if
    /// the user lacks its ability or it's over `limit` — and log it (not the
    /// arguments, which may be personal data). `client` names the caller in
    /// the log, the activity and `AgentCalled`; `verb` is the log's
    /// (`called`, `read`).
    pub(crate) async fn run(
        &self,
        tool: &Tool,
        body: Vec<u8>,
        limit: (u32, Duration),
        origin: Origin,
        client: &str,
        verb: &str,
    ) -> Result<crate::Result<Vec<u8>>, tokio::task::JoinError> {
        let name = tool.name.as_str();
        if let Some(why) = self.denied(&tool.ability) {
            crate::warn!(target: self.target, "{client} {verb} {name}: denied");
            return Ok(Err(crate::Error::with_kind("denied", why)));
        }
        if let Some(why) = self.throttled(tool, limit, true) {
            crate::warn!(target: self.target, "{client} {verb} {name}: rate limited");
            return Ok(Err(crate::Error::Command(why)));
        }
        let activity = |phase, ok| AgentActivity {
            tool: name.to_owned(),
            client: client.to_owned(),
            phase,
            ok,
        };
        let bus = self.ctx.try_get::<crate::event::EventBus>();
        if let Some(bus) = &bus {
            let _ = bus.emit(self.channel, &activity("started", None));
        }
        let started = Instant::now();
        let registry = self.registry.clone();
        let ctx = self.ctx.clone();
        let command = name.to_owned();
        let outcome =
            tokio::spawn(async move { registry.dispatch_from(ctx, &command, &body, origin).await })
                .await;
        let ok = matches!(outcome, Ok(Ok(_)));
        let took = started.elapsed();
        crate::info!(
            target: self.target,
            "{client} {verb} {name}: {} in {took:?}",
            if ok { "ok" } else { "failed" },
        );
        if let Some(bus) = &bus {
            let _ = bus.emit(self.channel, &activity("finished", Some(ok)));
        }
        self.ctx.dispatch_background(AgentCalled {
            tool: name.to_owned(),
            client: client.to_owned(),
            ok,
            millis: u64::try_from(took.as_millis()).unwrap_or(u64::MAX),
        });
        outcome
    }
}

/// The arguments a model gave by name, in the order `tool`'s command takes
/// them; one left out is `null` (an `Option`'s `None` — anything else fails
/// to decode, and says so). An unknown name is refused with the ones it
/// takes, so the model can correct itself.
pub(crate) fn positional(
    tool: &Tool,
    arguments: &Map<String, Value>,
) -> Result<Vec<Value>, String> {
    if let Some(unknown) = arguments.keys().find(|k| !tool.args.contains(k)) {
        return Err(format!(
            "`{}` has no argument `{unknown}`; it takes: {}",
            tool.name,
            tool.args.join(", ")
        ));
    }
    Ok(tool
        .args
        .iter()
        .map(|a| arguments.get(a).cloned().unwrap_or(Value::Null))
        .collect())
}

/// A command's `ValidationErrors`, if that's what `message` is.
pub(crate) fn validation_bag(message: &str) -> Option<BTreeMap<String, Vec<String>>> {
    crate::validation::is_validation_bag(message)
        .then(|| serde_json::from_str(message).ok())
        .flatten()
}

/// The bag as a model reads it: "Invalid input:" and a line per message.
pub(crate) fn invalid_input(bag: &BTreeMap<String, Vec<String>>) -> String {
    let lines: Vec<String> = bag
        .iter()
        .flat_map(|(field, messages)| messages.iter().map(move |m| format!("- {field}: {m}")))
        .collect();
    format!("Invalid input:\n{}", lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use serde_json::json;

    use super::*;
    use crate::command::{BoxFuture, Command, CommandSig};
    use crate::mcp::ToolAnnotations;
    use crate::middleware::{CommandRequest, Middleware, Next};
    use crate::Container;

    /// A command that counts its runs.
    struct Counted {
        name: &'static str,
        ability: &'static str,
        runs: Arc<Mutex<u32>>,
    }

    impl Command for Counted {
        fn name(&self) -> &'static str {
            self.name
        }
        fn ability(&self) -> Option<&'static str> {
            Some(self.ability)
        }
        fn call<'a>(&'a self, _ctx: Ctx, _args: &'a [u8]) -> BoxFuture<'a, crate::Result<Vec<u8>>> {
            *self.runs.lock().unwrap() += 1;
            Box::pin(async { Ok(rmp_serde::to_vec(&true).unwrap()) })
        }
        fn signature(&self, _types: &mut specta::Types) -> CommandSig {
            unimplemented!("not in the catalog here")
        }
    }

    /// Records the origin of every call it sees.
    struct Spy(Arc<Mutex<Vec<Origin>>>);

    impl Middleware for Spy {
        fn handle(
            &self,
            ctx: Ctx,
            req: CommandRequest,
            next: Next,
        ) -> BoxFuture<'static, crate::Result<Vec<u8>>> {
            self.0.lock().unwrap().push(req.origin.clone());
            Box::pin(async move { next.run(ctx, req).await })
        }
    }

    fn tool(name: &str, ability: &str, args: &[&str]) -> Tool {
        Tool {
            name: name.into(),
            description: String::new(),
            input_schema: json!({}),
            output_schema: json!({}),
            annotations: ToolAnnotations {
                read_only_hint: false,
                destructive_hint: false,
            },
            ability: ability.into(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            defaults: None,
            template: None,
            no_template: None,
        }
    }

    struct Setup {
        caller: Caller,
        runs: Arc<Mutex<u32>>,
        origins: Arc<Mutex<Vec<Origin>>>,
    }

    /// `customers_view` and `customers_destroy`, with `user` bound as the
    /// signed-in user's abilities when given.
    fn setup(target: &'static str, user: Option<&[&str]>) -> Setup {
        let runs = Arc::new(Mutex::new(0));
        let origins = Arc::new(Mutex::new(Vec::new()));
        let mut registry = CommandRegistry::new();
        for (name, ability) in [
            ("customers_view", "customers.view"),
            ("customers_destroy", "customers.delete"),
        ] {
            registry.register(Box::new(Counted {
                name,
                ability,
                runs: runs.clone(),
            }));
        }
        registry.add_middleware(Arc::new(Spy(origins.clone())));
        #[allow(unused_mut)]
        let mut container = Container::new();
        #[cfg(feature = "backend")]
        if let Some(list) = user {
            container.bind(crate::abilities::Abilities::of(list));
        }
        #[cfg(not(feature = "backend"))]
        let _ = user;
        let ctx = Ctx::new(Arc::new(container));
        Setup {
            caller: Caller::new(ctx, Arc::new(registry), target, "elyra:test"),
            runs,
            origins,
        }
    }

    const LIMIT: (u32, Duration) = (60, Duration::from_secs(60));

    #[test]
    fn arguments_by_name_become_the_commands_order() {
        let t = tool("customers_store", "customers.create", &["input", "tag"]);
        let given = json!({ "tag": "vip", "input": { "name": "Ada" } });
        assert_eq!(
            positional(&t, given.as_object().unwrap()).unwrap(),
            vec![json!({ "name": "Ada" }), json!("vip")]
        );
        // One left out is `null`.
        let given = json!({ "input": {} });
        assert_eq!(
            positional(&t, given.as_object().unwrap()).unwrap(),
            vec![json!({}), Value::Null]
        );
        let given = json!({ "id": 3 });
        assert_eq!(
            positional(&t, given.as_object().unwrap()).unwrap_err(),
            "`customers_store` has no argument `id`; it takes: input, tag"
        );
    }

    #[test]
    fn a_validation_bag_reads_as_lines() {
        let message =
            json!({ "email": ["The email must be valid."], "name": ["Required."] }).to_string();
        let bag = validation_bag(&message).unwrap();
        assert_eq!(
            invalid_input(&bag),
            "Invalid input:\n- email: The email must be valid.\n- name: Required."
        );
        assert!(validation_bag("the ledger is closed").is_none());
    }

    #[tokio::test]
    async fn an_agent_runs_on_the_apps_grant_alone() {
        // The user may only view; an MCP agent's grant is the app's.
        let s = setup("elyra::mcp", Some(&["customers.view"]));
        let destroy = tool("customers_destroy", "customers.delete", &[]);
        let origin = Origin::Agent {
            client: "claude".into(),
        };
        let out = s
            .caller
            .run(
                &destroy,
                vec![0x90],
                LIMIT,
                origin.clone(),
                "claude",
                "called",
            )
            .await
            .unwrap();
        assert!(out.is_ok());
        assert_eq!(*s.runs.lock().unwrap(), 1);
        assert_eq!(*s.origins.lock().unwrap(), vec![origin]);
    }

    #[cfg(feature = "backend")]
    #[tokio::test]
    async fn for_the_user_a_call_they_cant_make_is_denied_without_running() {
        let s = setup("elyra::assistant", Some(&["customers.view"]));
        let caller = s.caller.for_the_user();
        let destroy = tool("customers_destroy", "customers.delete", &[]);
        let view = tool("customers_view", "customers.view", &[]);

        // A limit of one: the refusal mustn't spend it.
        let one = (1, Duration::from_secs(60));
        let out = caller
            .run(
                &destroy,
                vec![0x90],
                one,
                Origin::Assistant,
                "assistant",
                "called",
            )
            .await
            .unwrap();
        let err = out.unwrap_err();
        assert_eq!(err.kind(), Some("denied"));
        assert_eq!(
            err.to_string(),
            "The user may not do this: it takes the `customers.delete` ability."
        );
        assert_eq!(*s.runs.lock().unwrap(), 0);
        assert!(s.origins.lock().unwrap().is_empty());
        assert!(caller.throttled(&destroy, one, false).is_none());

        let out = caller
            .run(
                &view,
                vec![0x90],
                one,
                Origin::Assistant,
                "assistant",
                "called",
            )
            .await
            .unwrap();
        assert!(out.is_ok());
        assert_eq!(*s.runs.lock().unwrap(), 1);
        assert_eq!(*s.origins.lock().unwrap(), vec![Origin::Assistant]);
    }

    #[cfg(feature = "backend")]
    #[tokio::test]
    async fn for_the_user_nothing_runs_before_their_abilities_are_known() {
        let s = setup("elyra::assistant", Some(&[]));
        let caller = s.caller.for_the_user();
        // Signed out, or not read yet.
        if let Some(user) = caller.ctx.try_get::<crate::abilities::Abilities>() {
            user.forget();
        }
        let view = tool("customers_view", "customers.view", &[]);
        let out = caller
            .run(
                &view,
                vec![0x90],
                LIMIT,
                Origin::Assistant,
                "assistant",
                "called",
            )
            .await
            .unwrap();
        assert_eq!(out.unwrap_err().kind(), Some("denied"));
        assert_eq!(*s.runs.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn for_the_user_without_the_backends_abilities_the_grant_decides() {
        let s = setup("elyra::assistant", None);
        let caller = s.caller.for_the_user();
        let destroy = tool("customers_destroy", "customers.delete", &[]);
        let out = caller
            .run(
                &destroy,
                vec![0x90],
                LIMIT,
                Origin::Assistant,
                "assistant",
                "called",
            )
            .await
            .unwrap();
        assert!(out.is_ok());
        assert_eq!(*s.runs.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn the_limit_is_counted_per_caller_and_tool() {
        let mcp = setup("elyra::mcp", None);
        let assistant = setup("elyra::assistant", None);
        let view = tool("customers_view", "customers.view", &[]);
        let one = (1, Duration::from_secs(60));
        assert!(mcp.caller.throttled(&view, one, true).is_none());
        let why = mcp.caller.throttled(&view, one, true).unwrap();
        assert_eq!(
            why,
            "`customers_view` is rate limited to 1 calls per 60s; try again later."
        );
        // Another tool, and another caller, each have their own count.
        let destroy = tool("customers_destroy", "customers.delete", &[]);
        assert!(mcp.caller.throttled(&destroy, one, true).is_none());
        assert!(assistant.caller.throttled(&view, one, true).is_none());
    }
}
