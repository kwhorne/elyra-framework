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

/// The calls a window made, as the inspector lists them (RFC 0014): names,
/// outcomes, timings and sizes, never arguments or results.
#[cfg(debug_assertions)]
#[derive(Clone, Default)]
pub(crate) struct Calls {
    windows: Arc<
        parking_lot::Mutex<std::collections::HashMap<String, std::collections::VecDeque<CallRow>>>,
    >,
}

/// One call.
#[cfg(debug_assertions)]
#[derive(Clone, Debug, serde::Serialize, PartialEq)]
pub(crate) struct CallRow {
    /// When it was answered, in milliseconds since the Unix epoch.
    pub(crate) at: u64,
    /// The command (`customers_update`), `live:customers_show` for a live
    /// query opened, or `assistant` for a turn asked.
    pub(crate) command: String,
    /// `ok`, or the error's kind (`denied`, `validation`, …; `error` without one).
    pub(crate) outcome: String,
    pub(crate) ms: u64,
    /// The request's and the answer's sizes, in bytes.
    pub(crate) sent: usize,
    pub(crate) received: usize,
    /// The ability a refused call needed.
    pub(crate) ability: Option<String>,
}

#[cfg(debug_assertions)]
impl Calls {
    /// Each window keeps its last this many.
    pub(crate) const KEEP: usize = 200;

    pub(crate) fn record(&self, window: &str, row: CallRow) {
        let mut windows = self.windows.lock();
        let calls = windows.entry(window.to_owned()).or_default();
        if calls.len() == Self::KEEP {
            calls.pop_front();
        }
        calls.push_back(row);
    }

    /// `window`'s calls, newest first.
    pub(crate) fn of(&self, window: &str) -> Vec<CallRow> {
        self.windows
            .lock()
            .get(window)
            .map(|calls| calls.iter().rev().cloned().collect())
            .unwrap_or_default()
    }
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

    fn call(command: &str) -> CallRow {
        CallRow {
            at: 1,
            command: command.into(),
            outcome: "ok".into(),
            ms: 2,
            sent: 3,
            received: 4,
            ability: None,
        }
    }

    #[test]
    fn a_window_keeps_its_last_calls_newest_first() {
        let calls = Calls::default();
        for i in 0..Calls::KEEP + 5 {
            calls.record("w1", call(&format!("c{i}")));
        }
        calls.record("w2", call("other"));
        let mine = calls.of("w1");
        assert_eq!(mine.len(), Calls::KEEP);
        assert_eq!(mine[0].command, format!("c{}", Calls::KEEP + 4));
        assert_eq!(mine.last().unwrap().command, "c5");
        assert_eq!(calls.of("w2").len(), 1);
        assert!(calls.of("w3").is_empty());
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
