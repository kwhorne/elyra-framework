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
| `.timeout(d)` | the deadline (30 s by default) |
| `.retry(times, sleep)` | try again after a failed connection or a `5xx`, for `GET` / `PUT` / `DELETE` |
| `.retry_any_method()` | retry `POST` / `PATCH` too, when the server makes it safe |
| `.send().await` | a `Response`, or an `HttpError` when no answer came |

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

`?` turns it into a command error.

## Safety

- **HTTPS only**, except to loopback (`localhost`, `127.0.0.1`, `::1`), and that
  holds for every redirect too.
- **A redirect to another host or port doesn't carry `Authorization` or
  cookies.**
- **Logs** (`elyra::http`) show the method, the host, the path, the status and
  the duration. They never show the query, the body or a header.

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

Routes match the URL without its query. A trailing `*` matches the rest, and
a later route wins over an earlier one. A request with no route fails clearly,
rather than getting an empty answer.

## Related

- [RFC 0005](proposals/0005-laravel-backend.md) · [Secrets](secrets.md) ·
  [Testing](testing.md)
