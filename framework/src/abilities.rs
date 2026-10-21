//! What the signed-in user may do, by Laravel (RFC 0010). Behind the
//! `backend` feature, turned on with
//! [`Backend::abilities`](crate::backend::Backend::abilities), and in the
//! container as [`Abilities`].
//!
//! After sign-in the list is read from `GET /api/abilities`: strings such as
//! `customers.delete`, where a trailing `*` covers a namespace. Signing out
//! forgets it. With `Backend::offline()`, the last list is the user's offline
//! copy, so it holds while the server can't be reached. It's published on
//! `elyra:abilities` for the frontend's `can` store.

use std::sync::Arc;

use parking_lot::RwLock;
use serde::Serialize;
use serde_json::Value;

use crate::backend::{Backend, BackendError};
use crate::event::EventBus;

/// The channel the list is published on.
pub const CHANNEL: &str = "elyra:abilities";

/// The user's abilities, as the frontend's store has them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, specta::Type)]
pub struct AbilitiesState {
    /// Whether the list has been read since the user signed in.
    pub loaded: bool,
    pub abilities: Vec<String>,
}

struct Inner {
    backend: Backend,
    route: String,
    /// `None` until read, and again once the user signs out.
    list: RwLock<Option<Vec<String>>>,
    bus: Option<EventBus>,
    reading: tokio::sync::Mutex<()>,
}

/// What the signed-in user may do, as Laravel says.
#[derive(Clone)]
pub struct Abilities {
    inner: Arc<Inner>,
}

impl Abilities {
    pub(crate) fn new(backend: Backend, route: String, bus: Option<EventBus>) -> Self {
        Self {
            inner: Arc::new(Inner {
                backend,
                route,
                list: RwLock::new(None),
                bus,
                reading: tokio::sync::Mutex::new(()),
            }),
        }
    }

    /// Whether the signed-in user has `ability`. Signed out, or before the
    /// list has been read: no.
    pub fn allows(&self, ability: &str) -> bool {
        self.inner.list.read().as_ref().is_some_and(|list| {
            list.iter()
                .any(|granted| crate::security::ability_matches(granted, ability))
        })
    }

    /// What Laravel said this user may do.
    pub fn list(&self) -> Vec<String> {
        self.inner.list.read().clone().unwrap_or_default()
    }

    /// Whether the list has been read since the user signed in.
    pub fn loaded(&self) -> bool {
        self.inner.list.read().is_some()
    }

    pub fn state(&self) -> AbilitiesState {
        let list = self.inner.list.read().clone();
        AbilitiesState {
            loaded: list.is_some(),
            abilities: list.unwrap_or_default(),
        }
    }

    /// Read the list from Laravel now. Offline, with `Backend::offline()`,
    /// it's the user's last copy.
    pub async fn refresh(&self) -> Result<Vec<String>, BackendError> {
        let _reading = self.inner.reading.lock().await;
        if !self.inner.backend.has_token() {
            self.forget();
            return Ok(Vec::new());
        }
        let answer: Value = self.inner.backend.get(&self.inner.route).json().await?;
        let list = parse(answer).ok_or_else(|| BackendError::Other {
            status: 200,
            message: format!(
                "{} should answer a list of abilities, like [\"customers.view\"]",
                self.inner.route
            ),
        })?;
        // Signed out while it was read: it isn't anyone's now.
        if self.inner.backend.has_token() {
            self.set(Some(list.clone()));
        }
        Ok(list)
    }

    /// Read the list in the background; a failure leaves what's there.
    pub(crate) fn spawn_refresh(&self) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let this = self.clone();
        handle.spawn(async move {
            if let Err(e) = this.refresh().await {
                crate::warn!(target: "elyra::abilities", "couldn't read the abilities: {e}");
            }
        });
    }

    /// Signed out: nobody's abilities.
    pub(crate) fn forget(&self) {
        self.set(None);
    }

    fn set(&self, list: Option<Vec<String>>) {
        {
            let mut current = self.inner.list.write();
            if *current == list {
                return;
            }
            *current = list;
        }
        if let Some(bus) = &self.inner.bus {
            let _ = bus.emit(CHANNEL, &self.state());
        }
    }
}

impl crate::security::UserAbilities for Abilities {
    fn user_allows(&self, ability: &str) -> bool {
        self.allows(ability)
    }
}

/// `["a", "b"]`, or the same in `{ "data": [..] }` or `{ "abilities": [..] }`.
fn parse(answer: Value) -> Option<Vec<String>> {
    let list = match answer {
        Value::Array(list) => list,
        Value::Object(mut map) => match map.remove("data").or_else(|| map.remove("abilities")) {
            Some(Value::Array(list)) => list,
            _ => return None,
        },
        _ => return None,
    };
    list.into_iter()
        .map(|v| v.as_str().map(str::to_owned))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_list_is_read_in_laravels_shapes() {
        let list = || Some(vec!["customers.view".to_string(), "invoices.*".to_string()]);
        assert_eq!(parse(json!(["customers.view", "invoices.*"])), list());
        assert_eq!(
            parse(json!({ "data": ["customers.view", "invoices.*"] })),
            list()
        );
        assert_eq!(
            parse(json!({ "abilities": ["customers.view", "invoices.*"] })),
            list()
        );
        assert_eq!(parse(json!([])), Some(vec![]));
        assert_eq!(parse(json!({ "can": [] })), None);
        assert_eq!(parse(json!(["ok", 3])), None);
        assert_eq!(parse(json!("customers.view")), None);
    }

    #[test]
    fn nothing_is_allowed_until_read_and_wildcards_cover_a_namespace() {
        let abilities = Abilities::new(
            Backend::new("https://crm.test"),
            "/api/abilities".into(),
            None,
        );
        assert!(!abilities.allows("customers.view"));
        assert!(!abilities.loaded());
        abilities.set(Some(vec!["customers.view".into(), "invoices.*".into()]));
        assert!(abilities.allows("customers.view"));
        assert!(!abilities.allows("customers.delete"));
        assert!(abilities.allows("invoices.delete"));
        assert_eq!(
            abilities.state(),
            AbilitiesState {
                loaded: true,
                abilities: vec!["customers.view".into(), "invoices.*".into()]
            }
        );
        abilities.forget();
        assert!(!abilities.allows("customers.view"));
        assert_eq!(abilities.state(), AbilitiesState::default());
    }
}
