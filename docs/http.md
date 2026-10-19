# HTTP client

`Http` is Laravel's `Http` facade in Rust, over `reqwest` (feature `http`).
`App` binds one, so a command resolves it. It's the base for the
[Laravel backend](proposals/0005-laravel-backend.md) (RFC 0005), and works for
any other API.

```rust
use elyra::http::Http;
use std::time::Duration;

#[command]
async fn rates(ctx: Ctx) -> elyra::Result<Rates> {
    let res = ctx.get::<Http>()
        .get("https://api.example.com/rates")
        .query(&[("base", "NOK")])
        .token(&api_key)                       // Authorization: Bearer …
        .timeout(Duration::from_secs(10))
        .retry(3, Duration::from_millis(200))
        .send()
        .await?;
    Ok(res.json()?)
}
```

## The request

| Method | Does |
| --- | --- |
| `.get(url)` / `.post` / `.put` / `.patch` / `.delete` | start a request |
| `.query(&q)` | query parameters: a struct, a map, or pairs. `None` fields are left out; a list becomes `key[]=…`, as Laravel reads it |
| `.json(&body)` | a JSON body |
| `.header(name, value)` / `.token(t)` | headers; `token` is `Authorization: Bearer` |
| `.attach(name, path)` | a file, streamed from disk as a `multipart/form-data` part ([files](#files)) |
| `.attach_as(name, path, file_name, type)` / `.attach_bytes(name, bytes, file_name, type)` | a file under another name and type, or bytes as a file |
| `.field(name, value)` / `.fields(&f)` | text fields beside the files; `fields` flattens a struct as `query` does |
| `.on_progress(f)` | called with the bytes sent or received so far, and the total |
| `.timeout(d)` | the deadline (30 s by default); for a transfer, the time without progress |
| `.retry(times, sleep)` | try again after a failed connection or a `5xx`, for `GET` / `PUT` / `DELETE` |
| `.retry_any_method()` | retry `POST` / `PATCH` too, when the server makes it safe |
| `.send().await` | a `Response`, or an `HttpError` when no answer came |
| `.download_to(path).await` / `.download_into(dir).await` | stream a successful answer to disk ([files](#files)) |

Every request asks for JSON (`Accept: application/json`), which is also what
makes Laravel answer a failed validation with `422` JSON instead of a
redirect.

## The response

Any status is a `Response`: `res.status()`, `res.ok()` (2xx), `res.header(name)`,
`res.text()`, `res.bytes()`, `res.json::<T>()`. What a `404` means is up to
the caller. The body is read whole, up to 32 MiB.

An `HttpError` means no usable answer:

- `Unreachable`: the connection, DNS, TLS, or the timeout.
- `Insecure`: not HTTPS.
- `InvalidUrl`.
- `Encode` / `Decode`: the query or body couldn't be encoded, or the
  response isn't the type asked for.
- `TooLarge`.
- `File`: a file to send or save to couldn't be read or written.
- `FileChanged`: a file's size or modification time changed while it was
  sent.

`?` turns it into a command error.

## Files

An attached file makes the body `multipart/form-data`, which Laravel reads
with `$request->file('receipt')`. Files are read from disk in chunks while
they're sent, so a large one costs no more memory than a small one:

```rust
let res = ctx.get::<Http>()
    .post("https://api.example.com/expenses")
    .attach("receipt", &path)               // its name, and a type from its extension
    .fields(&expense)                       // amount=129.5, paid=1, tags[]=…
    .on_progress(|p| println!("{} of {:?}", p.done, p.total))
    .send()
    .await?;
```

- The body has a known length (`Content-Length`), which PHP and nginx
  prefer to chunks.
- A retry reads the file again from the start. A `POST` is retried only with
  `.retry_any_method()`, when the server makes repeating it safe.
- If the file's size or modification time changes while it's sent, the
  request stops with `FileChanged` instead of sending a mix of two versions.

A download streams a successful answer to disk instead of into memory, so
the 32 MiB limit doesn't apply:

```rust
// To a path the user chose in a save dialog, replacing it:
http.get(url).download_to(&path).await?;

// Into a folder, under the server's name:
let res = http.get(url).download_into(&folder).await?;
let saved = res.saved_to();   // Some("…/Invoice 7.pdf")
```

- The body is written to `<name>.part` and renamed when it's complete, so a
  download that stops halfway leaves nothing that looks finished.
- `download_into` names the file from `Content-Disposition` (`filename*`
  first) or the URL's last segment. The name is cleaned: no folders, no
  leading dot, none of the characters a file system refuses, not a Windows
  device name. A taken name is numbered: `Invoice 7 (1).pdf`.
- An answer that isn't a success isn't saved: it's an ordinary `Response`
  with its body, and `saved_to()` is `None`.

For a transfer, `.timeout(d)` is the time without progress: a slow upload or
download that keeps moving isn't cut off, and one that stalls for `d` is.

## Safety

- **HTTPS only**, except to loopback (`localhost`, `127.0.0.1`, `::1`), and that
  holds for every redirect too.
- **A redirect to another host or port doesn't carry `Authorization` or
  cookies.**
- **Logs** (`elyra::http`) show the method, the host, the path, the status and
  the duration, and a transfer's size. They never show the query, the body,
  a header, or a file's name.
- **A file error names the file, not its folder**, which can hold the
  user's name.

## Testing

`HttpFake` answers from a table, never touches the network, and records each
request:

```rust
use elyra::http::{Http, HttpFake, Method};

let fake = HttpFake::new()
    .get("https://api.example.com/rates", 200, json!({ "nok": 11.0 }))
    .post("https://api.example.com/orders", 422, json!({ "errors": { "qty": ["Too many."] } }))
    .unreachable(Method::Get, "https://api.example.com/down");
let app = TestApp::new(app().swap(Http::fake(fake.clone())));

// … invoke commands …
let sent = fake.assert_sent("GET", "https://api.example.com/rates");
assert_eq!(sent.header("authorization"), Some("Bearer t0ken"));
fake.assert_not_sent("DELETE", "https://api.example.com/orders/1");
```

Multipart parts are recorded too, and `get_bytes` answers a download:

```rust
let fake = HttpFake::new()
    .post("https://api.example.com/expenses", 201, json!({ "id": 1 }))
    .get_bytes("https://api.example.com/invoices/7/pdf", 200, pdf, "invoice-7.pdf");
// …
let sent = fake.assert_sent("POST", "https://api.example.com/expenses");
let receipt = sent.part("receipt").unwrap();
assert_eq!(receipt.file_name.as_deref(), Some("receipt.pdf"));
assert_eq!(sent.part("amount").unwrap().text(), "129.5");
```

Routes match the URL without its query. A trailing `*` matches the rest, and
a later route wins over an earlier one. A request with no route fails clearly,
rather than getting an empty answer.

## Related

- [RFC 0005](proposals/0005-laravel-backend.md) · [RFC 0009](proposals/0009-files.md) · [Secrets](secrets.md) ·
  [Testing](testing.md)
