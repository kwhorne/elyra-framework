# RFC 0020 — An app that stays in the tray

**Status:** implemented (2026-11-03) · **Target:** 0.33.0 · builds on [RFC 0019](0019-tray-popover.md) · see [Decisions](#decisions)

## Summary

An app whose home is the tray icon: it starts without a window, keeps
running when its windows close, and leaves the Dock (macOS) to the apps
the user is working in. A timer, a sync client, an inbox that's watched
all day. The main window is still there, one click away:

```rust
App::new()
    .title("Timer")
    .stay_in_tray()
    .tray(
        TrayConfig::new()
            .tooltip("Timer")
            .popover(WindowConfig::new("quick").path("#/quick").size(320.0, 420.0))
            .item("open", "Open Timer")
            .quit("Quit Timer"),
    )
    .run();
```

```svelte
<!-- the main window's page: hidden, but loaded -->
<script>
  import { appWindow, channel } from "@elyra/runtime";
  const tray = channel("tray");
  $effect(() => { if ($tray === "open") appWindow.show(); });
</script>
```

It starts with the icon only. "Open Timer" shows the main window, and
closing it hides it again; the app runs on until "Quit Timer", ⌘Q, or
`quit()`.

## Motivation

- **Some apps are meant to be there all day.** A timer, a sync status,
  an inbox to glance at: closing the window shouldn't stop them, and a
  Dock icon for an app that's mostly an icon in the menu bar is in the
  way.
- **RFC 0019 made the look; this is the lifetime.** With the popover, a
  menu-bar app is a page under the icon. What's missing is the rest:
  starting without a window, not ending with the last one, the Dock, and
  how the user gets the main window back and quits.
- **Every app gets it slightly wrong.** A hidden window to keep the app
  alive, the Dock icon that stays or doesn't, the window that comes back
  behind the app in front, the app that can't be quit once its window
  is closed: the same small mistakes, each time.

## Current state (0.32)

- **Lifetime** (`shell/webview.rs`): the app exits when its last window
  closes; the popover doesn't count (`only_popover_left`). There is no
  quit except closing the last window, the tray's Quit item, and ⌘Q on
  macOS (the app menu's predefined Quit).
- **The main window** (`App::new()`) is always made at start: a primary
  window, `windows[0]`. `WindowConfig::visible(false)` (0.32) makes any
  window hidden at start; a hidden primary keeps the app alive, but
  closing it still ends the app, and the Dock icon stays.
- **A second launch** (`single_instance()`) shows and focuses the focused
  or primary window, and forwards its command line.
- **macOS:** tao 0.35 has the activation policy (`set_activation_policy`,
  and `set_activation_policy_at_runtime`), `set_dock_visibility`, and
  `Event::Reopen` (the app opened again from Finder, Spotlight or the
  Dock while it runs). The shell doesn't handle `Reopen`. The app menu
  (with Edit, so ⌘C/⌘V reach the webview) is set at start.
- **Linux:** the tray is an AppIndicator. GNOME shows none without an
  extension, and `tray-icon` can't tell: the icon is made, and nobody
  sees it.
- **`rata bundle`** writes the `.app`'s `Info.plist` from `elyra.toml`;
  it doesn't know the Rust builder.

## Design

### `App::stay_in_tray()`

One switch on the app, with a tray required:

- **It starts with the icon only.** The main window is made, hidden
  (`visible(false)`), so showing it is at once, as for the popover. The
  app's other startup windows (`App::window`) are made hidden too.
- **Closing a window doesn't end the app.** Closing the main window
  hides it: its page stays loaded and its live queries keep running
  (open question 2). Other windows close as now.
- **It ends on a quit:** the tray's Quit item, ⌘Q, `quit()` (below), the
  OS logging out, or an update's relaunch.
- **It's a single instance** (open question 4): two copies would be two
  icons, and a second launch is how the window comes back where there's
  no icon to click (below).

Without `.tray(..)`, an app that stays in the tray would be unreachable:
`run()` stops at once with an error saying so.

### Bringing the main window back

- **From the tray:** the app's own item, as above. The hidden main
  window's page is loaded and hears `"tray"`, and shows itself
  (`appWindow.show()`); from Rust, `Windows::show(Some("main"))`. In an
  app that stays in the tray, showing a window also focuses it and
  brings the app forward (on macOS, `activateIgnoringOtherApps`), so it
  comes in front of the app in front, not behind it.
- **Opening the app again:** a second launch (`single_instance`) shows
  and focuses the main window, as now. On macOS, opening a running
  bundle from Finder, Spotlight or Launchpad starts no second process;
  the OS sends `Reopen`, and that shows the main window too.
- **From a deep link** nothing changes: `elyra:deep-link` goes to the
  pages, and the app decides whether a window shows.

### The Dock on macOS

- **No Dock icon while no window is shown:** the app starts as an
  accessory (`ActivationPolicy::Accessory`), so it isn't in the Dock or
  in ⌘Tab, and has no menu bar.
- **A Dock icon while a window is shown** (open question 1): showing the
  main window, or any ordinary window, makes the app regular, with its
  Dock icon, ⌘Tab and its menu bar; when the last of them is hidden or
  closed it goes back to an accessory. The popover never counts: it's
  over every other app, not one of them.
- **The `.app`:** an accessory app set in code can show the Dock icon
  for a moment at launch, before the policy is set. If it does,
  `rata bundle` adds `LSUIElement` to `Info.plist` from `elyra.toml`
  (`[bundle] stay_in_tray = true`); `rata doctor` warns when the two
  disagree. Checked in the implementation, not assumed.

### On Windows and Linux

- **No taskbar button while no window is shown,** as for any hidden
  window. Shown, the main window has its taskbar button as now.
- **On Linux, if the tray can't be made** (`tray-icon` returns an error),
  the app falls back to an ordinary one: the main window shows, and the
  app ends when it closes, with a warning in the log.
- **GNOME without an AppIndicator extension** makes the icon and shows
  nothing; it can't be detected without asking D-Bus for a
  StatusNotifierWatcher (open question 5). The way back there is opening
  the app again from the launcher, which shows the main window. The
  guide says so.

### Quitting

- **`quit()`** in the runtime and `Windows::quit()` in Rust end the app,
  as the tray's Quit item does: a Quit button in the main window or the
  popover. It's window control, under `Capability::Window`; a window
  given less (RFC 0016) without it can't quit.
- **⌘Q** on macOS quits from any of the app's windows, the popover
  included, as now; an accessory app has no menu bar shown, but keeps
  its menu's key equivalents (to check, see Testing).
- **Ending is the same however it's asked:** the main window's geometry
  is saved (with `persist_window_state`), then the loop exits, as when
  the last window closes today.

### In the page

- **`elyra:window`** already says `visible`; a hidden main window gets
  it on show and hide, so a page knows when it's looked at.
- **`appWindow.close()`** on the main window hides it, as the close
  button does. The page doesn't need to know which kind of app it's in.

### Security

- **Nothing new is reached.** `quit()` is under the window capability
  every window has today unless it was given less; quitting is what
  closing the last window already did.
- **A running app the user can't see.** The icon is the sign it's
  running, which is why a tray is required and the Linux fallback shows
  the window. A background app that hides even that is out of scope.

### What it doesn't do

- **Hiding the tray icon too:** an app with no visible sign it runs.
- **Starting at login:** that's `autostart` (unchanged); an app that
  stays in the tray starts there without a window either way.
- **A window that starts shown, the first time:** onboarding is the
  app's: it shows the main window from code on its first run.
- **Unloading the hidden main window** to free its memory. It costs a
  webview for the run, as the popover does (open question 2).

### Testing

- **Lifetime as a pure function,** like `only_popover_left`: what a
  close does (hide or close, end the app or not) for the main window,
  another window and the popover, staying in the tray or not.
- **The Dock as a pure function:** regular or accessory for a set of
  shown windows, the popover not counting.
- **The config:** `stay_in_tray()` makes the startup windows hidden,
  turns on single instance; `run()` without a tray errors.
- **The runtime:** `quit()` calls the window endpoint; denied without
  the window capability.
- **`rata bundle`:** `LSUIElement` in `Info.plist` when asked, not
  otherwise; `rata doctor`'s warning.
- **On the Mac,** launch-smoked from the `.app`: no Dock icon at start,
  one while the main window shows, gone when it's closed; the window
  comes in front of another app; ⌘V in the popover and the main window
  while an accessory; `Reopen` from Finder shows the window; Quit ends
  it. Windows and Linux compiled in CI, called out as unverified.

## Implementation plan

1. `App::stay_in_tray()`, the lifetime and the Dock as pure functions,
   with their tests; `run()`'s check for a tray.
2. The event loop: startup windows hidden, closing the main window
   hides it, the activation policy as windows show and hide, `Reopen`,
   the Linux fallback; `Windows::quit()`.
3. The runtime: `quit()`.
4. `rata bundle`'s `LSUIElement`, if the launch flash needs it; `rata
   doctor`.
5. The example app stays in the tray behind a flag; docs: the tray
   guide, windows (lifecycle), single instance, the changelog.

## As built

- **`App::stay_in_tray()`** as designed: the startup windows made hidden,
  single instance turned on, and `prepare` stops with a panic without a
  tray (as it does for an invalid assistant). What a close does, when the
  app ends, and whether it's in the Dock are pure functions
  (`lifetime.rs`), tested; the event loop asks them. `Windows::quit()`
  and `quit()` in the runtime (`/__window/quit`, under the window
  capability) end the app as the tray's Quit item does, the main window's
  geometry saved first.
- **The Dock icon did flash.** A bundled `.app` with the policy set in code
  was a regular app for about 0.4 s at launch. So `rata bundle` writes
  `LSUIElement`; launched, it's an accessory from the first sample. Not
  from `elyra.toml` as sketched, though: `rata bundle` asks the release
  binary (`ELYRA_BUNDLE_INSPECT`, as `rata mcp inspect` asks the app), so
  the Rust builder stays the one place it's said and nothing can
  disagree. There is no `rata doctor` to warn; the RFC was wrong to name
  one. An app that doesn't answer in 15 s (an older Elyra opens its
  window instead) is stopped and bundled as an ordinary app.
- **Launching doesn't take focus:** tao activates an app at launch, which
  would take it from the app in front at login; an app that stays in the
  tray is launched without (`set_activate_ignoring_other_apps(false)`).
- **`Reopen` needed the Dock.** Opening the running `.app` again, macOS
  activates it before it sends `Reopen`: it was active as an accessory,
  became regular, and the menu bar stayed with the app before it (its
  window was in front, but System Events said Mail was frontmost).
  Deactivating first, or showing it later, didn't help. Handing the
  activation to the Dock and showing the window again 100 ms later does:
  frontmost three times out of three.
- **Checked on the Mac,** from the example app (`ELYRA_STAY_IN_TRAY=1`),
  driven from a file, with Mail in front: no Dock icon and nothing shown
  at start, focus left with the app in front; the tray's "Open" (its page
  hearing `"tray"` and calling `appWindow.show()`) shows the main window
  in front of Mail, a regular app and frontmost; closing it hides it, back
  to an accessory with Mail frontmost again; a second launch shows it, one
  process; with another window open, closing the main one keeps the app
  in the Dock until that one closes too; `quit` ends it. An ordinary app
  is unchanged: in the Dock, and it ends when its window closes.
- **Not seen:** ⌘C/⌘V while an accessory (posting keys needs
  accessibility access the test didn't have; the app menu, with Edit, is
  set as before), a real click on the tray icon (as for RFC 0019, the menu
  bar was full), and the Linux fallback and Windows, compiled in CI only.

## Alternatives considered

- **Leave it to apps:** `.visible(false)` on the main window and a
  `stay_open` flag. That keeps the app alive, but not the rest: the Dock,
  the window coming back in front, `Reopen`, the GNOME way back, Quit.
- **`App::menu_bar_app()`** as the name: macOS's word, where Windows
  calls it the notification area and Linux the tray. The framework says
  tray everywhere else.
- **A Dock policy setting on its own** (`App::dock(Dock::Hidden)`):
  more general, but a Dock-less app that ends with its last window is a
  trap, and every app that wants one wants the rest of this too.
- **Closing the main window for real,** and making it again on show:
  less memory while hidden, but a moment's blank window on each show,
  lost state in the page, and `appWindow.show()` with no page to run it.

## Open questions

1. **The Dock icon while a window is shown:** regular while an ordinary
   window is shown, accessory otherwise (recommended: the app is in ⌘Tab
   and has its menu bar while the user works in it, like most tray apps
   on the Mac), or never in the Dock?
2. **Closing the main window:** hides it, page and live queries kept
   (recommended: it's back at once and as it was, at a webview's memory
   for the run), or closes it, made again on show?
3. **The name:** `stay_in_tray()` (recommended: says what changes when
   the window closes, and "tray" is the framework's word),
   `menu_bar_app()`, or `background()`?
4. **Single instance:** turned on by `stay_in_tray()` (recommended: two
   copies are two icons, and a second launch is the way back on GNOME),
   or left to the app?
5. **GNOME without a tray host:** documented, with a second launch as the
   way back (recommended), or detected over D-Bus, falling back to an
   ordinary app?
6. **`LSUIElement`:** only if the Dock icon flashes at launch with the
   policy set in code (recommended: one place to say it while it's
   enough), or always in the `.app`?

## Decisions

Settled on 2026-11-03, all as recommended:

1. **In the Dock while a window is shown:** regular while an ordinary
   window is shown, accessory otherwise; the popover doesn't count.
2. **Closing the main window hides it,** its page and live queries kept.
3. **The name is `stay_in_tray()`.**
4. **`stay_in_tray()` turns on single instance.**
5. **GNOME without a tray host is documented,** with a second launch as
   the way back; not detected over D-Bus.
6. **`LSUIElement` only if the Dock icon flashes at launch** with the
   policy set in code.
