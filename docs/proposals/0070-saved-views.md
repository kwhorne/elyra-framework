# RFC 0070 — Saved views: a list's questions, kept by name

**Status:** accepted (2026-11-28) · **Target:** 0.64.0 · builds on [RFC 0068](0068-columns-the-user-picks.md) and [RFC 0069](0069-filters.md) · see [Decisions](#decisions)

## Summary

With filters (RFC 0069) and columns the user picks (RFC 0068), a list
answers questions: "the open invoices over 10,000, due this month, with
the org. number shown, the largest first". A user asks the same few every
day, and sets the funnels, the sort and the columns again each time,
because a list opens unfiltered (RFC 0069's decision 4). Every desktop
list worth the name keeps them: Outlook's views, Finder's smart folders,
a spreadsheet's filter views.

This RFC keeps a list's question under a name:

```svelte
<Table
  page={invoices}
  bind:query
  persist="invoices"
  choosable
  views={[
    { name: "Overdue", filters: { status: ["overdue"] }, sort: "due" },
    { name: "Large", filters: { amount: { min: 10000 } }, sort: "amount", direction: "desc" },
  ]}
  bind:view
  columns={columns}
/>
```

- **A *Views* menu** over the table, beside *Columns*: *All*, the app's
  views, the user's own, and *Save view…*. Picking one sets the filters,
  the search, the sort and the columns at once.
- **The app's views** (`views`) are given; **the user's** are saved by
  name from what the list shows now, kept with `persist` in the settings
  store, and renamed or deleted from the menu.
- **A changed view says so**: "Overdue (changed)", with *Save* (over the
  user's own) and *Save as new view…*.
- **`bind:view`** is the view's name, so an app can put it in the route
  (`/invoices?view=Overdue`), and a link, the ⌘K palette or the assistant
  can open a list in a view.

## Motivation

- **The questions repeat.** Setting three funnels, a sort and two columns
  every morning is the work a view saves.
- **The app knows some of them.** "Overdue" is the accounting app's own
  question; it should come with it, not be built by each user.
- **A view is a place.** A link to "the overdue invoices" is how a user
  sends a colleague, a notification or the assistant to them.
- **Filters reset by design** (RFC 0069), which is right for a list that
  opens, and wrong for a question asked daily: a view is the way to keep
  one on purpose.

## Current state (0.63, with RFCs 0068 and 0069)

- `query` holds `search`, `filters`, `sort`, `direction` and `page`;
  `bind:query` has them.
- `persist` keeps `{ widths, hidden, order }` (RFC 0068) under
  `elyra-ui.table.<persist>`; filters aren't kept (RFC 0069).
- The table has a *Columns* `Dropdown` over it with `choosable`.

## Design

### A view

```js
{ name: "Overdue", filters: { status: ["overdue"] }, search: "", sort: "due", direction: "asc", columns: { hidden: ["phone"], order: [...] } }
```

Every part but `name` is optional: a view that doesn't say a sort leaves
the sort as the app's default, one without `columns` leaves the user's
columns as they are. The widths aren't in it: they're how the list looks,
not what it asks.

### The menu

With `views` (the app's, or `[]` for only the user's) and `persist`, a
*Views* `Dropdown` over the table, its button the current view's name
("All" without one):

- *All*: no filters, no search, the app's sort and the user's columns.
- The app's views, then the user's, each a `menuitemradio`, the current
  one checked.
- *Save view…*: a small dialog for the name; it saves what the list shows
  now (`query` less the page, and the columns with `choosable`).
- On a user's view, *Rename…* and *Delete*; the app's can't be changed.

When the list no longer matches its view (a funnel, the search, a sort, a
column changed), the button says "Overdue (changed)" and the menu has
*Save* (a user's view) and *Save as new view…*; picking the view again
goes back to it.

### Kept and linked

The user's views are kept under `elyra-ui.table.<persist>.views` in the
settings store, on this machine, for this user. `bind:view` is the
current view's name (or `null` for *All*): set it to open the list in a
view. A name that's neither the app's nor the user's is *All*.

### What it says

The table's `aria-description` names its view ("View: Overdue"), and the
assistant's note says it with the list's filters (RFC 0013's window note,
from the table's own summary).

### Words

`elyra.table.views` ("Views"), `allRows` ("All"), `saveView` ("Save
view…"), `saveViewAgain` ("Save"), `saveAsNew` ("Save as new view…"),
`renameView`, `deleteView`, `viewName` ("Name"), `changed` (":view
(changed)"), `viewIs` ("View: :view"), in English and Norwegian.

### Testing

- `internal/views.js`: a view applied to a query and a columns choice;
  whether the list matches its view; a saved view from the list.
- The menu: pick, save, rename, delete; the app's can't be changed; the
  changed state and *Save* / *Save as new*.
- `persist`: the user's views kept and read back; a view naming a column
  the app took away.
- `bind:view` both ways; an unknown name is *All*.
- A browser check in the docs: save a view, change it, go back.

## Implementation plan

1. `internal/views.js`, held by tests without a DOM.
2. `Table`'s *Views* menu, the save dialog, `bind:view`, the words.
3. `make:resource --view`: the list's `view` in its route's query.
4. Docs (`docs/ui/table.md`, an example), CHANGELOG.

## Open questions

1. **The user's views on this machine, or synced through the backend?**
   *Recommendation:* this machine, in the settings store, as the columns
   are. A backend that syncs them is the app's (or a later RFC's), and the
   Laravel side has no table for them today.
2. **The widths in a view?**
   *Recommendation:* no. A view is a question; the widths are the list's
   look, the same in every view.
3. **Open the list in the last view used?**
   *Recommendation:* no: in `bind:view`'s, else *All*, as RFC 0069 decided
   for filters. A view in the route is how a list opens in one.
4. **The page in a view?**
   *Recommendation:* no; a view starts on its first page.
5. **`make:resource --view` puts the view in the route?**
   *Recommendation:* yes, `?view=`, so its lists' views are links.

## Decisions

1. **The user's views are kept on this machine**, in the settings store.
2. **No widths in a view.**
3. **A list opens in `bind:view`'s view**, else *All*; not the last used.
4. **No page in a view**: it starts on its first.
5. **`make:resource --view` puts the view in the route** (`?view=`).

## Not in this RFC

- **Views shared with a team**, or synced between machines.
- **A view as a nav entry** (a sidebar item per view).
- **Default views per user** chosen in the app's settings.

## Alternatives considered

- **Keep the last filters** (`persist` for filters too): RFC 0069 decided
  against it; a list that opens on yesterday's question hides rows.
- **Views as the app's only**: the users' own questions are most of them.
- **Bookmarks of the route with the query in it**: works for links, gives
  the list no menu, and a long query string isn't a name.
