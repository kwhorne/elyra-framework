# RFC 0038 — Edit it in its own app: opened in Excel or Word, saved back with ⌘S

**Status:** implemented (2026-11-20) · **Target:** 0.50.0 · builds on [RFC 0007](0007-offline-and-sync.md), [RFC 0009](0009-files.md), [RFC 0022](0022-multiple-servers.md) and [RFC 0031](0031-a-token-per-workspace.md) · see [Decisions](#decisions)

## Summary

Business apps keep files on records: the contract on a customer, the
budget on a project, the drawing on a work order. To change one on the
web, the user downloads it, finds it in Downloads, edits it, saves it, goes
back to the browser, finds the record again and uploads it, and then
there are two copies on their disk and they're not sure which one went up.

A desktop app can do what the web can't: **open the file in the user's
own app (Excel, Word, Preview, a CAD program) and send it back every
time they save**, as SharePoint and Google Drive for desktop do.

```rust
#[command(can = "attachments.update")]
async fn attachments_edit(ctx: Ctx, id: i64) -> elyra::Result<Editing> {
    Ok(ctx.get::<Backend>()
        .edit(format!("/api/attachments/{id}/file"))   // downloaded, opened in its app
        .save_back(format!("/api/attachments/{id}/file"))
        .open()
        .await?)
}
```

```svelte
<script>
  import { commands, editing } from "@elyra/runtime";
</script>

<Button onclick={() => commands.attachmentsEdit({ id: file.id })}>Edit in its app</Button>

{#each $editing as e (e.id)}
  <p>{e.name}: {e.state === "saved" ? `saved ${e.savedAt}` : e.state}</p>
{/each}
```

- **One call downloads the file into a folder of the app's own and opens
  it** in the user's default app for that kind of file.
- **Every save is sent back**: a minute after ⌘S at most, usually a few
  seconds. Offline, it waits in the outbox, and only the last save goes.
- **Someone else changed it meanwhile**: the save is a conflict, not an
  overwrite, and the user chooses, as with any other write (RFC 0007).
- **The page can't open anything but a file the app downloaded for
  editing**, and never a program.

## Motivation

- **It's the round trip office users hate most on the web.** Download,
  find, edit, upload, and the "which copy is right" afterwards. A business
  app on the desktop is chosen partly so this goes away.
- **The parts are all there.** RFC 0009 gave streamed downloads and
  uploads, `queue_offline()` with copies of the files, and the `transfers`
  store; RFC 0007 gave conflicts (`412`) with keep mine and take theirs;
  `shell.openExternal` opens a path in its app, under a policy. What's
  missing is noticing the save and tying it back to the record.
- **An app can't build it well on its own.** Doing it right means a folder
  per user and server, saves that land in the outbox correctly, a
  conflict that never overwrites the user's open file, sign-out that
  doesn't lose a save, and a policy so a server can't make the app run a
  program. Every app would get a different part of that wrong.

## Current state (0.49)

- **Downloads** (`Backend::download`, `download_into`, `download_to`):
  streamed to disk, under the server's cleaned name, marked as downloaded
  (quarantine on macOS, the Mark of the Web on Windows).
- **Uploads** (`.attach(..)`): multipart, streamed; with
  `.queue_offline()`, a copy of the file is kept in
  `<data>/uploads/<owner>/<key>/` and sent later, once, with its
  `Idempotency-Key`.
- **Conflicts**: `.unless_changed_since(updated_at)` sends
  `If-Unmodified-Since`; a `412` (or `409`) is kind `conflict`. Offline,
  the write waits in `failed`, and `Outbox::keep_mine` / `take_theirs`
  resolve it.
- **`shell.openExternal`** opens an absolute path that exists in its
  default app, and refuses anything executable anywhere in the path.
- **Sign-out** is refused (`pending_changes`) while writes wait.
- **Nothing watches a file.** The framework has no file watcher.

## Design

### Opening

`Backend::edit(url)` is a `GET` builder like any other (headers, query,
`can` on the command). `.open()`:

1. **Downloads the file** into the app's editing folder (below), under the
   server's cleaned name, so the editor's title bar shows "Contract
   Acme.docx", not a key. Marked as downloaded, as every download is.
2. **Notes the version** the server gave: `Last-Modified`, or, if the
   answer has none, `ETag`. It's what the first save is checked against.
3. **Checks the file's kind** against the policy (see
   [Security](#security)) and **opens it** with the OS's default app for
   it, as `shell.openExternal` does.
4. **Returns an `Editing`**: an id, the name, the state. It's in the
   `editing` store from then on.

Opening the same `edit` URL again, for the same user, while a copy is
being edited, **doesn't download it again**: it opens the copy that's
there, so the user's unsaved and unsent work isn't replaced.

### The editing folder

```
<data>/editing/<owner>/<id>/Contract Acme.docx
```

- **`<owner>`** is the server, workspace and user (RFC 0022, RFC 0031), as
  the outbox's uploads are kept, so two users of the machine and two
  servers never share a copy.
- **`<id>`** is one folder per file being edited, so two files of the same
  name from two records don't collide, and the editor's own lock and
  backup files (`~$Contract Acme.docx`, `.~lock.…#`) stay beside it.
- **Only the file of that name is watched.** Lock files, `.DS_Store` and
  an editor's temporary files are never sent.

### Noticing a save

Every second, the shell looks at the files being edited: their size and
modified time, one `stat` each. When one has changed:

1. **It waits until it's still**: the same size and modified time two
   checks in a row. Word and Excel save through a temporary file and a
   rename, and a large file is written over a few seconds.
2. **It reads it and hashes it** (SHA-256). The same hash as the last one
   sent is no save: an editor that touches a file on opening, or saves
   without a change, sends nothing.
3. **It copies it** to `<id>/.saved/<hash>`, and sends the copy. The editor
   can go on writing the original meanwhile.

A file the editor holds locked against reading (rare; some CAD programs)
is tried again on the next check.

### Saving back

`.save_back(url)` says where. The save is an upload:

```rust
backend.post(url)
    .field("_method", "PUT")                       // Laravel's method spoofing
    .attach("file", copy)
    .unless_changed_since(version)                 // the version it was opened at, or last saved as
    .queue_offline()
```

- **`POST` with `_method=PUT`**, because PHP parses a multipart body only
  for `POST`; Laravel routes it to `Route::put`. `.save_back_with(Method::Post, url)`
  sends a plain `POST`, for a server that keeps every save as a new
  version.
- **The version moves on**: the answer's `Last-Modified` (or `ETag`, or
  `updated_at` in a JSON body) is what the next save is checked against.
  So saving twice isn't a conflict with yourself.
- **Online, every save is sent**, in order. A server that keeps versions
  gets each one.
- **Offline, only the last save waits.** A save that finds an earlier save
  of the same file still in the outbox replaces it (and its copy): the
  user saved ten times on the train, and one upload goes when the network
  is back.
- **`transfers` shows the upload**, as any upload, and `editing` shows the
  state: `open`, `saving`, `saved` (with the time), `waiting` (offline),
  `conflict`, `denied` (a `403`), `failed` (with the server's message).

### Conflicts

Bob changes the contract on the web while Ada has it open in Word. Ada's
save is checked against the version she opened, the server answers `412`,
and her save is a conflict, in `editing` and in the outbox:

- **Keep mine**: sent again without the check. Bob's version is replaced.
- **Take theirs**: Bob's version is downloaded **beside** Ada's, as
  "Contract Acme (from the server).docx", and opened. Ada's file is never
  overwritten while it may be open in Word, which on Windows can't be
  done anyway, and Ada may want to copy her changes across. The editing
  carries on with the new file.
- **Neither yet**: Ada's copy stays, and further saves wait behind the
  conflict.

There's no merge. Word and Excel have their own compare, and the user has
both files to use it with.

### Ending

The framework can't know when Word is done with a file: editors keep
files open, closed, open again, and a document window isn't a process.
So an editing ends when:

- **The user says so**: `editing.finish(id)`, or a button the app shows.
  Refused while a save of it waits (`pending_changes`), as sign-out is;
  `finish(id, { discard: true })` throws the waiting save away.
- **Sign-out** (and ending a workspace): refused while a save waits, as
  today; once nothing waits, every editing of that owner ends and its
  folder is removed, so the next person at the machine finds nothing.
- **Seven days with no change**, at start, with nothing waiting.

**An editing outlives the app.** It's kept in the app's database, and the
next start looks at each file once: a file saved while the app wasn't
running is sent then, checked against the version, as any save.

### In the frontend

```ts
import { editing } from "@elyra/runtime";

$editing;                      // [{ id, name, state, savedAt, error, url }]
editing.show(id);              // open it in its app again
editing.reveal(id);            // show it in Finder / Explorer
editing.finish(id);            // stop: the copy is removed
editing.keepMine(id);          // a conflict, as the outbox's
editing.takeTheirs(id);
```

`elyra:editing` is the event, as `elyra:transfers` is. Every window sees
the same list.

### On the server

Nothing new is required. The `GET` answers with the file (and
`Last-Modified`, which `response()->file(..)` sets); the `PUT` takes
`$request->file('file')`, checks `If-Unmodified-Since` as RFC 0007
describes, and answers with the new `updated_at`:

```php
public function update(Request $request, Attachment $attachment)
{
    $this->authorize('update', $attachment);
    abort_if($attachment->changedSince($request), 412);
    $request->validate(['file' => ['required', 'file', 'max:51200']]);

    $attachment->replaceFile($request->file('file'));

    return response()->json($attachment)->setLastModified($attachment->updated_at);
}
```

`elyracode/elyra-backend` gets the `changedSince()` check as a trait for a
model (`ChecksUnmodifiedSince`): RFC 0007 left it to a few lines in each
controller, and this is the second place it's needed.

### Security

- **The page names no path.** `editing.show(id)` and `reveal(id)` take an
  editing's id; the shell opens only files in its own editing folder that
  it downloaded. `shell.openExternal` is unchanged.
- **Never a program.** The server's file name is checked before it's
  written: the executable kinds `shell.openExternal` refuses (`.exe`,
  `.app`, `.sh`, `.bat`, `.jar`, `.desktop`, …) plus the ones that run
  something when opened (`.lnk`, `.url`, `.scf`, `.webloc`, `.inetloc`,
  `.terminal`, `.command`, `.html`, `.svg`). `.edit(..).only(["docx",
  "xlsx", "pdf"])` narrows it further for an app that knows its files.
- **Downloads stay marked.** Quarantine on macOS, the Mark of the Web on
  Windows, as RFC 0009 decided: a file another user uploaded is still from
  the internet. On Windows, Office opens it in Protected View, and the
  user presses **Enable Editing** once. This is the price; see the open
  questions.
- **Saves go to the server the file came from**, with the token of the
  owner it was opened as. Switching workspace in a window doesn't send a
  save elsewhere.
- **The ability is the server's.** Starting an editing is a command with
  its `can`; the saves after it are uploads Laravel authorizes. An ability
  taken away meanwhile is a `403`, `denied`, and the copy stays.
- **Nothing of it in the log** but the editing's id and the state; the
  file's name is the user's data.

### Testing

- **Noticing a save**: unit tests over a folder in a temporary directory:
  a write, a write through rename, a write in two parts (not sent until
  still), a touch without a change (not sent), a lock file (ignored).
- **Saving back**, against the test server the backend tests already use:
  the `_method`, the precondition, the version moving on, `412` as a
  conflict, `403` as denied, offline then online with only the last save
  sent.
- **Against Laravel** in CI's backend job (`laravel_files.rs`): open, save
  twice, conflict.
- **Opening** is behind the same seam as `shell.openExternal`, so tests
  see what would be opened and nothing is.
- **By hand**: Word, Excel and Preview on macOS, Word and Excel on Windows
  (Protected View included), LibreOffice on Linux.

## Implementation plan

1. The editing folder, `Editing` and the database table; `Backend::edit`,
   `.save_back`, `.only`, `.open()`; the policy.
2. Noticing a save; sending it; the version; replacing a waiting save in
   the outbox.
3. Conflicts (beside, never over), ending, sign-out, the seven days, the
   look at start.
4. `@elyra/runtime`'s `editing` store and `elyra:editing`.
5. `ChecksUnmodifiedSince` in `elyracode/elyra-backend`; the backend job.
6. Docs: `docs/backend.md` (a section), `docs/system.md` (the policy),
   the Boost guideline.

## Not in this RFC

- **A component in `@elyra/ui`.** The store and the example above are
  enough to start; a component when apps show what they want.
- **Editing a file that isn't on a server**: the user's own file is
  already in their own app.
- **Locking the file on the server** while it's edited. Laravel has no
  standard for it, and the conflict covers the case.
- **Merging** two versions of a document.
- **Saying which app will open it** ("Edit in Excel"). The OS knows
  (`LSCopyDefaultApplicationURLForURL`, `AssocQueryString`), and it's a
  small addition later.

## Alternatives considered

- **A local WebDAV server**, as Office's own SharePoint integration does,
  opened with `ms-word:ofe|u|…`. Office would save straight to it, with
  locking. But it's Office only, needs a port and a server in the shell,
  and Preview and CAD programs don't speak it.
- **The OS's file providers** (File Provider on macOS, the Cloud Files API
  on Windows), as OneDrive and Dropbox do. The best experience, and a
  large, separate project per platform, with signing and entitlements.
  Not for a framework's first version of this.
- **A file watcher crate** (`notify`): events instead of a check every
  second. One more dependency with its own behaviour per platform, and
  network folders and editors that save through a rename are where
  watchers disagree. With a handful of files, a `stat` each second costs
  nothing.
- **Leave it to the app**: each app gets the folder, the outbox and the
  sign-out wrong in its own way.

## As built

- **`Backend::edit(path)`** returns an `EditRequest`: `.save_back(..)`,
  `.save_back_with(Method::Post, ..)`, `.only([..])`, `.header(..)`,
  `.label(..)`, `.open()`. Behind `backend` and `system` (the `open` crate
  opens the file). `Backend::editing_folder(..)` moves the folder from
  `editing` in the app's data folder.
- **The editing is its own outbox**, not a row in `elyra_outbox`. The file
  and a copy of the save in `.saved/` are what waits, and only the last
  save is kept, which the outbox couldn't do without replacing rows it may
  be sending; the version has to move on with each answer, which the outbox
  has no way to hand back. So it works without `Backend::offline()` and
  without a database: each editing is kept as `editing.json` beside its
  file, not in the app's database. A save with no answer (or a `5xx`, a
  `429`, a sign-in that expired) is `waiting` and tried again every 15
  seconds; a newer save goes at once. Sign-out counts waiting saves with
  the outbox's (`pending_changes`), through `Backend`'s own count, so every
  way of signing out (a server, a workspace, all of them) refuses the same.
- **The name is checked after the download**, not before: the server's name
  is known from its answer. A refused file is removed with its folder before
  anything opens it.
- **The owner** is the server and the user (and workspace) as the offline
  copy knows them; a save is sent with that server's token only while the
  same user and workspace are signed in there, and waits otherwise.
- **A file the editor holds** is read again at the next look; nothing is
  lost.
- **The store has no `url`**: the page has no use for the server's path,
  and an editing is named by its id. `savedAt` is milliseconds since 1970.
- **Tested**: 12 tests over a fake server (`tests/editing.rs`:
  the folder, a save, a touch, opening twice, programs refused, both
  conflict resolutions, offline, sign-out, finishing, a restart), the
  runtime's store, and one against a real Laravel app in CI's backend job
  (`laravel_files.rs`): saved, saved again, changed on the web, a `412`,
  theirs beside. Not yet tried with Word and Excel themselves; that's next,
  on macOS and Windows.

## Decisions

Settled on 2026-11-20, all as recommended:

1. **A check every second**, `stat` only, no file watcher and no new
   dependency.
2. **Online, every save is sent**, in order; offline, only the last save
   waits, replacing an earlier one in the outbox.
3. **Downloads stay marked** (quarantine, the Mark of the Web), as RFC 0009
   decided; on Windows, Office's Protected View asks for **Enable
   Editing** once. An opt-out later, if users ask.
4. **Take theirs puts the server's version beside** the user's file, never
   over a file the editor may have open.
5. **An editing ends** when the user finishes it, at sign-out, or after
   seven days with no change; it outlives the app.
6. **Saves are `PUT` through `_method`** by default;
   `save_back_with(Method::Post, ..)` for a server that keeps versions.
7. **No component in `@elyra/ui`** in this RFC: the `editing` store and an
   example.
