//! Shown at once (RFC 0039): a write marked `.optimistic()` is applied to
//! every read of its record and of its lists while it's on its way or waits
//! in the outbox, so every window shows it before the server answers, and
//! a re-read doesn't flicker back. A refusal is no longer applied, and the
//! lists re-run, so the change is taken back.
//!
//! Nothing here is stored: the in-flight writes are in memory, and the
//! waiting ones are read from the outbox, whose rows say they're
//! optimistic. The offline copy stays what the server said.

use std::collections::BTreeMap;

use parking_lot::Mutex;
use serde_json::{Map, Value};

use crate::backend::{is_id, resource_key};
use crate::event::EventBus;
use crate::http::Method;

/// The channel `$pending` follows.
pub const CHANNEL: &str = "elyra:pending";

/// The outbox row's mark: the write is optimistic, with the fields it
/// changes when they aren't its body. Never sent.
#[cfg_attr(not(feature = "database"), allow(dead_code))]
pub(crate) const MARK: &str = "x-elyra-optimistic";

/// A write not yet confirmed by the server.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Change {
    /// Its `Idempotency-Key`, or a key of its own while in flight.
    pub key: String,
    /// The server and the offline owner it was made as.
    pub server: Option<String>,
    pub owner: Option<String>,
    pub method: Method,
    /// The record's path, without a query: `/api/customers/42`.
    pub path: String,
    /// The fields it sets; `None` for a `DELETE`.
    pub fields: Option<Map<String, Value>>,
}

impl Change {
    /// A write to a record, or `None` for one that isn't: a `POST`, or a
    /// path that doesn't end in an id.
    pub(crate) fn new(
        key: String,
        server: Option<String>,
        owner: Option<String>,
        method: Method,
        path: &str,
        fields: Option<Value>,
    ) -> Option<Self> {
        let path = without_query(path).trim_end_matches('/').to_owned();
        if !matches!(method, Method::Put | Method::Patch | Method::Delete) {
            return None;
        }
        if !path.rsplit('/').next().is_some_and(is_id) {
            return None;
        }
        let fields = match (method, fields) {
            (Method::Delete, _) => None,
            (_, Some(Value::Object(map))) => Some(map),
            // A body that isn't an object changes nothing that's shown.
            _ => Some(Map::new()),
        };
        Some(Self {
            key,
            server,
            owner,
            method,
            path,
            fields,
        })
    }

    /// The record's id, as its path names it.
    fn id(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or_default()
    }

    /// The path of its lists: `/api/customers` for `/api/customers/42`.
    fn lists(&self) -> &str {
        self.path
            .rsplit_once('/')
            .map_or(self.path.as_str(), |(lists, _)| lists)
    }

    /// Whether a read of `path` shows this record: the record itself, or
    /// one of its lists.
    pub(crate) fn touches(&self, path: &str) -> bool {
        let bare = without_query(path).trim_end_matches('/');
        if bare == self.path {
            return true;
        }
        let last = bare.rsplit('/').next().unwrap_or_default();
        !is_id(last) && resource_key(bare) == resource_key(&self.path) && bare == self.lists()
    }

    fn mine(&self, server: Option<&str>, owner: Option<&str>) -> bool {
        self.server.as_deref() == server && self.owner.as_deref() == owner
    }
}

/// Apply `change` to the JSON a read of `path` answered.
pub(crate) fn apply(change: &Change, path: &str, body: &mut Value, record_field: &str) {
    let bare = without_query(path).trim_end_matches('/');
    if bare == change.path {
        // The record: an API Resource's `data`, or the model itself.
        if let Some(fields) = &change.fields {
            let record = match body {
                Value::Object(map) if map.get("data").is_some_and(Value::is_object) => {
                    map.get_mut("data")
                }
                other => Some(other),
            };
            if let Some(Value::Object(record)) = record {
                merge(record, fields);
            }
        }
        return;
    }
    // A list: a page's `data`, or the array itself.
    let removed = {
        let items = match body {
            Value::Array(items) => Some(items),
            Value::Object(map) => map.get_mut("data").and_then(Value::as_array_mut),
            _ => None,
        };
        let Some(items) = items else {
            return;
        };
        let at = items
            .iter()
            .position(|item| same_id(item.get(record_field), change.id()));
        match (at, &change.fields) {
            (Some(at), Some(fields)) => {
                if let Some(record) = items[at].as_object_mut() {
                    merge(record, fields);
                }
                false
            }
            (Some(at), None) => {
                items.remove(at);
                true
            }
            (None, _) => false,
        }
    };
    if removed {
        // A page's count: Laravel's paginator (`total`), or an API
        // Resource collection's (`meta.total`).
        if let Value::Object(map) = body {
            if let Some(total) = map.get_mut("total") {
                decrement(total);
            }
            if let Some(total) = map.get_mut("meta").and_then(|m| m.get_mut("total")) {
                decrement(total);
            }
        }
    }
}

/// Only the fields the record already has: one the server doesn't send back
/// (`password`) isn't added.
fn merge(record: &mut Map<String, Value>, fields: &Map<String, Value>) {
    for (name, value) in fields {
        if let Some(field) = record.get_mut(name) {
            *field = value.clone();
        }
    }
}

fn decrement(total: &mut Value) {
    if let Some(n) = total.as_u64() {
        *total = Value::from(n.saturating_sub(1));
    }
}

fn same_id(value: Option<&Value>, id: &str) -> bool {
    match value {
        Some(Value::Number(n)) => n.to_string() == id,
        Some(Value::String(s)) => s == id,
        _ => false,
    }
}

fn without_query(path: &str) -> &str {
    path.split(['?', '#']).next().unwrap_or(path)
}

/// The writes not yet confirmed: those on their way, and those waiting in
/// the outbox. Shared by a backend's clones.
#[derive(Default)]
pub(crate) struct Pending {
    in_flight: Mutex<Vec<Change>>,
    queued: Mutex<Vec<Change>>,
    bus: Mutex<Option<EventBus>>,
    /// What `$pending` last said, to publish only a change.
    shown: Mutex<BTreeMap<String, Vec<Value>>>,
    /// `.optimistic()` without `.offline()` was warned of.
    warned: std::sync::atomic::AtomicBool,
}

impl Pending {
    pub(crate) fn publish_on(&self, bus: EventBus) {
        *self.bus.lock() = Some(bus);
    }

    /// A write starts.
    pub(crate) fn start(&self, change: Change) {
        self.in_flight.lock().push(change);
        self.publish();
    }

    /// A write was answered, or failed, or went to the outbox.
    pub(crate) fn end(&self, key: &str) {
        self.in_flight.lock().retain(|c| c.key != key);
        self.publish();
    }

    /// A write left the network for the outbox: it stays pending there,
    /// under the outbox's key, until the outbox is read again.
    pub(crate) fn queue(&self, key: &str, queued_as: &str) {
        let moved = {
            let mut in_flight = self.in_flight.lock();
            let at = in_flight.iter().position(|c| c.key == key);
            at.map(|at| in_flight.remove(at))
        };
        if let Some(mut change) = moved {
            change.key = queued_as.to_owned();
            let mut queued = self.queued.lock();
            if !queued.iter().any(|c| c.key == change.key) {
                queued.push(change);
            }
        }
        self.publish();
    }

    /// The outbox's optimistic writes, as it has them now. Whether that
    /// changed anything.
    #[cfg_attr(not(feature = "database"), allow(dead_code))]
    pub(crate) fn use_queued(&self, changes: Vec<Change>) -> bool {
        let changed = {
            let mut queued = self.queued.lock();
            let changed = *queued != changes;
            *queued = changes;
            changed
        };
        if changed {
            self.publish();
        }
        changed
    }

    /// The paths of the outbox's writes shown at once.
    #[cfg(feature = "database")]
    pub(crate) fn queued_paths(&self) -> Vec<String> {
        self.queued.lock().iter().map(|c| c.path.clone()).collect()
    }

    /// Every pending write that shows in a read of `path`, oldest first.
    pub(crate) fn touching(
        &self,
        server: Option<&str>,
        owner: Option<&str>,
        path: &str,
    ) -> Vec<Change> {
        let queued = self.queued.lock().clone();
        let in_flight = self.in_flight.lock().clone();
        queued
            .into_iter()
            .chain(in_flight)
            .filter(|c| c.mine(server, owner) && c.touches(path))
            .collect()
    }

    /// Whether a write on its way (not one waiting) shows in a read of
    /// `path`: that read answers from the copy at once.
    #[cfg_attr(not(feature = "database"), allow(dead_code))]
    pub(crate) fn in_flight_touches(
        &self,
        server: Option<&str>,
        owner: Option<&str>,
        path: &str,
    ) -> bool {
        self.in_flight
            .lock()
            .iter()
            .any(|c| c.mine(server, owner) && c.touches(path))
    }

    /// Warn once that `.optimistic()` shows a change only after a round trip
    /// without `.offline()`.
    pub(crate) fn warn_without_copy(&self) {
        if !self.warned.swap(true, std::sync::atomic::Ordering::SeqCst) {
            crate::warn!(
                target: "elyra::backend",
                ".optimistic() without Backend::offline(): a change shows only after the \
                 server answers; add .offline() to show it at once"
            );
        }
    }

    /// `$pending`: per resource path (`customers`, `customers/42/notes`),
    /// the records with a change not yet sent.
    pub(crate) fn shown(&self) -> BTreeMap<String, Vec<Value>> {
        let mut shown: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        let queued = self.queued.lock().clone();
        let in_flight = self.in_flight.lock().clone();
        for change in queued.into_iter().chain(in_flight) {
            let resource = change
                .lists()
                .trim_start_matches('/')
                .trim_start_matches("api/")
                .to_owned();
            let id = change.id();
            let id = id
                .parse::<i64>()
                .map(Value::from)
                .unwrap_or_else(|_| Value::from(id));
            let ids = shown.entry(resource).or_default();
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        shown
    }

    fn publish(&self) {
        let now = self.shown();
        {
            let mut shown = self.shown.lock();
            if *shown == now {
                return;
            }
            *shown = now.clone();
        }
        if let Some(bus) = self.bus.lock().as_ref() {
            let _ = bus.emit(CHANNEL, &now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn put(path: &str, fields: Value) -> Change {
        Change::new("k".into(), None, None, Method::Put, path, Some(fields)).unwrap()
    }

    fn delete(path: &str) -> Change {
        Change::new("k".into(), None, None, Method::Delete, path, None).unwrap()
    }

    #[test]
    fn only_a_write_to_a_record_is_optimistic() {
        assert!(
            Change::new("k".into(), None, None, Method::Post, "/api/customers", None).is_none()
        );
        assert!(Change::new("k".into(), None, None, Method::Put, "/api/customers", None).is_none());
        assert!(Change::new(
            "k".into(),
            None,
            None,
            Method::Patch,
            "/api/customers/42?x=1",
            None
        )
        .is_some());
    }

    #[test]
    fn it_touches_its_record_and_its_lists_only() {
        let change = put("/api/customers/42", json!({}));
        assert!(change.touches("/api/customers/42"));
        assert!(change.touches("/api/customers?page=2&search=ac"));
        assert!(!change.touches("/api/customers/43"));
        assert!(!change.touches("/api/customers/42/notes"));
        assert!(!change.touches("/api/orders"));
        let nested = put("/api/customers/42/notes/7", json!({}));
        assert!(nested.touches("/api/customers/42/notes"));
        assert!(nested.touches("/api/customers/42/notes/7"));
        assert!(!nested.touches("/api/customers/43/notes"));
        assert!(!nested.touches("/api/customers"));
    }

    #[test]
    fn a_record_gets_only_the_fields_it_has() {
        let change = put(
            "/api/customers/42",
            json!({ "name": "Acme AS", "password": "x", "address": { "city": "Oslo" } }),
        );
        let mut plain =
            json!({ "id": 42, "name": "Acme", "address": { "city": "Bergen" }, "updated_at": "t" });
        apply(&change, "/api/customers/42", &mut plain, "id");
        assert_eq!(
            plain,
            json!({ "id": 42, "name": "Acme AS", "address": { "city": "Oslo" }, "updated_at": "t" })
        );
        let mut wrapped = json!({ "data": { "id": 42, "name": "Acme" } });
        apply(&change, "/api/customers/42", &mut wrapped, "id");
        assert_eq!(wrapped, json!({ "data": { "id": 42, "name": "Acme AS" } }));
    }

    #[test]
    fn a_list_s_item_changes_and_a_delete_takes_it_out() {
        let change = put("/api/customers/42", json!({ "name": "Acme AS" }));
        let mut page = json!({ "data": [{ "id": 41, "name": "B" }, { "id": 42, "name": "Acme" }], "total": 2 });
        apply(&change, "/api/customers?page=1", &mut page, "id");
        assert_eq!(page["data"][1]["name"], "Acme AS");
        assert_eq!(page["data"][0]["name"], "B");

        let mut array = json!([{ "id": "42", "name": "Acme" }]);
        apply(&change, "/api/customers", &mut array, "id");
        assert_eq!(array[0]["name"], "Acme AS");

        let gone = delete("/api/customers/42");
        apply(&gone, "/api/customers", &mut page, "id");
        assert_eq!(page["data"].as_array().unwrap().len(), 1);
        assert_eq!(page["total"], 1);
        let mut resource = json!({ "data": [{ "id": 42 }], "meta": { "total": 1 } });
        apply(&gone, "/api/customers", &mut resource, "id");
        assert_eq!(resource, json!({ "data": [], "meta": { "total": 0 } }));
    }

    #[test]
    fn a_list_keyed_otherwise_is_matched_by_its_key() {
        // RFC 0017's `.record_key("invoices", "number")`.
        let change = put("/api/invoices/1001", json!({ "paid": true }));
        let mut page = json!({ "data": [{ "id": 1, "number": 1001, "paid": false }, { "id": 1001, "number": 7, "paid": false }] });
        apply(&change, "/api/invoices", &mut page, "number");
        assert_eq!(page["data"][0]["paid"], true);
        assert_eq!(page["data"][1]["paid"], false);
    }

    #[test]
    fn writes_apply_in_order_and_pending_says_which() {
        let pending = Pending::default();
        pending.start(
            Change::new(
                "a".into(),
                None,
                None,
                Method::Put,
                "/api/customers/42",
                Some(json!({ "name": "One" })),
            )
            .unwrap(),
        );
        pending.start(
            Change::new(
                "b".into(),
                None,
                None,
                Method::Patch,
                "/api/customers/42",
                Some(json!({ "name": "Two" })),
            )
            .unwrap(),
        );
        pending.start(
            Change::new(
                "c".into(),
                None,
                None,
                Method::Delete,
                "/api/customers/42/notes/7",
                None,
            )
            .unwrap(),
        );
        let mut record = json!({ "id": 42, "name": "Acme" });
        for change in pending.touching(None, None, "/api/customers/42") {
            apply(&change, "/api/customers/42", &mut record, "id");
        }
        assert_eq!(record["name"], "Two");
        assert_eq!(
            pending.shown(),
            BTreeMap::from([
                ("customers".to_owned(), vec![json!(42)]),
                ("customers/42/notes".to_owned(), vec![json!(7)]),
            ])
        );
        pending.queue("a", "outbox-a");
        assert_eq!(pending.touching(None, None, "/api/customers").len(), 2);
        assert!(!pending.in_flight_touches(None, None, "/api/orders"));
        pending.end("b");
        pending.use_queued(Vec::new());
        assert_eq!(pending.touching(None, None, "/api/customers/42").len(), 0);
    }

    #[test]
    fn another_server_s_or_user_s_write_isn_t_shown() {
        let pending = Pending::default();
        pending.start(
            Change::new(
                "a".into(),
                Some("https://a".into()),
                Some("ada".into()),
                Method::Put,
                "/api/customers/42",
                Some(json!({})),
            )
            .unwrap(),
        );
        assert_eq!(
            pending
                .touching(Some("https://a"), Some("ada"), "/api/customers")
                .len(),
            1
        );
        assert!(pending
            .touching(Some("https://b"), Some("ada"), "/api/customers")
            .is_empty());
        assert!(pending
            .touching(Some("https://a"), Some("bob"), "/api/customers")
            .is_empty());
    }
}
