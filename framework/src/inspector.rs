//! The developer's inspector (RFC 0014), in debug builds only.
//!
//! Its ability simulator is a [`Mask`]: abilities the developer turned off
//! for this run. The mask is checked where every ability check already
//! goes (the shell's guard, an AI caller's `denied`, [`Abilities::allows`]
//! and the state the runtime's `can` store reads), so the button, the
//! command, the assistant's tool, the window note and an MCP agent all lose
//! the ability at once. It only takes away: an unmasked ability is
//! whatever the app and Laravel granted.
//!
//! In a release build the mask is empty and can't be set, and the
//! inspector's routes don't exist.
//!
//! [`Abilities::allows`]: crate::abilities::Abilities::allows

#[cfg(debug_assertions)]
use std::collections::BTreeSet;
#[cfg(debug_assertions)]
use std::sync::Arc;

#[cfg(debug_assertions)]
use parking_lot::RwLock;

/// `App::inspector(false)`: the routes answer as if there were none.
#[derive(Clone, Copy)]
pub(crate) struct InspectorOff;

/// The abilities turned off in the inspector, shared by everything that
/// checks one. Bound in the container by [`crate::App`].
#[derive(Clone, Default)]
pub struct Mask {
    #[cfg(debug_assertions)]
    off: Arc<RwLock<BTreeSet<String>>>,
}

impl Mask {
    /// Whether `ability` is turned off: masked itself, or by a masked
    /// namespace (`customers.*`). Never, in a release build.
    pub fn masks(&self, ability: &str) -> bool {
        #[cfg(debug_assertions)]
        {
            self.off
                .read()
                .iter()
                .any(|off| crate::security::ability_matches(off, ability))
        }
        #[cfg(not(debug_assertions))]
        {
            let _ = ability;
            false
        }
    }

    /// What's turned off, in order.
    pub fn list(&self) -> Vec<String> {
        #[cfg(debug_assertions)]
        {
            self.off.read().iter().cloned().collect()
        }
        #[cfg(not(debug_assertions))]
        {
            Vec::new()
        }
    }

    /// Turn `ability` off, or back on. Whether anything changed. Debug
    /// builds only; the drawer does it through `/__inspector/mask`, and a
    /// test can too.
    #[cfg(debug_assertions)]
    pub fn set(&self, ability: &str, off: bool) -> bool {
        let mut set = self.off.write();
        if off {
            set.insert(ability.to_owned())
        } else {
            set.remove(ability)
        }
    }

    /// The reason a masked ability gives, where a call is refused.
    pub(crate) const REASON: &'static str = "simulated by the inspector";
}

/// Why a call needing `ability` is refused by the mask, if it is: the shared
/// wording of the guard's and an AI caller's refusal.
pub(crate) fn masked(mask: Option<&Mask>, ability: &str) -> Option<String> {
    mask.filter(|mask| mask.masks(ability)).map(|_| {
        format!(
            "you may not do this: it takes the `{ability}` ability ({})",
            Mask::REASON
        )
    })
}

#[cfg(all(test, debug_assertions))]
mod tests {
    use super::*;

    #[test]
    fn a_mask_takes_abilities_and_namespaces_away() {
        let mask = Mask::default();
        assert!(!mask.masks("customers.delete"));
        assert!(mask.set("customers.delete", true));
        assert!(!mask.set("customers.delete", true), "already off");
        assert!(mask.masks("customers.delete"));
        assert!(!mask.masks("customers.view"));
        mask.set("invoices.*", true);
        assert!(mask.masks("invoices.send"));
        assert_eq!(mask.list(), ["customers.delete", "invoices.*"]);
        assert!(mask.set("customers.delete", false));
        assert!(!mask.masks("customers.delete"));
        // A clone is the same mask.
        let shared = mask.clone();
        shared.set("orders.view", true);
        assert!(mask.masks("orders.view"));
    }

    #[test]
    fn a_masked_ability_is_refused_with_its_reason() {
        let mask = Mask::default();
        assert_eq!(masked(Some(&mask), "a.b"), None);
        assert_eq!(masked(None, "a.b"), None);
        mask.set("a.b", true);
        assert_eq!(
            masked(Some(&mask), "a.b").unwrap(),
            "you may not do this: it takes the `a.b` ability (simulated by the inspector)"
        );
    }
}
