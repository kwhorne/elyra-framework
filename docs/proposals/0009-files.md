# RFC 0009 — Files between the desktop and Laravel: uploads, downloads, offline

**Status:** implemented (2026-10-19) · **Target:** 0.17.0 · the guides: [docs/backend.md](../backend.md#files), [docs/http.md](../http.md#files), [docs/system.md](../system.md#a-file-the-user-chose) · builds on [RFC 0005](0005-laravel-backend.md) and [RFC 0007](0007-offline-and-sync.md) · see [Decisions](#decisions)

## Summary

Business apps attach things: a contract to a customer, a receipt to an
expense, a photo to a work order. They also hand them back, as an invoice's
PDF or an export. On the web it's an `<input type="file">` and a link. A
desktop app can do better: the user drops a file from Finder, the upload
shows its progress in the app, it waits for the network when there is none,
and the PDF goes where the user wants it, from the server straight to disk.
Today an Elyra app can't do either. `Http` sends only JSON and keeps every
answer in memory.

```rust
#[command(can = "customers.update")]
async fn customers_attach(ctx: Ctx, id: i64, file: ChosenFile) -> elyra::Result<Attachment> {
    Ok(ctx.get::<Backend>()
        .post(format!("/api/customers/{id}/attachments"))
        .attach("file", &file)          // streamed from disk, with progress
        .field("note", "Signed copy")
        .queue_offline()                // no network: it waits in the outbox
        .json()
        .await?)
}

#[command(can = "invoices.view")]
async fn invoices_pdf(ctx: Ctx, id: i64, to: ChosenFile) -> elyra::Result<()> {
    ctx.get::<Backend>()
        .get(format!("/api/invoices/{id}/pdf"))
        .download_to(&to)               // streamed to disk, never all in memory
        .await?;
    Ok(())
}
```

```ts
const [file] = await dialog.open({ filters: [{ name: "PDF", extensions: ["pdf"] }] });
await commands.customersAttach({ id: 7, file });      // `transfers` shows the progress
```

```php
public function store(Request $request, Customer $customer)
{
    $request->validate(['file' => ['required', 'file', 'max:20480'], 'note' => ['nullable', 'string']]);

    return $customer->attachments()->create([
        'path' => $request->file('file')->store('attachments'),
        'name' => $request->file('file')->getClientOriginalName(),
        'note' => $request->input('note'),
    ]);
}
```

- **Uploads are multipart**, as Laravel's `$request->file(..)` expects. They
  are streamed from disk, so a 2 GB video costs no more memory than a PDF.
- **The frontend hands over a file the user chose**, from a dialog or a drop,
  as a `ChosenFile`. It can't make the app upload a path the user didn't
  choose.
- **Laravel's `422` on a file** (`max`, `mimes`) is shown on the field, like
  any other. So is the web server's `413`, which otherwise looks like an
  empty request.
- **Offline, an upload waits in the outbox** (RFC 0007), with its own copy of
  the file, and is sent once.
- **Downloads are streamed to disk**: to a file the user chose, or to the
  Downloads folder under the server's name. They are marked as coming from
  the internet, as a browser's are.
- **Progress** of every upload and download is in the `transfers` store,
  where a transfer can be cancelled.

## Motivation

- **It's the gap in the Laravel line.** Signing in, the API, broadcasting,
  offline and notifications are in place (RFC 0005–0008). An app that can't
  attach a file to a customer can't replace the web half for most users.
- **It's what the desktop is good at.** Files are already on the desktop:
  drag-and-drop from Finder, a save dialog that remembers its folder, a
  download that doesn't stall the UI. The browser's version of each of these
  is worse.
- **Doing it by hand is easy to get wrong.** Every app would otherwise read
  the whole file into memory, base64 it through the IPC (capped at 16 MiB),
  lose the upload when the network drops, or write a server-chosen file name
  into a path without checking it.
- **Laravel has the model.** `UploadedFile`, validation rules for files,
  `Storage::download(..)` and signed URLs are all standard. The desktop
  should fit them, not invent a protocol.

## Current state (0.16)

- `Http` is reqwest 0.12, already built with its `multipart` and `stream`
  features, but the request body is a `Vec<u8>` of JSON. A response is read
  whole into memory and refused above 32 MiB (`http::MAX_BODY`).
- A request's timeout (30 s by default) covers the whole request. A large
  upload on a slow line would hit it.
- `BackendRequest` adds the token, maps `422` to `ValidationErrors` and
  `401` to signing out, and `.queue_offline()` puts a write in the outbox
  with an `Idempotency-Key`.
- The outbox's `body` column is text: the JSON body. There's no place for a
  file.
- `dialog.open(..)` / `dialog.save(..)` (the `system` feature) and
  `onFileDrop(..)` give the frontend paths as plain strings. Any command can
  take a string and read that path; nothing ties it to the user's choice.
- `system::paths()` has no Downloads folder.
- `make:resource --generate` knows `string`, `text`, `email`, `date`,
  `integer`, `float`, `bool`, `json` and `references`. It has no `file`.

## Design

### A file the user chose

```rust
pub struct ChosenFile { /* a path, and how it was chosen */ }

impl ChosenFile {
    pub fn path(&self) -> &Path;
    pub fn name(&self) -> &str;          // the file name, for display
    pub fn size(&self) -> Option<u64>;   // for an existing file
    pub fn from_path(path: impl Into<PathBuf>) -> Self;  // Rust code's own files
}
```

- **On the wire it's a grant, not a path.** When a dialog returns or files are
  dropped, the shell records each path in a per-run list and hands the
  frontend an opaque id for it, alongside the path for display. A command
  argument of type `ChosenFile` is decoded from that id, so a path the user
  didn't choose doesn't decode. In TypeScript it's a branded type
  (`ChosenFile`) from codegen, with `name` and `path`.
- **What it grants:** a file from `dialog.open(..)` or a drop can be read; a
  path from `dialog.save(..)` can be written; a folder chosen with
  `dialog.open({ directory: true })` lets a download write a new file in it.
- **Rust code isn't limited.** `ChosenFile::from_path(..)` is for a file the
  app makes or knows itself, such as an export in its own data folder. Only
  the decoding from the frontend checks the grant.
- **Grants last for the run.** They aren't stored, and a restart forgets them.

### Uploads

```rust
backend.post("/api/expenses")
    .attach("receipt", &file)                    // a ChosenFile
    .attach_as("scan", &file, "scan.jpg", "image/jpeg")
    .attach_bytes("thumbnail", bytes, "thumb.png", "image/png")
    .field("amount", "129.00")
    .fields(&expense)                            // a struct's fields as text parts
    .send()
    .await?;
```

- **Multipart, streamed.** A request with `.attach(..)` is
  `multipart/form-data`. The file is read in chunks while it's sent. The
  content type is guessed from the extension, and `.attach_as(..)` sets it.
- **Fields as Laravel reads them.** `.fields(&value)` flattens a struct with
  the same rules as `.query(..)`: `None` left out, lists as `key[]`, and
  booleans as `1`/`0`, which Laravel's `boolean` rule accepts. A method other
  than `POST` is sent as `POST` with `_method=PUT`, as PHP reads files only
  from a `POST`.
- **Retries.** An upload is retried on a failed connection or a `5xx` only
  with an `Idempotency-Key`, which `.queue_offline()` or `.idempotent()` adds,
  as the outbox does today. The file is read again from the start.
- **Timeouts.** For a transfer, the timeout is the time without progress (no
  bytes sent or received for 30 s, unless set), not the whole request.
- **The file changed while sending.** If the size or modification time
  differs from when it started, the upload stops with an error, rather than
  sending half of two files.

### Laravel's answers to an upload

- **`422`** is `ValidationErrors`, as today; `file.0` and `files.*` keep
  their keys, so a form shows the message on the field.
- **`413`** (nginx's `client_max_body_size`, or PHP's `post_max_size`, when
  Laravel answers it with `PostTooLargeException`) becomes
  `ValidationErrors` on the attached field: "The file is too large for the
  server." Without this, the user sees a generic error, or, when PHP drops
  the body, a `422` saying the file is required.
- **Client-side limits.** `.max_size(bytes)` refuses a larger file before
  sending it, with the same validation error. It's optional: the server's
  rule is what counts.

### Downloads

```rust
// To a file the user chose with dialog.save(..):
backend.get("/api/invoices/7/pdf").download_to(&to).await?;

// Into a folder, under the server's name (Content-Disposition):
let saved: PathBuf = backend.get("/api/exports/42").download_into(&folder).await?;

// Into the Downloads folder:
let saved = backend.get("/api/exports/42").download().await?;
```

- **Streamed to disk.** The body is written to `<name>.part` next to the
  target and renamed when complete, so a cancelled or failed download leaves
  nothing that looks finished. No `MAX_BODY` applies.
- **The server's name is cleaned.** It's taken from `Content-Disposition`
  (`filename*` first), or else from the URL's last segment. Path separators,
  `..`, control characters and a leading dot are removed, and Windows'
  reserved names are avoided. An existing file isn't overwritten: the new
  one is `invoice (1).pdf`. `download_to(..)` overwrites, because the user
  chose that path in a save dialog, which asked them.
- **Marked as from the internet.** On macOS the file gets the
  `com.apple.quarantine` attribute, and on Windows the Mark of the Web
  (`Zone.Identifier`), as a browser's download does, so the OS checks it
  before it's opened. Nothing is opened by itself.
- **Signed URLs.** `Storage::temporaryUrl(..)` on S3 answers with a URL on
  another host. A redirect there already drops `Authorization`, and
  `Backend::download_url(url)` fetches an absolute URL the API returned
  without the token.
- **`system::paths()` gains `downloads`**, and the frontend's `paths()`
  too.

### Progress and cancelling

```rust
pub struct Transfer {
    pub id: String,
    pub direction: Direction,        // Upload | Download
    pub name: String,                // the file's name
    pub sent: u64,                   // bytes so far
    pub total: Option<u64>,
    pub state: TransferState,        // Running | Queued | Done | Failed | Cancelled
}
```

- **`Transfers`**, in the container, lists what's running and what finished
  in the last minute. It's published on `elyra:transfers`, at most ten times a
  second.
- **Cancelling.** `transfers.cancel(id)` from the frontend, or
  `Transfers::cancel(id)` in Rust, stops it. The command that started it gets
  a `cancelled` error. A queued upload is removed from the outbox.
- **A label.** `.label("Contract for Acme")` names the transfer for the
  UI; otherwise it's the file's name.

### Offline uploads

- **A copy is queued.** `.queue_offline()` with an attached file copies the
  file into the app's data folder (`elyra-outbox-files/<owner>/<key>`) when
  it's queued. The user can move, change or delete the original, and what is
  sent is what they attached. The outbox row's body describes the parts, and
  points to the copies.
- **Limits.** `Backend::offline_upload_limit(bytes)` caps the copies
  (default 500 MB per user). Over it, the upload fails at once with an
  `offline` error, rather than filling the disk.
- **Sent once, then removed.** The copy is deleted when the server answers
  with a success or the user discards the write. A refused upload keeps its
  copy in `elyra_outbox_failed` until retried or discarded, like any write.
- **Per user.** Copies belong to the user who queued them, and go with the
  outbox when that user's writes are discarded at sign-out.
- **Progress.** A queued upload is a `Queued` transfer, and a `Running` one
  while the outbox sends it.

### The frontend

```ts
import { dialog, onFileDrop, transfers, type ChosenFile } from "@elyra/runtime";

const files: ChosenFile[] = await dialog.open({ multiple: true });
onFileDrop((files) => attach(files));        // ChosenFile[] now, not string[]

$transfers   // Transfer[]: name, direction, sent, total, state
transfers.cancel(id);
```

- `dialog.open(..)`, `dialog.save(..)` and `onFileDrop(..)` return
  `ChosenFile`s. A `ChosenFile` has `.path` and `.name`, so code that
  displays the path keeps working. Code that passes it on as a `string` stops
  type-checking, which is where a command's argument should become
  `ChosenFile`.
- **A file field.** `make:resource` writes a `FileField.svelte` into the app
  (it's the app's to change): a button and a drop zone, the chosen name, the
  progress while it's sent, and the field's validation message.

### `make:resource --backend` and a `file` field

```bash
rata make:resource Customer --backend --generate "name:string,contract:file?"
```

- The command takes `contract: Option<ChosenFile>` and sends multipart when
  there's one. The form has the file field. The detail view links the
  attachment, downloaded with `download_to(..)` after a save dialog.
- The Laravel side is printed, as for notifications: the validation rule,
  `store(..)` on the request's file, and a download route with
  `Storage::download($path, $name)`.

### Testing

```rust
let fake = HttpFake::new()
    .post("https://crm.test/api/customers/7/attachments", 201, json!({ "id": 1 }))
    .get_bytes("https://crm.test/api/invoices/7/pdf", 200, pdf, "invoice-7.pdf");

let sent = fake.assert_sent("POST", "https://crm.test/api/customers/7/attachments");
assert_eq!(sent.part("file").unwrap().file_name(), Some("contract.pdf"));
assert_eq!(sent.part("note").unwrap().text(), "Signed copy");
```

- `Sent::part(name)` gives a part's name, file name, content type and
  bytes. `get_bytes(..)` answers a download with a body and a
  `Content-Disposition`.
- `ChosenFile::from_path(..)` stands in for the user's choice in tests.
- `TestApp` invokes a command with a `ChosenFile` argument through
  `testing::chosen(path)`, which grants it, so the IPC path is tested as
  well.

## Security

- **Only what the user chose.** A compromised or buggy frontend can't make
  the app upload `~/.ssh/id_ed25519`. A `ChosenFile` from the frontend
  decodes only from a grant made by a dialog or a drop in this run. A save
  dialog's grant writes only that path.
- **Downloaded names are untrusted.** They are cleaned before use, can't
  leave the target folder, don't overwrite, and are written through a
  `.part` file.
- **Downloads are marked**, quarantine on macOS and the Mark of the Web on
  Windows, and never opened by the framework.
- **The token stays with the API.** It isn't sent on a redirect to another
  host or to an absolute URL fetched with `download_url(..)`.
- **Logs** carry the method, host, path, status, size and duration. They
  never include a file's name or contents, which are often personal.
- **Queued copies** are per user, in the app's own data folder, removed when
  sent or discarded. They're as private as the files they copy, no more.
- **The frontend** reaches transfers only with `Capability::Auth`, like the
  outbox.

## Alternatives considered

- **Bytes over the IPC.** The frontend reads the file with a
  `FileReader` and sends it to a command. It's simple but capped at 16 MiB,
  it doubles memory, and offline copies would be the frontend's job. A drop
  gives the shell a path anyway.
- **Upload from the webview with `fetch`.** It would need the token in the
  frontend, which RFC 0005 keeps out of it, and CORS on the Laravel app.
- **Plain paths, and each command checks them.** No new type, but every app
  would have to remember to check, and a forgotten check is a file leak.
- **Chunked, resumable uploads (tus, or Laravel packages like
  `pion/laravel-chunk-upload`).** They're better for multi-gigabyte files on
  bad lines, but need a protocol on the server too. They can be added later
  behind the same `.attach(..)` (question 6).
- **Reference the original in the outbox, not a copy.** It saves disk, but
  what's sent could then differ from what was attached, or be gone.

## Implementation plan

1. **Multipart and streaming in `Http`.**
   - `.attach(..)`, `.attach_as(..)`, `.attach_bytes(..)`, `.field(..)`,
     `.fields(..)`, streamed from disk.
   - `download_to(..)` / `download_into(..)` through `.part`, cleaned names
     and the timeout without progress.
   - `HttpFake` parts and `get_bytes(..)`.
2. **`ChosenFile`.** Grants from dialogs and drops, decoding a command
   argument, the codegen type, and `testing::chosen(..)`.
3. **The backend.**
   - Uploads with the token, `422`/`413` on the field, idempotent retries,
     and `.max_size(..)`.
   - Downloads with `Content-Disposition`, quarantine / Mark of the Web,
     `download_url(..)` and `paths().downloads`.
   - `Transfers` and `elyra:transfers`, with cancelling.
4. **Offline uploads.** Copies in the outbox, their limit, removal, and the
   failed list.
5. **The frontend.** `ChosenFile` from `dialog`/`onFileDrop`, the
   `transfers` store, and `/__auth/transfer-cancel`.
6. **`make:resource --backend` with `file`,** `FileField.svelte`, the Laravel
   side printed, attachments in `scripts/laravel-backend.sh`, CI uploading
   and downloading against the real Laravel app, and the docs.

As built, a few details differ from the text above:

- `ChosenFile`'s kinds are the `FileChoice` enum (`open`, `save`,
  `folder`). Rust code grants its own files with `ChosenFile::from_path`,
  `for_saving` and `folder`, which tests use too; there's no
  `testing::chosen`.
- `Sent::part(name)` gives a `SentPart` whose `name`, `file_name`,
  `content_type` and `bytes` are fields, with `text()`.
- `.idempotent()` adds the key and allows a retry; how many is still
  `.retry(..)`.
- `Backend::offline_upload_folder(..)` moves the queued copies. A refused
  upload is retried as it was: a retry with a new body is refused.
- A file field's download command is `<plural>_<field>(id, to)`, and returns
  where it saved. `FileField.svelte` lives in `app/src/resources/`; only the
  first file field on a form takes a dropped file.
- On Linux there's no mark for a downloaded file, so none is set.
- The real-app CI doesn't send a body over PHP's `post_max_size`: PHP's
  built-in server prints its warning before Laravel can answer `413`. The
  `413` is tested with a fake.
- Codegen names an `Option<T>` argument of a named type `T | null`, which a
  `file` field's commands needed.

## Open questions

1. **How the frontend hands over a file:** a `ChosenFile` grant from a dialog
   or a drop (recommended), plain path strings that each command checks, or
   the file's bytes over the IPC?
2. **Offline uploads:** copy the file into the app's data folder when it's
   queued (recommended), keep a reference to the original, or don't queue
   uploads at all?
3. **Progress:** one `transfers` store for every transfer, with cancelling
   (recommended), or a progress callback per call only?
4. **Where a download goes by default:** `download()` into the Downloads
   folder under the server's cleaned name, alongside `download_to(..)` for a
   save dialog's path (recommended), or always an explicit path?
5. **Marking downloads:** quarantine on macOS and the Mark of the Web on
   Windows (recommended), or leave files unmarked as other HTTP clients do?
6. **Resumable uploads:** not in this RFC; one streamed multipart request
   with idempotent retries (recommended), or tus now?
7. **`make:resource --backend`:** a `file` field type with a generated
   `FileField.svelte` and the Laravel side printed (recommended), or leave
   files out of the generator for now?
8. **`onFileDrop` and `dialog`:** change them to return `ChosenFile`
   (recommended: `.path` keeps display code working, and a `string` passed
   to a command stops type-checking), or add new functions and leave these
   as they are?

## Decisions

Settled on 2026-10-19, all as recommended:

1. **`ChosenFile`:** the frontend hands over a grant from a dialog or a drop
   in this run, never a bare path or the file's bytes.
2. **Offline uploads copy the file** into the app's data folder when they're
   queued, within `offline_upload_limit(..)`.
3. **One `transfers` store** for every upload and download, with
   cancelling.
4. **Downloads:** `download()` into the Downloads folder under the server's
   cleaned name, `download_into(..)` a chosen folder, and `download_to(..)`
   a save dialog's path.
5. **Downloads are marked** with quarantine on macOS and the Mark of the Web
   on Windows.
6. **No resumable uploads** in this RFC: one streamed multipart request,
   retried only with an `Idempotency-Key`.
7. **`make:resource --backend` gets a `file` field,** with a generated
   `FileField.svelte` and the Laravel side printed.
8. **`dialog` and `onFileDrop` return `ChosenFile`s,** with `.path` and
   `.name`; the change is called out in the changelog's upgrading note.
