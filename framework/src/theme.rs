//! The Theme Studio's draft (RFC 0037), in a debug build: the design tokens
//! the developer is trying, kept for this run and sent to every window on
//! `elyra:theme-draft`, so each window and each matrix frame (RFC 0034)
//! draws them in its own look. Nothing is written to a file.

// A release keeps no draft: what changes it isn't compiled there.
#![cfg_attr(not(debug_assertions), allow(dead_code))]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The tokens a draft may set: `@elyra/ui/tokens.css`'s, and nothing else.
pub(crate) const TOKENS: &[&str] = &[
    "--elyra-bg",
    "--elyra-surface",
    "--elyra-raised",
    "--elyra-inset",
    "--elyra-text",
    "--elyra-muted",
    "--elyra-border",
    "--elyra-focus",
    "--elyra-accent",
    "--elyra-accent-soft",
    "--elyra-on-accent",
    "--elyra-danger",
    "--elyra-success",
    "--elyra-warning",
    "--elyra-shadow",
    "--elyra-overlay",
    "--elyra-radius",
    "--elyra-radius-lg",
    "--elyra-space",
    "--elyra-font",
    "--elyra-font-mono",
    "--elyra-font-size",
];

/// Which look a value is for. `all` is the shape, the space and the fonts,
/// the same in every look.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Look {
    All,
    Dark,
    Light,
    DarkMore,
    LightMore,
}

/// The draft: per look, token → value.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub(crate) struct Draft {
    pub(crate) looks: BTreeMap<Look, BTreeMap<String, String>>,
}

/// One change, as the studio sends it: a value, or `None` to put the
/// token back.
#[derive(Debug, Deserialize)]
pub(crate) struct Change {
    pub(crate) look: Look,
    pub(crate) token: String,
    #[serde(default)]
    pub(crate) value: Option<String>,
}

/// Why a change is refused, or `None`: a token `tokens.css` has, and a
/// value that can't close the declaration or reach anything outside it.
pub(crate) fn refused(change: &Change) -> Option<String> {
    if !TOKENS.contains(&change.token.as_str()) {
        return Some(format!("not a design token: {:?}", change.token));
    }
    let Some(value) = &change.value else {
        return None;
    };
    let lower = value.to_ascii_lowercase();
    let bad = value.is_empty()
        || value.len() > 200
        || value.contains([';', '{', '}', '<', '>', '\\', '\n', '\r'])
        || lower.contains("url(")
        || lower.contains("/*")
        || lower.contains("@import")
        || lower.contains("expression(");
    bad.then(|| format!("not a value for {}: {value:?}", change.token))
}

/// The draft of this run, shared by the shell and every window.
#[derive(Default)]
pub(crate) struct Drafts {
    draft: parking_lot::Mutex<Draft>,
}

impl Drafts {
    pub(crate) fn get(&self) -> Draft {
        self.draft.lock().clone()
    }

    /// Apply `change` (already checked): the draft after it.
    pub(crate) fn apply(&self, change: Change) -> Draft {
        let mut draft = self.draft.lock();
        let look = draft.looks.entry(change.look).or_default();
        match change.value {
            Some(value) => {
                look.insert(change.token, value.trim().to_owned());
            }
            None => {
                look.remove(&change.token);
            }
        }
        draft.looks.retain(|_, tokens| !tokens.is_empty());
        draft.clone()
    }

    /// Forget every change: the draft after it, empty.
    pub(crate) fn reset(&self) -> Draft {
        let mut draft = self.draft.lock();
        *draft = Draft::default();
        draft.clone()
    }

    /// What a window opened now is told before its script runs, if there's
    /// a draft.
    pub(crate) fn init_script(&self) -> Option<String> {
        let draft = self.draft.lock();
        if draft.looks.is_empty() {
            return None;
        }
        let json = serde_json::to_string(&*draft).ok()?;
        Some(format!("globalThis.__ELYRA_THEME_DRAFT__ = {json};"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(look: Look, token: &str, value: Option<&str>) -> Change {
        Change {
            look,
            token: token.into(),
            value: value.map(str::to_owned),
        }
    }

    #[test]
    fn a_draft_takes_tokens_per_look_and_gives_them_back() {
        let drafts = Drafts::default();
        assert_eq!(drafts.init_script(), None);
        drafts.apply(change(Look::Dark, "--elyra-accent", Some(" #f97316 ")));
        drafts.apply(change(Look::Light, "--elyra-accent", Some("#c2410c")));
        let draft = drafts.apply(change(Look::All, "--elyra-radius", Some("10px")));
        assert_eq!(draft.looks[&Look::Dark]["--elyra-accent"], "#f97316");
        assert_eq!(draft.looks[&Look::All]["--elyra-radius"], "10px");
        let script = drafts.init_script().unwrap();
        assert!(script.starts_with("globalThis.__ELYRA_THEME_DRAFT__ = {\"looks\":{\"all\":{\"--elyra-radius\":\"10px\"},\"dark\":"), "{script}");

        let draft = drafts.apply(change(Look::Dark, "--elyra-accent", None));
        assert!(!draft.looks.contains_key(&Look::Dark), "an empty look goes");
        assert!(drafts.reset().looks.is_empty());
        assert_eq!(drafts.init_script(), None);
    }

    #[test]
    fn only_tokens_and_plain_values_are_taken() {
        let ok =
            |token: &str, value: &str| refused(&change(Look::Dark, token, Some(value))).is_none();
        assert!(ok("--elyra-accent", "#f97316"));
        assert!(ok("--elyra-shadow", "0 16px 40px rgba(0, 0, 0, 0.45)"));
        assert!(ok("--elyra-font", "\"Inter\", system-ui, sans-serif"));
        assert!(!ok("--elyra-nope", "#fff"));
        assert!(!ok("color", "red"));
        assert!(!ok("--elyra-accent", "red; } body { display: none"));
        assert!(!ok("--elyra-accent", "url(https://evil.test/x.png)"));
        assert!(!ok("--elyra-accent", "red /* */"));
        assert!(!ok("--elyra-accent", "</style><script>"));
        assert!(!ok("--elyra-accent", ""));
        assert!(
            refused(&change(Look::Dark, "--elyra-accent", None)).is_none(),
            "a reset"
        );
    }
}
