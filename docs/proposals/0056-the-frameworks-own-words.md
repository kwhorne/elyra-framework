# RFC 0056 — The framework's own words, in the app's language

**Status:** draft (2026-11-26) · **Target:** 0.58.0 · builds on [translations](../i18n.md) · see [Open questions](#open-questions)

## Summary

An app translated to Norwegian with `lang/nb.json` still shows the
framework's own words in English. The date picker says "Choose dates",
"Clear" and "Pick the last day"; the outbox drawer says "Keep mine" and
"Take theirs"; the About dialog says "Version" and "Close"; the menu bar
says "Edit" and "Help". The only way round it is to pass the words to
every instance, and each component asks for them differently: a `labels`
object on some, `previousLabel` on others, and nothing at all on the date
pickers, `Select`, `Combobox`, `Modal` or the runtime's panels.

This RFC puts every word the framework shows through the app's
translations, under keys of its own (`elyra.*`), and ships them in English
and Norwegian:

```json
// lang/nb.json — nothing to add: elyra's Norwegian comes with it
{ "welcome": "Hei, :name!" }
```

```json
// lang/de.json — a language elyra doesn't ship: the app adds the words
{
  "welcome": "Hallo, :name!",
  "elyra.datepicker.clear": "Leeren",
  "elyra.daterange.choose": "Zeitraum wählen"
}
```

```svelte
<!-- as before, for one instance: a prop still wins -->
<DateRangePicker bind:start bind:end labels={{ choose: "Choose a period" }} />
```

- **One set of keys** for `@elyra/ui`, the runtime's panels (the About
  dialog, the command palette, the assistant, the update toast, `confirm`
  and `prompt`) and the menus Rust builds (Edit, Help, About, the system's
  items).
- **English and Norwegian (`nb`) ship with the framework.** Any other
  language is the app's to add, key by key, and `rata check` lists what's
  missing.
- **The app's languages decide.** The framework's words follow the app's
  locale and fallback, so an English-only app on a Norwegian Mac stays in
  English.
- **A `labels` prop on every component with words**, overriding the
  catalog per instance, as `Table` and `AccountSwitcher` have today.
- **Dates follow the app's locale**: `DatePicker` and `DateRangePicker`
  take the month names, the week's first day and the typed format from
  `$locale` when no `locale` is passed.

## Motivation

- **Dogfood.** kver is Norwegian. Once it takes `@elyra/ui`'s pickers and
  drawers, it either passes Norwegian words to each instance or lives with
  English ones.
- **Words that can't be reached.** `DateRangePicker`'s presets, `Select`'s
  "No matches", `Modal`'s close button, the About dialog and the menu bar
  have no prop at all, so an app can't translate them even by hand.
- **Screen readers read the aria-labels.** "Previous month" and "Clear"
  are spoken, in English, in the middle of a Norwegian page.
- **Three ways to pass words** (`labels`, `previousLabel`, `label`) for one
  need is a thing to learn per component.

## Current state (0.57)

- `Translator` (`framework/src/i18n.rs`) holds the app's catalogs; the
  runtime gets the current locale's messages, fallback first, from
  `/__i18n` (`resolved()`). `translate` returns the key when it's missing.
- `@elyra/ui`: `labels` on `AccountSwitcher`, `Microphone`,
  `NotificationBell`, `Presence`, `Table` and `TreeView`; single props on
  `Pagination` (`previousLabel`, `nextLabel`, `pageLabel`), `Spinner`,
  `Sidebar`, `SplitPane`, `AvatarGroup`, `Breadcrumbs`. Fixed English in
  `DatePicker`, `DateRangePicker` (and `internal/ranges.js`), `Select`,
  `Combobox`, `FileField`, `Modal`, `Callout`, `OutboxDrawer`,
  `EditingDrawer`, `StatusBar`, `Sparkline`.
- The runtime: fixed English in the About dialog, `confirm`/`alert`/
  `prompt` (bar `confirmLabel`/`cancelLabel`), the command palette, the
  assistant panel and the update toast.
- Rust: "Edit", "Help" and "About <App>" in `shell/webview.rs`; RFC 0055's
  system items take muda's English.
- The developer's tools (Inspector, Theme Studio, Point at it, the state
  matrix) are English, in debug builds only.

## Design

### The keys

`elyra.<component>.<word>`, one key per word in its place, so a
translator sees what it's for and an app can change one without the
others:

```json
{
  "elyra.datepicker.choose": "Choose date",
  "elyra.datepicker.clear": "Clear",
  "elyra.datepicker.previous": "Previous month",
  "elyra.daterange.preset.last7": "Last 7 days",
  "elyra.outbox.keepMine": "Keep mine",
  "elyra.accounts.pending": "{1} A change made on :host hasn't been sent yet. Sign out and discard it?|[2,*] :count changes made on :host haven't been sent yet. Sign out and discard them?",
  "elyra.about.version": "Version :version",
  "elyra.menu.edit": "Edit"
}
```

Messages that today are functions (`AccountSwitcher`'s `pending(host,
n)`) become messages with placeholders and plural segments, as the app's
own are.

### Where they live

`framework/lang/en.json` and `framework/lang/nb.json`, embedded in the
crate. `Translator` keeps them as a layer under the app's catalogs: a
lookup tries, for each locale in the chain (`nb-NO`, `nb`, the fallback),
the app's catalog and then the framework's. So the app's `nb` beats the
framework's `nb`, and the framework's `nb` beats the app's `en`.

The framework's catalog is only used in **locales the app has a catalog
for**. An app with only `lang/en.json`, on a Norwegian Mac, resolves to
`en` and shows English throughout. Without an `I18nProvider` at all,
nothing changes: the words are English, as today.

`resolved()` (and so `/__i18n`) includes the `elyra.*` keys, so the page
gets them in the catalog it already loads.

### In `@elyra/ui`

Each component reads its words through one internal helper:

```js
// ui/src/internal/words.js
const words = useWords("datepicker", WORDS, () => labels);
words.clear; // labels.clear ?? catalog "elyra.datepicker.clear" ?? WORDS.clear
```

- `WORDS` is the component's English, kept in the component so it works
  with no catalog (the docs site, tests, an app without i18n). A test pins
  it to `framework/lang/en.json`.
- It re-reads when the catalog loads or the locale changes.
- `labels` is added to every component with words. The single props
  (`previousLabel`, `label` on `Spinner`, …) stay and win over both.

### In the runtime

The same lookup for the panels it draws, through a `word(key, english)`
that falls back to the English instead of returning the key. `confirm`'s
`confirmLabel` and `cancelLabel` stay. The developer's tools stay English.

### In Rust

`shell/webview.rs` labels Edit, Help, About and RFC 0055's system items
through `Translator` when there is one, and relabels them when the locale
changes, with RFC 0055's `set`, so no window's menu bar is rebuilt.

### `rata check`

For each locale the app has a catalog for, the `elyra.*` keys that neither
the app nor the framework translates, as one warning per locale:

```
warning: lang/de.json: 41 of elyra's words fall back to English
         (elyra.datepicker.clear, elyra.daterange.choose, … — `rata check --verbose` lists them)
```

### Typed keys

`rata codegen` types the `elyra.*` keys too, so an app's override in
`$t("elyra.outbox.keepMine")` or `labels` is checked like its own.

### Testing

- `en.json` and `nb.json` have the same keys, and each message the same
  placeholders.
- Every `WORDS` in `@elyra/ui` and every `word()` in the runtime matches
  `en.json`, and no component shows a literal word outside them (as
  `tokens.test.js` does for durations).
- The layering: the app's `nb` over the framework's `nb` over the app's
  `en`; an English-only app stays English in `nb-NO`.
- A browser check: the date range picker in `nb`, its presets and
  aria-labels included.

## Implementation plan

1. `framework/lang/{en,nb}.json`; the layer in `Translator`; `resolved()`.
2. `useWords` in `@elyra/ui`; every component with words through it, and
   `labels` where it's missing; dates from `$locale`.
3. `word()` in the runtime's panels.
4. The menus in Rust; relabelling on a locale change.
5. `rata check` and `rata codegen`; docs (`i18n.md`, each component's API
   table), CHANGELOG.

## Open questions

1. **Which languages ship?**
   *Recommendation:* English and Norwegian Bokmål. Each one needs someone
   who speaks it to check it; Nynorsk, Swedish, Danish or German come as
   contributions, or from an app.
2. **Keys per component, or shared words** (`elyra.clear` for every
   Clear)?
   *Recommendation:* per component. One word can need two translations in
   two places, and an app that changes one shouldn't change the others.
3. **Should the framework's words follow the OS language when the app has
   no catalog for it?**
   *Recommendation:* no. The app's languages decide, so a page is never in
   two languages.
4. **Do the developer's tools (Inspector, Theme Studio, Point at it) get
   translated?**
   *Recommendation:* no. Only the developer sees them, in debug builds,
   and their words name the framework's own concepts.
5. **Should the date pickers take `$locale` when no `locale` is passed?**
   This changes what an app on a Norwegian Mac with English strings sees:
   the webview's locale today, the app's after.
   *Recommendation:* yes. It's the same rule as question 3, and the
   CHANGELOG says so under Upgrading.

## Not in this RFC

- **Right-to-left layouts.**
- **Number and currency formats** beyond what `Intl` does with the locale
  (`Stat`, `Progress` already take `format`).
- **Translating the app's own menu**: it's built in Rust, so its labels
  can already come from `Translator::get`.

## Alternatives considered

- **Each package ships its own catalog** (`@elyra/ui/lang/nb.json`,
  registered in the page): no Rust change, but the menus and `rata check`
  need the words too, and three catalogs could drift apart where one
  can't.
- **Only `labels` everywhere, no catalog**: each app writes the same
  Norwegian for the same date picker, and the menus and panels still have
  nothing to take it.
- **`Intl.DisplayNames` and the browser's own words**: they cover month
  and weekday names, which the pickers already take from `Intl`, but not
  "Keep mine" or "Pick the last day".
