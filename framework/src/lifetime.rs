//! How long the app runs, and whether it's in the Dock: what closing a
//! window does, when the app ends, and (on macOS) whether it's a regular app
//! or an accessory. Pure, so it's tested on every platform; the event loop
//! asks it ([RFC 0020](https://github.com/kwhorne/elyra-framework/blob/main/docs/proposals/0020-stay-in-tray.md)).

/// Which kind of window a close is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// The app's first window.
    Main,
    /// The tray's popover (RFC 0019).
    Popover,
    /// Any other window.
    Other,
}

/// What closing a window does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Close {
    /// It's hidden, its page kept loaded.
    Hide,
    /// It's dropped.
    Drop,
}

/// Closing `kind`: the popover always hides; in an app that stays in the
/// tray, so does the main window; anything else is dropped.
pub(crate) fn close(kind: Kind, stay_in_tray: bool) -> Close {
    match kind {
        Kind::Popover => Close::Hide,
        Kind::Main if stay_in_tray => Close::Hide,
        Kind::Main | Kind::Other => Close::Drop,
    }
}

/// Whether the app ends, with `left` windows left besides the popover. An
/// app that stays in the tray ends only on a quit.
pub(crate) fn ends(stay_in_tray: bool, left: usize) -> bool {
    !stay_in_tray && left == 0
}

/// Whether the app is in the Dock (a regular app, not an accessory), with
/// `shown` windows shown besides the popover: always, unless it stays in the
/// tray; then only while one is shown.
pub(crate) fn in_dock(stay_in_tray: bool, shown: usize) -> bool {
    !stay_in_tray || shown > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_popover_always_hides() {
        assert_eq!(close(Kind::Popover, false), Close::Hide);
        assert_eq!(close(Kind::Popover, true), Close::Hide);
    }

    #[test]
    fn the_main_window_hides_only_in_an_app_that_stays_in_the_tray() {
        assert_eq!(close(Kind::Main, false), Close::Drop);
        assert_eq!(close(Kind::Main, true), Close::Hide);
    }

    #[test]
    fn other_windows_close() {
        assert_eq!(close(Kind::Other, false), Close::Drop);
        assert_eq!(close(Kind::Other, true), Close::Drop);
    }

    #[test]
    fn an_app_ends_with_its_last_window_unless_it_stays_in_the_tray() {
        assert!(ends(false, 0));
        assert!(!ends(false, 1));
        assert!(!ends(true, 0));
        assert!(!ends(true, 2));
    }

    #[test]
    fn it_is_in_the_dock_while_a_window_shows() {
        assert!(in_dock(false, 0));
        assert!(in_dock(false, 1));
        assert!(!in_dock(true, 0));
        assert!(in_dock(true, 1));
    }
}
