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

/// The event Laravel broadcasts on the user's channel when what they may do
/// changed (`elyracode/elyra-backend` dispatches it for spatie's roles).
pub const CHANGED_EVENT: &str = "AbilitiesChanged";

/// The live-query key a command that asked [`Abilities::allows`] depends on.
pub const LIVE_KEY: &str = "elyra:abilities";

/// The user's abilities, as the frontend's store has them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, specta::Type)]
pub struct AbilitiesState {
    /// Whether the list has been read since the user signed in.
    pub loaded: bool,
    pub abilities: Vec<String>,
    /// Turned off in the inspector (RFC 0014), in a debug build: `can`
    /// says no to these whatever the list says. Left out when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub masked: Vec<String>,
}

struct Inner {
    backend: Backend,
    route: String,
    /// `None` until read, and again once the user signs out.
    list: RwLock<Option<Vec<String>>>,
    bus: Option<EventBus>,
    reading: tokio::sync::Mutex<()>,
    /// Re-runs the live queries that asked, when the list changes.
    changed: RwLock<Option<Arc<dyn Fn() + Send + Sync>>>,
    /// The inspector's mask (RFC 0014).
    mask: RwLock<crate::inspector::Mask>,
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
                changed: RwLock::new(None),
                mask: RwLock::new(crate::inspector::Mask::default()),
            }),
        }
    }

    /// Check every ability against the inspector's `mask` too (RFC 0014).
    pub(crate) fn masked_by(self, mask: crate::inspector::Mask) -> Self {
        *self.inner.mask.write() = mask;
        self
    }

    /// Whether the signed-in user has `ability`. Signed out, or before the
    /// list has been read: no. A live command that asks re-runs when the
    /// list changes.
    pub fn allows(&self, ability: &str) -> bool {
        #[cfg(feature = "database")]
        elyra_db::live::depends_on(LIVE_KEY);
        if self.inner.mask.read().masks(ability) {
            return false;
        }
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
            masked: self.inner.mask.read().list(),
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

    /// Run `f` when the list changes (the app re-runs live queries).
    #[cfg_attr(not(feature = "database"), allow(dead_code))]
    pub(crate) fn on_change(&self, f: impl Fn() + Send + Sync + 'static) {
        *self.inner.changed.write() = Some(Arc::new(f));
    }

    /// A user who has `list`, without a backend to ask.
    #[cfg(test)]
    pub(crate) fn of(list: &[&str]) -> Self {
        let abilities = Self::new(
            Backend::new("https://crm.test"),
            "/api/abilities".into(),
            None,
        );
        abilities.set(Some(list.iter().map(|a| (*a).to_owned()).collect()));
        abilities
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
        let changed = self.inner.changed.read().clone();
        if let Some(changed) = changed {
            changed();
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
                abilities: vec!["customers.view".into(), "invoices.*".into()],
                masked: vec![],
            }
        );
        abilities.forget();
        assert!(!abilities.allows("customers.view"));
        assert_eq!(abilities.state(), AbilitiesState::default());
    }
    #[cfg(debug_assertions)]
    #[test]
    fn the_inspectors_mask_takes_an_ability_away_and_says_so() {
        let mask = crate::inspector::Mask::default();
        let abilities = Abilities::new(
            Backend::new("https://crm.test"),
            "/api/abilities".into(),
            None,
        )
        .masked_by(mask.clone());
        abilities.set(Some(vec!["customers.*".into()]));
        assert!(abilities.allows("customers.delete"));
        mask.set("customers.delete", true);
        assert!(!abilities.allows("customers.delete"));
        assert!(abilities.allows("customers.view"));
        assert_eq!(abilities.state().masked, ["customers.delete"]);
        // Unmasked, Laravel's list is the answer again.
        mask.set("customers.delete", false);
        assert!(abilities.allows("customers.delete"));
        assert!(abilities.state().masked.is_empty());
    }
}
