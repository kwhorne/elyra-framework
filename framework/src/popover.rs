//! The tray's popover (RFC 0019): where it opens, and when a click shows or
//! hides it. Both pure, so they're tested on every platform; the event loop
//! (`shell::webview`) does the rest.

use std::time::{Duration, Instant};

/// A rectangle in physical pixels: an icon, a monitor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub(crate) fn center(&self) -> (f64, f64) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
}

/// Between the icon and the popover.
const GAP: f64 = 4.0;
/// Between the popover and the monitor's edge.
const MARGIN: f64 = 8.0;

/// Where a popover of `width` × `height` opens for an icon at `icon` on
/// `monitor`: by the monitor's edge nearest the icon (below an icon in a
/// top bar, above one in a bottom taskbar, beside one in a side taskbar),
/// centered on the icon along that edge, and kept inside the monitor.
pub(crate) fn place(icon: Rect, width: f64, height: f64, monitor: Rect) -> (f64, f64) {
    let (cx, cy) = icon.center();
    let top = cy - monitor.y;
    let bottom = monitor.y + monitor.height - cy;
    let left = cx - monitor.x;
    let right = monitor.x + monitor.width - cx;
    let nearest = top.min(bottom).min(left).min(right);

    let (x, y) = if nearest == top {
        (cx - width / 2.0, icon.y + icon.height + GAP)
    } else if nearest == bottom {
        (cx - width / 2.0, icon.y - height - GAP)
    } else if nearest == left {
        (icon.x + icon.width + GAP, cy - height / 2.0)
    } else {
        (icon.x - width - GAP, cy - height / 2.0)
    };
    (
        keep_inside(x, width, monitor.x, monitor.width),
        keep_inside(y, height, monitor.y, monitor.height),
    )
}

/// `at` moved so `size` fits within `start..start + room`, `MARGIN` from
/// each end; at `start + MARGIN` when it can't fit.
fn keep_inside(at: f64, size: f64, start: f64, room: f64) -> f64 {
    let max = start + room - MARGIN - size;
    at.min(max).max(start + MARGIN)
}

/// A monitor as tao gives it: `bounds` in its "physical" pixels, and its
/// scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Screen {
    pub bounds: Rect,
    pub scale: f64,
}

/// Where the popover opens, in one space with its monitor's bounds: what
/// [`place`] needs. On Windows (and X11) tao's physical pixels are that
/// space already. On macOS they aren't: a monitor's position is its points
/// times its own scale, the cursor's its points times the main monitor's,
/// and `tray-icon`'s rect its points times the scale of the monitor the
/// icon is on. So there everything is taken back to points, and the
/// popover placed in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Space {
    Physical,
    Points,
}

impl Space {
    /// The platform's.
    pub(crate) fn here() -> Self {
        if cfg!(target_os = "macos") {
            Space::Points
        } else {
            Space::Physical
        }
    }

    /// A monitor's bounds in this space.
    pub(crate) fn screen(self, screen: Screen) -> Rect {
        match self {
            Space::Physical => screen.bounds,
            Space::Points => scaled(screen.bounds, screen.scale),
        }
    }

    /// The icon's rect in this space, and the monitor it's on: in points,
    /// the one where taking it back by that monitor's scale lands on it.
    pub(crate) fn icon(self, icon: Rect, screens: &[Screen]) -> Option<(Rect, Rect)> {
        screens.iter().find_map(|screen| {
            let icon = match self {
                Space::Physical => icon,
                Space::Points => scaled(icon, screen.scale),
            };
            let bounds = self.screen(*screen);
            contains(bounds, icon.center()).then_some((icon, bounds))
        })
    }

    /// The cursor in this space: on macOS, by the main monitor's scale.
    pub(crate) fn cursor(self, x: f64, y: f64, main_scale: f64) -> Rect {
        let (x, y) = match self {
            Space::Physical => (x, y),
            Space::Points => (x / main_scale, y / main_scale),
        };
        Rect {
            x,
            y,
            width: 0.0,
            height: 0.0,
        }
    }

    /// The monitor `point` is on, in this space.
    pub(crate) fn screen_at(self, point: (f64, f64), screens: &[Screen]) -> Option<Rect> {
        screens
            .iter()
            .map(|s| self.screen(*s))
            .find(|bounds| contains(*bounds, point))
    }
}

fn scaled(r: Rect, scale: f64) -> Rect {
    Rect {
        x: r.x / scale,
        y: r.y / scale,
        width: r.width / scale,
        height: r.height / scale,
    }
}

fn contains(r: Rect, (x, y): (f64, f64)) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

/// After a hide by blur, how long a click on the icon is the click that
/// caused it (RFC 0019, decision 6).
pub(crate) const REOPEN_AFTER: Duration = Duration::from_millis(300);

/// Whether the popover is shown, and when it last lost focus.
#[derive(Debug, Default)]
pub(crate) struct Popover {
    shown: bool,
    blurred: Option<Instant>,
}

impl Popover {
    pub(crate) fn is_shown(&self) -> bool {
        self.shown
    }

    /// A click on the icon: `true` to show it, `false` to hide it, `None`
    /// to leave it hidden. A click soon after a hide by blur is the click
    /// that took its focus: it hid it, and mustn't show it again.
    pub(crate) fn clicked(&mut self, now: Instant) -> Option<bool> {
        if self.shown {
            self.hidden();
            return Some(false);
        }
        if let Some(at) = self.blurred.take() {
            if now.saturating_duration_since(at) < REOPEN_AFTER {
                return None;
            }
        }
        self.shown = true;
        Some(true)
    }

    /// Shown by other means (code, the Linux menu). A blur before it is
    /// forgotten on the way out: hiding clears it, a blur replaces it.
    pub(crate) fn shown(&mut self) {
        self.shown = true;
    }

    /// Hidden by other means (Esc, code).
    pub(crate) fn hidden(&mut self) {
        self.shown = false;
        self.blurred = None;
    }

    /// It lost focus: `true` if it was shown, and so is to be hidden.
    pub(crate) fn blurred(&mut self, now: Instant) -> bool {
        if !self.shown {
            return false;
        }
        self.shown = false;
        self.blurred = Some(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    const SCREEN: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 1920.0,
        height: 1080.0,
    };

    #[test]
    fn below_an_icon_in_a_top_bar() {
        // macOS's menu bar: a 22 px icon at x 1500.
        let (x, y) = place(rect(1500.0, 0.0, 22.0, 22.0), 320.0, 420.0, SCREEN);
        assert_eq!((x, y), (1511.0 - 160.0, 22.0 + GAP));
    }

    #[test]
    fn above_an_icon_in_a_bottom_taskbar() {
        // Windows: a 40 px taskbar at the bottom.
        let (x, y) = place(rect(1700.0, 1040.0, 40.0, 40.0), 320.0, 420.0, SCREEN);
        assert_eq!((x, y), (1720.0 - 160.0, 1040.0 - 420.0 - GAP));
    }

    #[test]
    fn beside_an_icon_in_a_side_taskbar() {
        let left = place(rect(0.0, 500.0, 40.0, 40.0), 320.0, 420.0, SCREEN);
        assert_eq!(left, (40.0 + GAP, 520.0 - 210.0));
        let right = place(rect(1880.0, 500.0, 40.0, 40.0), 320.0, 420.0, SCREEN);
        assert_eq!(right, (1880.0 - 320.0 - GAP, 520.0 - 210.0));
        // Low on a side taskbar, it's pulled up to fit.
        let low = place(rect(0.0, 900.0, 40.0, 40.0), 320.0, 420.0, SCREEN);
        assert_eq!(low, (40.0 + GAP, 1080.0 - MARGIN - 420.0));
    }

    #[test]
    fn kept_inside_the_monitor_in_a_corner() {
        // Centered on an icon in the top right corner it would run off the
        // right edge; on one at the bottom left, off the left.
        let (x, _) = place(rect(1890.0, 0.0, 22.0, 22.0), 320.0, 420.0, SCREEN);
        assert_eq!(x, 1920.0 - MARGIN - 320.0);
        let (x, _) = place(rect(10.0, 1040.0, 40.0, 40.0), 320.0, 420.0, SCREEN);
        assert_eq!(x, MARGIN);
    }

    #[test]
    fn on_a_monitor_left_of_and_above_the_main_one() {
        // Negative coordinates: the monitor starts at (-1280, -1024).
        let monitor = rect(-1280.0, -1024.0, 1280.0, 1024.0);
        let (x, y) = place(rect(-200.0, -1024.0, 24.0, 24.0), 320.0, 420.0, monitor);
        assert_eq!((x, y), (-188.0 - 160.0, -1024.0 + 24.0 + GAP));
        // Near its right edge, kept on it, not pushed onto the main one.
        let (x, _) = place(rect(-30.0, -1024.0, 24.0, 24.0), 320.0, 420.0, monitor);
        assert_eq!(x, -MARGIN - 320.0);
    }

    #[test]
    fn a_popover_bigger_than_the_room_starts_at_the_margin() {
        let small = rect(0.0, 0.0, 300.0, 200.0);
        let (x, y) = place(rect(140.0, 0.0, 20.0, 20.0), 320.0, 420.0, small);
        assert_eq!((x, y), (MARGIN, MARGIN));
    }

    /// Three monitors as tao reports them on macOS: a Retina laptop
    /// (1800 × 1169 points, scale 2) in the middle, a 1920 × 1080 at scale 1
    /// to its left, and a 3008 × 1692 at scale 2 to its right.
    fn mac_screens() -> [Screen; 3] {
        [
            Screen {
                bounds: rect(0.0, 0.0, 3600.0, 2338.0),
                scale: 2.0,
            },
            Screen {
                bounds: rect(-1920.0, 0.0, 1920.0, 1080.0),
                scale: 1.0,
            },
            Screen {
                bounds: rect(3600.0, 0.0, 6016.0, 3384.0),
                scale: 2.0,
            },
        ]
    }

    #[test]
    fn on_macos_the_icon_is_found_in_points_on_its_own_monitor() {
        let screens = mac_screens();
        // An icon at 1500 points in the laptop's menu bar: tray-icon says
        // 3000 (× 2).
        let (icon, bounds) = Space::Points
            .icon(rect(3000.0, 0.0, 44.0, 48.0), &screens)
            .unwrap();
        assert_eq!(icon, rect(1500.0, 0.0, 22.0, 24.0));
        assert_eq!(bounds, rect(0.0, 0.0, 1800.0, 1169.0));
        // At 2500 points on the right monitor: 5000. Taken as physical,
        // it would be on none.
        let (icon, bounds) = Space::Points
            .icon(rect(5000.0, 0.0, 44.0, 48.0), &screens)
            .unwrap();
        assert_eq!(icon.x, 2500.0);
        assert_eq!(bounds, rect(1800.0, 0.0, 3008.0, 1692.0));
        // On the left one, at scale 1.
        let (icon, bounds) = Space::Points
            .icon(rect(-400.0, 0.0, 22.0, 24.0), &screens)
            .unwrap();
        assert_eq!(icon.x, -400.0);
        assert_eq!(bounds, rect(-1920.0, 0.0, 1920.0, 1080.0));
    }

    #[test]
    fn on_macos_the_cursor_is_taken_back_by_the_main_monitors_scale() {
        // tao's cursor at (2530, 1578): 1265 × 789 points, on the laptop,
        // not the monitor that starts at 3600 "physical".
        let screens = mac_screens();
        let cursor = Space::Points.cursor(2530.0, 1578.0, 2.0);
        assert_eq!((cursor.x, cursor.y), (1265.0, 789.0));
        assert_eq!(
            Space::Points.screen_at(cursor.center(), &screens),
            Some(rect(0.0, 0.0, 1800.0, 1169.0))
        );
        // On the border, the monitor that starts there.
        assert_eq!(
            Space::Points.screen_at((1800.0, 10.0), &screens),
            Some(rect(1800.0, 0.0, 3008.0, 1692.0))
        );
        assert_eq!(
            Space::Points.screen_at((1799.5, 10.0), &screens),
            Some(rect(0.0, 0.0, 1800.0, 1169.0))
        );
    }

    #[test]
    fn elsewhere_physical_pixels_are_one_space() {
        let screens = [
            Screen {
                bounds: rect(0.0, 0.0, 3840.0, 2160.0),
                scale: 1.5,
            },
            Screen {
                bounds: rect(3840.0, 0.0, 1920.0, 1080.0),
                scale: 1.0,
            },
        ];
        let icon = rect(5600.0, 1040.0, 40.0, 40.0);
        assert_eq!(
            Space::Physical.icon(icon, &screens),
            Some((icon, rect(3840.0, 0.0, 1920.0, 1080.0)))
        );
        assert_eq!(Space::Physical.cursor(10.0, 20.0, 1.5).x, 10.0);
        assert_eq!(
            Space::Physical.screen_at((10.0, 20.0), &screens),
            Some(screens[0].bounds)
        );
        // Off every monitor: none.
        assert_eq!(Space::Physical.screen_at((-5.0, 0.0), &screens), None);
        assert_eq!(
            Space::Physical.icon(rect(-50.0, 0.0, 20.0, 20.0), &screens),
            None
        );
    }

    #[test]
    fn a_click_shows_it_and_another_hides_it() {
        let now = Instant::now();
        let mut p = Popover::default();
        assert_eq!(p.clicked(now), Some(true));
        assert!(p.is_shown());
        assert_eq!(p.clicked(now), Some(false));
        assert!(!p.is_shown());
    }

    #[test]
    fn the_click_that_took_its_focus_does_not_show_it_again() {
        let now = Instant::now();
        let mut p = Popover::default();
        p.clicked(now);
        // The click on the icon takes the popover's focus first.
        assert!(p.blurred(now));
        assert_eq!(p.clicked(now + Duration::from_millis(50)), None);
        assert!(!p.is_shown());
        // The next click is the user's.
        assert_eq!(p.clicked(now + Duration::from_millis(60)), Some(true));
    }

    #[test]
    fn a_click_well_after_a_blur_shows_it() {
        let now = Instant::now();
        let mut p = Popover::default();
        p.clicked(now);
        p.blurred(now);
        assert_eq!(p.clicked(now + REOPEN_AFTER), Some(true));
    }

    #[test]
    fn a_blur_while_hidden_changes_nothing() {
        let now = Instant::now();
        let mut p = Popover::default();
        assert!(!p.blurred(now));
        assert_eq!(p.clicked(now), Some(true));
    }

    #[test]
    fn shown_and_hidden_by_other_means_forget_the_blur() {
        let now = Instant::now();
        let mut p = Popover::default();
        p.clicked(now);
        p.blurred(now);
        p.shown();
        assert!(p.is_shown());
        assert_eq!(p.clicked(now), Some(false));
        p.clicked(now + REOPEN_AFTER);
        p.blurred(now + REOPEN_AFTER);
        p.hidden();
        assert_eq!(p.clicked(now + REOPEN_AFTER), Some(true));
    }
}
