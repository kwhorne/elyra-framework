//! `/__inspector/*` (RFC 0014): the developer's drawer, in debug builds
//! only. This module isn't compiled into a release, so its routes don't
//! exist there.
//!
//! - `abilities`: every ability the app knows, with whether the app grants
//!   it, whether the user has it, and whether it's masked.
//! - `mask` `{ ability, off }`: turn one off for this run, or back on. It
//!   only takes away (see [`crate::inspector`]).
//! - `window` `{ route }`: what the assistant would see of this window: the
//!   note, the rules, the tools.
//! - `live`: the window's live queries.
//! - `calls`: the window's last calls, newest first.
//! - `offline` `{ on? }`: the backend unplugged, or plugged back in; without
//!   `on`, just whether it is (see [`crate::inspector::Unplugged`]).
//! - `presence`: the presence channels the windows want (RFC 0015), who's
//!   in each, and whether this window is one that wants it.
//! - `scope`: this window's label and limits (RFC 0016): the capabilities
//!   the app grants that it doesn't, and its ability limits.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::inspector::{InspectorOff, Mask};
use crate::security::{ability_matches, WindowScope};

use super::protocol::{msgpack_err, msgpack_ok, Body};
use super::{Runner, WindowRef};

/// One ability, as the drawer lists it.
#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct AbilityRow {
    pub(crate) ability: String,
    /// Whether the app grants it to the frontend (`App::allow_ability`).
    pub(crate) granted: bool,
    /// Whether the signed-in user has it, by Laravel; `None` without
    /// `Backend::abilities()`.
    pub(crate) user: Option<bool>,
    /// Whether the inspector turned it off.
    pub(crate) masked: bool,
    /// Whether this window allows it (RFC 0016); `None` when the window has
    /// no ability limits.
    pub(crate) window: Option<bool>,
}

/// This window's limits (RFC 0016), as the drawer shows them.
#[derive(Serialize)]
struct ScopeSection {
    /// `None` outside a window (the test harness without one).
    label: Option<String>,
    /// The capabilities the app grants that this window doesn't.
    denied: Vec<String>,
    abilities: Option<crate::security::WindowAbilities>,
}

#[derive(Deserialize)]
struct MaskOne {
    ability: String,
    off: bool,
}

#[derive(Deserialize, Default)]
struct WindowAt {
    #[serde(default)]
    route: Option<String>,
}

#[derive(Deserialize, Default)]
struct Unplug {
    #[serde(default)]
    on: Option<bool>,
}

/// The offline switch, as the drawer shows it.
#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct OfflineRow {
    /// Whether the app has a backend to unplug.
    pub(crate) backend: bool,
    pub(crate) on: bool,
    /// Whether it works offline (`Backend::offline()`): reads from its
    /// copy, writes to the outbox. Without it, a call is an `offline` error.
    pub(crate) copy: bool,
}

/// The presence section: whether the app has presence, the signed-in
/// user's id, and each wanted channel.
#[derive(Serialize)]
struct PresenceSection {
    enabled: bool,
    me: Option<String>,
    #[cfg(feature = "backend")]
    channels: Vec<crate::presence::InspectedChannel>,
    #[cfg(not(feature = "backend"))]
    channels: Vec<()>,
}

/// The `window` section, for an app without an assistant.
#[derive(Serialize)]
struct NoAssistant {
    assistant: bool,
}

pub(super) fn serve(
    runner: &Runner,
    op: &str,
    window: &str,
    scope: Option<&WindowRef>,
    body: &[u8],
) -> Body {
    if runner.ctx.try_get::<InspectorOff>().is_some() {
        return msgpack_err("the inspector is off (App::inspector(false))".into());
    }
    match op {
        "abilities" => msgpack_ok(&abilities(runner, scope)),
        "mask" => {
            let one: MaskOne = match rmp_serde::from_slice(body) {
                Ok(one) => one,
                Err(e) => return msgpack_err(format!("mask: {e}")),
            };
            if !valid(&one.ability) {
                return msgpack_err(format!("not an ability: {:?}", one.ability));
            }
            if runner.policy.mask().set(&one.ability, one.off) {
                changed(runner);
            }
            msgpack_ok(&abilities(runner, scope))
        }
        "window" => {
            let at: WindowAt = rmp_serde::from_slice(body).unwrap_or_default();
            #[cfg(feature = "ai")]
            if let Some(assistants) = runner.ctx.try_get::<crate::assistant::Assistants>() {
                if let Some(seen) =
                    assistants.inspect(window, scope.map(|w| &w.scope), at.route.as_deref())
                {
                    return msgpack_ok(&seen);
                }
            }
            let _ = (window, at.route);
            msgpack_ok(&NoAssistant { assistant: false })
        }
        "live" => {
            #[cfg(feature = "database")]
            if let Some(live) = runner.ctx.try_get::<crate::live::LiveRegistry>() {
                return msgpack_ok(&live.inspect(window));
            }
            msgpack_ok(&Vec::<()>::new())
        }
        "calls" => match runner.ctx.try_get::<crate::inspector::Calls>() {
            Some(calls) => msgpack_ok(&calls.of(window)),
            None => msgpack_ok(&Vec::<()>::new()),
        },
        "offline" => {
            let want: Unplug = rmp_serde::from_slice(body).unwrap_or_default();
            msgpack_ok(&offline(runner, want.on))
        }
        "presence" => msgpack_ok(&presence(runner, window)),
        "scope" => msgpack_ok(&ScopeSection {
            label: scope.map(|w| w.label.clone()),
            denied: scope
                .map(|w| w.scope.denied_of(&runner.policy))
                .unwrap_or_default(),
            abilities: scope.and_then(|w| w.scope.abilities()),
        }),
        _ => msgpack_err(format!("unknown inspector op: {op}")),
    }
}

/// Who's where, for the drawer.
fn presence(runner: &Runner, window: &str) -> PresenceSection {
    #[cfg(feature = "backend")]
    if let Some(presence) = runner.ctx.try_get::<crate::presence::Presence>() {
        let channels = presence.inspect(window);
        return PresenceSection {
            enabled: true,
            me: presence.me(),
            channels,
        };
    }
    let _ = (runner, window);
    PresenceSection {
        enabled: false,
        me: None,
        channels: Vec::new(),
    }
}

/// The switch, turned to `on` when given.
fn offline(runner: &Runner, on: Option<bool>) -> OfflineRow {
    #[cfg(feature = "backend")]
    if let Some(backend) = runner.ctx.try_get::<crate::backend::Backend>() {
        if let Some(on) = on {
            if backend.unplugged().set(on) {
                unplug(runner, &backend, on);
            }
        }
        #[cfg(feature = "database")]
        let copy = backend.offline_handle().is_some();
        #[cfg(not(feature = "database"))]
        let copy = false;
        return OfflineRow {
            backend: true,
            on: backend.unplugged().is_on(),
            copy,
        };
    }
    let _ = (runner, on);
    OfflineRow {
        backend: false,
        on: false,
        copy: false,
    }
}

/// What follows the switch at once, rather than at the next request: the
/// broadcasting connection, and the connection state the status bar shows.
#[cfg(feature = "backend")]
fn unplug(runner: &Runner, backend: &crate::backend::Backend, on: bool) {
    if let Some(broadcaster) = runner.ctx.try_get::<crate::broadcasting::Broadcaster>() {
        broadcaster.unplugged(on);
    }
    #[cfg(feature = "database")]
    if let Some(offline) = backend.offline_handle() {
        if on {
            offline.went_offline();
            // Offline already, for real: now it's simulated too.
            offline.emit();
        } else {
            offline.recheck();
        }
    }
    #[cfg(not(feature = "database"))]
    let _ = (backend, on);
}

/// An ability's name, or a namespace: `customers.delete`, `customers.*`, `*`.
fn valid(ability: &str) -> bool {
    !ability.is_empty()
        && ability.len() <= 128
        && ability
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':' | '*'))
}

/// The user's list from Laravel, when the app has one.
fn user_list(runner: &Runner) -> Option<Vec<String>> {
    #[cfg(feature = "backend")]
    {
        runner
            .ctx
            .try_get::<crate::abilities::Abilities>()
            .map(|a| a.list())
    }
    #[cfg(not(feature = "backend"))]
    {
        let _ = runner;
        None
    }
}

fn abilities(runner: &Runner, window: Option<&WindowRef>) -> Vec<AbilityRow> {
    let user = user_list(runner);
    rows(
        runner.registry.commands().filter_map(|c| c.ability()),
        runner.policy.granted(),
        user.as_deref(),
        runner.policy.mask(),
        window.map(|w| &w.scope),
        |a| runner.policy.grants_ability(a),
    )
}

/// The rows: what the commands declare, what the app grants, what the user
/// has and what's masked, each once, in order.
pub(crate) fn rows<'a>(
    declared: impl Iterator<Item = &'a str>,
    granted: &[String],
    user: Option<&[String]>,
    mask: &Mask,
    window: Option<&WindowScope>,
    grants: impl Fn(&str) -> bool,
) -> Vec<AbilityRow> {
    // Only a window with ability limits says anything per ability.
    let window = window.filter(|w| w.abilities().is_some());
    let masked = mask.list();
    let mut names: BTreeSet<String> = declared.map(str::to_owned).collect();
    names.extend(granted.iter().cloned());
    names.extend(user.into_iter().flatten().cloned());
    names.extend(masked.iter().cloned());
    names
        .into_iter()
        .map(|ability| AbilityRow {
            granted: grants(&ability),
            user: user.map(|list| list.iter().any(|g| ability_matches(g, &ability))),
            masked: masked.iter().any(|m| ability_matches(m, &ability)),
            window: window.map(|w| w.allows_ability(&ability)),
            ability,
        })
        .collect()
}

/// The mask changed: the runtime's `can` follows on `elyra:abilities`, and
/// the live queries that asked about an ability run again.
fn changed(runner: &Runner) {
    #[cfg(feature = "backend")]
    if let Some(abilities) = runner.ctx.try_get::<crate::abilities::Abilities>() {
        let _ = runner.bus.emit("elyra:abilities", &abilities.state());
        #[cfg(feature = "database")]
        if let Some(live) = runner.ctx.try_get::<crate::live::LiveRegistry>() {
            live.invalidate(crate::abilities::LIVE_KEY);
        }
        return;
    }
    #[derive(Serialize)]
    struct State {
        loaded: bool,
        abilities: [&'static str; 1],
        masked: Vec<String>,
    }
    let _ = runner.bus.emit(
        "elyra:abilities",
        &State {
            loaded: true,
            abilities: ["*"],
            masked: runner.policy.mask().list(),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_abilities_or_namespaces() {
        for ok in ["customers.delete", "customers.*", "*", "a_b-c:d"] {
            assert!(valid(ok), "{ok}");
        }
        let long = "a".repeat(129);
        for bad in ["", "a b", "a/b", "<x>", long.as_str()] {
            assert!(!valid(bad), "{bad}");
        }
    }

    #[test]
    fn the_rows_are_every_ability_the_app_knows_once() {
        let mask = Mask::default();
        mask.set("invoices.*", true);
        let granted = vec!["customers.*".to_string()];
        let user = vec!["customers.view".to_string(), "reports.*".to_string()];
        let rows = rows(
            [
                "customers.view",
                "customers.delete",
                "invoices.send",
                "reports.view",
            ]
            .into_iter(),
            &granted,
            Some(&user),
            &mask,
            None,
            |a| a.starts_with("customers."),
        );
        let names: Vec<&str> = rows.iter().map(|r| r.ability.as_str()).collect();
        assert_eq!(
            names,
            [
                "customers.*",
                "customers.delete",
                "customers.view",
                "invoices.*",
                "invoices.send",
                "reports.*",
                "reports.view"
            ]
        );
        let row = |name: &str| rows.iter().find(|r| r.ability == name).unwrap();
        assert_eq!(
            row("customers.delete"),
            &AbilityRow {
                ability: "customers.delete".into(),
                granted: true,
                user: Some(false),
                masked: false,
                window: None,
            }
        );
        assert_eq!(row("customers.view").user, Some(true));
        // A namespace the user has covers its abilities.
        assert_eq!(row("reports.*").user, Some(true));
        assert_eq!(row("reports.view").user, Some(true));
        assert!(row("invoices.send").masked && !row("invoices.send").granted);
        assert!(row("invoices.*").masked);
        // Without Laravel's list, `user` is unknown.
        let rows = super::rows(
            ["a.b"].into_iter(),
            &[],
            None,
            &Mask::default(),
            None,
            |_| true,
        );
        assert_eq!(rows[0].user, None);
    }

    #[test]
    fn a_window_with_ability_limits_says_which_it_allows() {
        let mut preview = WindowScope::default();
        preview.only_abilities(["invoices.view"]);
        let rows = rows(
            ["invoices.view", "invoices.delete"].into_iter(),
            &[],
            None,
            &Mask::default(),
            Some(&preview),
            |_| true,
        );
        let row = |name: &str| rows.iter().find(|r| r.ability == name).unwrap();
        assert_eq!(row("invoices.view").window, Some(true));
        assert_eq!(row("invoices.delete").window, Some(false));
        // Limits on capabilities alone say nothing about abilities.
        let mut quiet = WindowScope::default();
        quiet.deny(crate::security::Capability::Assistant);
        let rows = super::rows(
            ["a.b"].into_iter(),
            &[],
            None,
            &Mask::default(),
            Some(&quiet),
            |_| true,
        );
        assert_eq!(rows[0].window, None);
    }
}
