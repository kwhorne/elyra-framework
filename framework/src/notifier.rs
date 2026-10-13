//! Notifications in the OS (RFC 0008): the [`Notifier`] in the container,
//! which shows them and says when one was clicked. `App` binds the system's;
//! a test swaps in [`Notifier::fake`] and asserts on what was shown.
//!
//! ```ignore
//! let app = TestApp::new(app().swap(Notifier::fake()));
//! // …something that notifies…
//! app.get::<Notifier>().assert_shown(|n| n.title == "Invoice paid");
//! ```

use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

use parking_lot::Mutex;

/// What the OS shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    pub title: String,
    pub body: Option<String>,
}

/// What to do when the user clicks it.
pub type OnClick = Box<dyn FnOnce() + Send>;

/// How many shown notifications are waited on for a click at a time; the
/// rest are shown all the same.
#[cfg(feature = "system")]
const MAX_WAITING: usize = 8;

/// Shows notifications in the OS. Cheap to clone.
#[derive(Clone)]
pub struct Notifier {
    kind: Kind,
}

/// What a fake was asked to show, with each one's click.
type Recorded = Arc<Mutex<Vec<(Shown, Option<OnClick>)>>>;

#[derive(Clone)]
enum Kind {
    System { waiting: Arc<AtomicUsize> },
    Fake(Recorded),
}

impl Default for Notifier {
    fn default() -> Self {
        Self::system()
    }
}

impl Notifier {
    /// The OS's notifications (the `system` feature; without it, showing one
    /// is an error). On macOS they need a bundled app.
    pub fn system() -> Self {
        Self {
            kind: Kind::System {
                waiting: Arc::new(AtomicUsize::new(0)),
            },
        }
    }

    /// One that records what it's asked to show, and shows nothing — for
    /// tests: `App::swap(Notifier::fake())`.
    pub fn fake() -> Self {
        Self {
            kind: Kind::Fake(Arc::new(Mutex::new(Vec::new()))),
        }
    }

    /// Show `shown`, and run `on_click` if the user clicks it.
    pub fn show(&self, shown: Shown, on_click: Option<OnClick>) -> Result<(), String> {
        match &self.kind {
            Kind::Fake(recorded) => {
                recorded.lock().push((shown, on_click));
                Ok(())
            }
            Kind::System { waiting } => system_show(shown, on_click, waiting.clone()),
        }
    }

    #[track_caller]
    fn recorded(&self) -> &Mutex<Vec<(Shown, Option<OnClick>)>> {
        match &self.kind {
            Kind::Fake(recorded) => recorded,
            Kind::System { .. } => {
                panic!("notification assertions need a fake: bind Notifier::fake() (App::swap)")
            }
        }
    }

    /// Everything shown so far, in order (fakes only).
    #[track_caller]
    pub fn shown(&self) -> Vec<Shown> {
        self.recorded()
            .lock()
            .iter()
            .map(|(s, _)| s.clone())
            .collect()
    }

    /// Assert one shown matches, and return it (fakes only).
    #[track_caller]
    pub fn assert_shown(&self, matches: impl Fn(&Shown) -> bool) -> Shown {
        let shown = self.shown();
        match shown.iter().find(|s| matches(s)) {
            Some(s) => s.clone(),
            None => panic!("expected a notification to be shown; shown: {shown:?}"),
        }
    }

    /// Assert nothing was shown (fakes only).
    #[track_caller]
    pub fn assert_nothing_shown(&self) {
        let shown = self.shown();
        assert!(
            shown.is_empty(),
            "expected no notification; shown: {shown:?}"
        );
    }

    /// Click the `index`th one shown, as the user would (fakes only). `false`
    /// when there's no such one, or it was clicked already.
    #[track_caller]
    pub fn click(&self, index: usize) -> bool {
        let on_click = self
            .recorded()
            .lock()
            .get_mut(index)
            .and_then(|(_, on_click)| on_click.take());
        match on_click {
            Some(f) => {
                f();
                true
            }
            None => false,
        }
    }
}

#[cfg(feature = "system")]
fn system_show(
    shown: Shown,
    on_click: Option<OnClick>,
    waiting: Arc<AtomicUsize>,
) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    let mut n = notify_rust::Notification::new();
    n.summary(&shown.title);
    if let Some(body) = &shown.body {
        n.body(body);
    }
    // D-Bus reports a click on the body only when the notification offers
    // the "default" action.
    #[cfg(all(unix, not(target_os = "macos")))]
    if on_click.is_some() {
        n.action("default", "Open");
    }
    let wait = on_click.filter(|_| {
        if waiting.fetch_add(1, Ordering::SeqCst) < MAX_WAITING {
            true
        } else {
            waiting.fetch_sub(1, Ordering::SeqCst);
            false
        }
    });
    let Some(on_click) = wait else {
        // Nothing to wait for (or already waiting on enough): fire and forget.
        return n.show().map(|_| ()).map_err(|e| e.to_string());
    };
    // Showing may block until the user acts (macOS), and waiting does: a
    // thread of its own.
    std::thread::Builder::new()
        .name("elyra-notification".into())
        .spawn(move || {
            match n.show() {
                Ok(handle) => {
                    let _ = handle.wait_for_response(|response: &notify_rust::NotificationResponse| {
                        if clicked(response) {
                            on_click();
                        }
                    });
                }
                Err(e) => {
                    crate::warn!(target: "elyra::notifications", "couldn't show a notification: {e}")
                }
            }
            waiting.fetch_sub(1, Ordering::SeqCst);
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(feature = "system")]
fn clicked(response: &notify_rust::NotificationResponse) -> bool {
    match response {
        notify_rust::NotificationResponse::Default => true,
        notify_rust::NotificationResponse::Action(key) => key == "default",
        _ => false,
    }
}

#[cfg(not(feature = "system"))]
fn system_show(_: Shown, _: Option<OnClick>, _: Arc<AtomicUsize>) -> Result<(), String> {
    Err("notifications in the OS need the `system` feature".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn a_fake_records_and_clicks() {
        let notifier = Notifier::fake();
        let clicked = Arc::new(AtomicBool::new(false));
        let flag = clicked.clone();
        notifier
            .show(
                Shown {
                    title: "Paid".into(),
                    body: None,
                },
                Some(Box::new(move || flag.store(true, Ordering::SeqCst))),
            )
            .unwrap();
        assert_eq!(notifier.assert_shown(|s| s.title == "Paid").body, None);
        assert!(notifier.click(0));
        assert!(clicked.load(Ordering::SeqCst));
        assert!(!notifier.click(0), "once");
        assert!(!notifier.click(1), "no such one");
    }

    #[test]
    #[should_panic(expected = "need a fake")]
    fn assertions_need_a_fake() {
        Notifier::system().assert_nothing_shown();
    }
}
