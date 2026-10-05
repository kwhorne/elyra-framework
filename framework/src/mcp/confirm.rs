//! Confirmation for the tools `Mcp::confirm` names (RFC 0003 step 4).
//!
//! A modern client is asked through a multi round-trip: the first
//! `tools/call` answers `input_required` with an `elicitation/create` and a
//! `requestState`, and the call runs only when the client retries with the
//! user's `accept` and that state echoed back.
//!
//! The state passes through the client, so it's signed: an HMAC, under a key
//! that lives only in this process, over the tool, a digest of its
//! arguments, the client's name, an expiry and a nonce. A retry with
//! different arguments, from another client, or after the expiry is asked
//! again, and each state runs the tool at most once — an accepted delete
//! can't be replayed.

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hmac::{Hmac, KeyInit, Mac};
use parking_lot::Mutex;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use super::endpoint::{hex, unhex};

/// How long the user has to answer.
const TTL: Duration = Duration::from_secs(5 * 60);

/// The key the confirmation goes under in `inputRequests` / `inputResponses`.
pub(super) const KEY: &str = "confirm";

/// What the retry's state and answer add up to.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Answer {
    /// The user accepted this call, and the state hasn't been used before.
    Accepted,
    /// The user said no, or dismissed the question.
    Declined,
    /// No answer, or one that doesn't fit this call: ask (again).
    Ask,
}

/// Issues and redeems signed request states.
pub(super) struct Confirmations {
    key: Vec<u8>,
    /// Nonces already redeemed, with their expiry, so a state runs once.
    used: Mutex<HashMap<String, u64>>,
}

impl Confirmations {
    pub(super) fn new() -> Self {
        Self {
            key: crate::security::random_token().into_bytes(),
            used: Mutex::new(HashMap::new()),
        }
    }

    /// A state for asking to run `tool` with `args` for `client`.
    pub(super) fn issue(&self, tool: &str, args: &Value, client: &str) -> String {
        let payload = json!({
            "n": crate::security::random_token(),
            "t": tool,
            "a": digest(args),
            "c": client,
            "e": now_ms() + TTL.as_millis() as u64,
        })
        .to_string();
        let payload = hex(payload.as_bytes());
        let mac = hex(&self.mac(&payload).finalize().into_bytes());
        format!("{payload}.{mac}")
    }

    /// Weigh a retry: its `requestState` and `inputResponses`, against the
    /// call it's on.
    pub(super) fn answer(
        &self,
        state: Option<&Value>,
        responses: Option<&Value>,
        tool: &str,
        args: &Value,
        client: &str,
    ) -> Answer {
        let Some(nonce) = state
            .and_then(Value::as_str)
            .and_then(|s| self.verify(s, tool, args, client))
        else {
            return Answer::Ask;
        };
        let action = responses
            .and_then(|r| r.get(KEY))
            .and_then(|r| r.get("action"))
            .and_then(Value::as_str);
        match action {
            Some("accept") => {
                if self.redeem(nonce.0, nonce.1) {
                    Answer::Accepted
                } else {
                    Answer::Ask
                }
            }
            Some("decline" | "cancel") => Answer::Declined,
            _ => Answer::Ask,
        }
    }

    /// The nonce and expiry of a state that's genuine, unexpired, and for
    /// this very call.
    fn verify(&self, state: &str, tool: &str, args: &Value, client: &str) -> Option<(String, u64)> {
        let (payload, mac) = state.split_once('.')?;
        self.mac(payload).verify_slice(&unhex(mac)?).ok()?;
        let payload: Value = serde_json::from_slice(&unhex(payload)?).ok()?;
        let expiry = payload["e"].as_u64()?;
        let fits = payload["t"] == tool
            && payload["a"] == digest(args).as_str()
            && payload["c"] == client
            && expiry > now_ms();
        fits.then(|| (payload["n"].as_str().unwrap_or_default().to_owned(), expiry))
    }

    /// Mark `nonce` used; `false` when it already was.
    fn redeem(&self, nonce: String, expiry: u64) -> bool {
        let mut used = self.used.lock();
        let now = now_ms();
        used.retain(|_, e| *e > now);
        used.insert(nonce, expiry).is_none()
    }

    fn mac(&self, payload: &str) -> Hmac<Sha256> {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.key).expect("HMAC takes any key");
        mac.update(b"elyra-mcp-confirm/1|");
        mac.update(payload.as_bytes());
        mac
    }
}

/// The question the user sees.
pub(super) fn message(
    client: &str,
    tool: &str,
    description: &str,
    args: &Map<String, Value>,
) -> String {
    let mut text = format!("{client} wants to run `{tool}`.");
    if let Some(what) = description.lines().next().filter(|l| !l.is_empty()) {
        text.push_str(&format!("\n\n{what}"));
    }
    if !args.is_empty() {
        let pretty = serde_json::to_string_pretty(args).unwrap_or_default();
        text.push_str(&format!("\n\n{pretty}"));
    }
    text
}

/// The `elicitation/create` params: a form with nothing to fill in — the
/// answer is the accept or decline itself.
pub(super) fn elicitation(message: &str, with_mode: bool) -> Value {
    let mut params = json!({
        "message": message,
        "requestedSchema": { "type": "object", "properties": {} },
    });
    if with_mode {
        params["mode"] = json!("form");
    }
    params
}

/// Whether a modern client's capabilities let it ask in a form.
pub(super) fn can_elicit(capabilities: Option<&Value>) -> bool {
    match capabilities.and_then(|c| c.get("elicitation")) {
        // An empty object means form mode.
        Some(Value::Object(modes)) => modes.is_empty() || modes.contains_key("form"),
        _ => false,
    }
}

/// A digest of the arguments that doesn't depend on key order.
fn digest(args: &Value) -> String {
    hex(&Sha256::digest(canonical(args).to_string().as_bytes()))
}

fn canonical(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            // Rebuilt in sorted order, whether or not `Map` keeps insertion
            // order (serde_json's `preserve_order`).
            Value::Object(
                keys.into_iter()
                    .map(|k| (k.clone(), canonical(&map[k])))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
        other => other.clone(),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accept() -> Value {
        json!({ KEY: { "action": "accept" } })
    }

    #[test]
    fn an_accepted_state_runs_the_call_once() {
        let c = Confirmations::new();
        let args = json!([7]);
        let state = json!(c.issue("destroy", &args, "claude"));
        assert_eq!(
            c.answer(Some(&state), Some(&accept()), "destroy", &args, "claude"),
            Answer::Accepted
        );
        assert_eq!(
            c.answer(Some(&state), Some(&accept()), "destroy", &args, "claude"),
            Answer::Ask,
            "a replay is asked again"
        );
    }

    #[test]
    fn a_state_fits_only_its_own_call() {
        let c = Confirmations::new();
        let args = json!([7, { "b": 1, "a": 2 }]);
        let state = json!(c.issue("destroy", &args, "claude"));
        let ask = |tool, args: &Value, client| {
            c.answer(Some(&state), Some(&accept()), tool, args, client)
        };
        assert_eq!(ask("update", &args, "claude"), Answer::Ask, "another tool");
        assert_eq!(
            ask("destroy", &json!([8, { "b": 1, "a": 2 }]), "claude"),
            Answer::Ask,
            "other args"
        );
        assert_eq!(
            ask("destroy", &args, "cursor"),
            Answer::Ask,
            "another client"
        );
        // Key order doesn't matter.
        assert_eq!(
            ask("destroy", &json!([7, { "a": 2, "b": 1 }]), "claude"),
            Answer::Accepted
        );
    }

    #[test]
    fn a_forged_or_foreign_state_is_asked_again() {
        let c = Confirmations::new();
        let args = json!([7]);
        let state = c.issue("destroy", &args, "claude");
        let (payload, mac) = state.split_once('.').unwrap();
        // Same payload, edited: the MAC no longer fits.
        let edited = String::from_utf8(unhex(payload).unwrap())
            .unwrap()
            .replace("destroy", "destroz");
        let forged = json!(format!("{}.{mac}", hex(edited.as_bytes())));
        assert_eq!(
            c.answer(Some(&forged), Some(&accept()), "destroz", &args, "claude"),
            Answer::Ask
        );
        // Another process's state (another key).
        let other = json!(Confirmations::new().issue("destroy", &args, "claude"));
        assert_eq!(
            c.answer(Some(&other), Some(&accept()), "destroy", &args, "claude"),
            Answer::Ask
        );
        for junk in [json!("nonsense"), json!(42), json!("ab.cd")] {
            assert_eq!(
                c.answer(Some(&junk), Some(&accept()), "destroy", &args, "claude"),
                Answer::Ask
            );
        }
    }

    #[test]
    fn an_expired_state_is_asked_again() {
        let c = Confirmations::new();
        let args = json!([]);
        let payload = hex(
            json!({ "n": "x", "t": "t", "a": digest(&args), "c": "c", "e": now_ms() - 1 })
                .to_string()
                .as_bytes(),
        );
        let mac = hex(&c.mac(&payload).finalize().into_bytes());
        let state = json!(format!("{payload}.{mac}"));
        assert_eq!(
            c.answer(Some(&state), Some(&accept()), "t", &args, "c"),
            Answer::Ask
        );
    }

    #[test]
    fn decline_cancel_and_no_answer() {
        let c = Confirmations::new();
        let args = json!([1]);
        let state = json!(c.issue("t", &args, "c"));
        let with = |r: Value| c.answer(Some(&state), Some(&r), "t", &args, "c");
        assert_eq!(
            with(json!({ KEY: { "action": "decline" } })),
            Answer::Declined
        );
        assert_eq!(
            with(json!({ KEY: { "action": "cancel" } })),
            Answer::Declined
        );
        assert_eq!(with(json!({})), Answer::Ask);
        assert_eq!(
            with(json!({ "other": { "action": "accept" } })),
            Answer::Ask
        );
        assert_eq!(
            c.answer(None, Some(&accept()), "t", &args, "c"),
            Answer::Ask,
            "no state"
        );
    }

    #[test]
    fn capabilities() {
        assert!(can_elicit(Some(&json!({ "elicitation": {} }))));
        assert!(can_elicit(Some(&json!({ "elicitation": { "form": {} } }))));
        assert!(
            !can_elicit(Some(&json!({ "elicitation": { "url": {} } }))),
            "URL mode only"
        );
        assert!(!can_elicit(Some(&json!({}))));
        assert!(!can_elicit(None));
    }
}
