//! The facade routes: `/__store`, `/__cache`, `/__storage`, `/__queue`,
//! `/__window`, `/__sys`, `/__sidecar`, `/__autostart`.
//!
//! Each is a thin MessagePack adapter over a Rust-side service the container
//! already holds — the frontend half of the same `Cache`/`Storage`/`Queue`
//! facades a command uses directly. Access is decided in [`guard`](super::guard)
//! before any of this runs; what remains here is decoding, dispatch, and the two
//! allowlist checks that need the argument itself (`shell.open`, sidecar spawn).

use crate::window::Windows;

use super::protocol::{msgpack_err, msgpack_ok, Body};
use super::Runner;

#[derive(serde::Deserialize)]
struct StoreSet {
    key: String,
    value: serde_json::Value,
}

#[derive(serde::Deserialize)]
struct StoragePut {
    path: String,
    contents: String,
}

#[derive(serde::Deserialize)]
struct QueuePush {
    job: String,
    #[serde(default)]
    payload: serde_json::Value,
}

#[derive(serde::Deserialize)]
struct CachePut {
    key: String,
    value: serde_json::Value,
    /// Time-to-live in seconds; `None` = forever.
    ttl: Option<u64>,
}

#[derive(serde::Deserialize)]
struct CacheIncr {
    key: String,
    #[serde(default)]
    by: Option<i64>,
}

/// `POST /__store/<op>` — the persistent key-value settings store.
pub(super) fn serve_store(runner: &Runner, op: &str, body: Vec<u8>) -> Body {
    let Some(store) = runner.ctx.try_get::<crate::store::Store>() else {
        return msgpack_err("store unavailable".into());
    };
    match op {
        "get" => match rmp_serde::from_slice::<String>(&body) {
            Ok(key) => msgpack_ok(&store.get(&key)),
            Err(e) => msgpack_err(e.to_string()),
        },
        "set" => match rmp_serde::from_slice::<StoreSet>(&body) {
            Ok(arg) => {
                store.set(arg.key, arg.value);
                msgpack_ok(&())
            }
            Err(e) => msgpack_err(e.to_string()),
        },
        "delete" => match rmp_serde::from_slice::<String>(&body) {
            Ok(key) => msgpack_ok(&store.delete(&key)),
            Err(e) => msgpack_err(e.to_string()),
        },
        "all" => msgpack_ok(&store.all()),
        "clear" => {
            store.clear();
            msgpack_ok(&())
        }
        other => msgpack_err(format!("unknown store op: {other}")),
    }
}

/// `POST /__cache/<op>` — the in-process cache facade (needs `CacheProvider`).
pub(super) fn serve_cache(runner: &Runner, op: &str, body: Vec<u8>) -> Body {
    let Some(cache) = runner.ctx.try_get::<crate::cache::Cache>() else {
        return msgpack_err("cache unavailable (add CacheProvider)".into());
    };
    let ttl = |secs: Option<u64>| secs.map(std::time::Duration::from_secs);
    match op {
        "get" => match rmp_serde::from_slice::<String>(&body) {
            Ok(key) => msgpack_ok(&cache.get(&key)),
            Err(e) => msgpack_err(e.to_string()),
        },
        "put" => match rmp_serde::from_slice::<CachePut>(&body) {
            Ok(a) => {
                cache.put(a.key, a.value, ttl(a.ttl));
                msgpack_ok(&())
            }
            Err(e) => msgpack_err(e.to_string()),
        },
        "add" => match rmp_serde::from_slice::<CachePut>(&body) {
            Ok(a) => msgpack_ok(&cache.add(a.key, a.value, ttl(a.ttl))),
            Err(e) => msgpack_err(e.to_string()),
        },
        "has" => match rmp_serde::from_slice::<String>(&body) {
            Ok(key) => msgpack_ok(&cache.has(&key)),
            Err(e) => msgpack_err(e.to_string()),
        },
        "forget" => match rmp_serde::from_slice::<String>(&body) {
            Ok(key) => msgpack_ok(&cache.forget(&key)),
            Err(e) => msgpack_err(e.to_string()),
        },
        "increment" => match rmp_serde::from_slice::<CacheIncr>(&body) {
            Ok(a) => msgpack_ok(&cache.increment(&a.key, a.by.unwrap_or(1))),
            Err(e) => msgpack_err(e.to_string()),
        },
        "decrement" => match rmp_serde::from_slice::<CacheIncr>(&body) {
            Ok(a) => msgpack_ok(&cache.decrement(&a.key, a.by.unwrap_or(1))),
            Err(e) => msgpack_err(e.to_string()),
        },
        "flush" => {
            cache.flush();
            msgpack_ok(&())
        }
        other => msgpack_err(format!("unknown cache op: {other}")),
    }
}

/// `POST /__storage/<op>` — the filesystem storage facade (needs `StorageProvider`).
pub(super) fn serve_storage(runner: &Runner, op: &str, body: Vec<u8>) -> Body {
    let Some(storage) = runner.ctx.try_get::<crate::storage::Storage>() else {
        return msgpack_err("storage unavailable (add StorageProvider)".into());
    };
    let io_err = |e: std::io::Error| msgpack_err(e.to_string());
    match op {
        "put" => match rmp_serde::from_slice::<StoragePut>(&body) {
            Ok(a) => match storage.put_str(&a.path, &a.contents) {
                Ok(()) => msgpack_ok(&()),
                Err(e) => io_err(e),
            },
            Err(e) => msgpack_err(e.to_string()),
        },
        "get" => match rmp_serde::from_slice::<String>(&body) {
            Ok(p) => match storage.get_str(&p) {
                Ok(s) => msgpack_ok(&s),
                Err(e) => io_err(e),
            },
            Err(e) => msgpack_err(e.to_string()),
        },
        "exists" => match rmp_serde::from_slice::<String>(&body) {
            Ok(p) => msgpack_ok(&storage.exists(&p)),
            Err(e) => msgpack_err(e.to_string()),
        },
        "delete" => match rmp_serde::from_slice::<String>(&body) {
            Ok(p) => match storage.delete(&p) {
                Ok(()) => msgpack_ok(&()),
                Err(e) => io_err(e),
            },
            Err(e) => msgpack_err(e.to_string()),
        },
        "size" => match rmp_serde::from_slice::<String>(&body) {
            Ok(p) => match storage.size(&p) {
                Ok(n) => msgpack_ok(&n),
                Err(e) => io_err(e),
            },
            Err(e) => msgpack_err(e.to_string()),
        },
        "url" => match rmp_serde::from_slice::<String>(&body) {
            Ok(p) => match storage.url(&p) {
                Ok(u) => msgpack_ok(&u),
                Err(e) => io_err(e),
            },
            Err(e) => msgpack_err(e.to_string()),
        },
        "files" => match rmp_serde::from_slice::<String>(&body) {
            Ok(p) => match storage.files(&p) {
                Ok(list) => msgpack_ok(&list),
                Err(e) => io_err(e),
            },
            Err(e) => msgpack_err(e.to_string()),
        },
        other => msgpack_err(format!("unknown storage op: {other}")),
    }
}

/// `POST /__queue/<op>` — enqueue jobs (needs `QueueProvider`; handlers are Rust-side).
pub(super) fn serve_queue(runner: &Runner, op: &str, body: Vec<u8>) -> Body {
    let Some(queue) = runner.ctx.try_get::<crate::queue::Queue>() else {
        return msgpack_err("queue unavailable (add QueueProvider)".into());
    };
    match op {
        // `false` means the queue was full — surface it instead of pretending
        // the job was accepted.
        "push" => match rmp_serde::from_slice::<QueuePush>(&body) {
            Ok(a) => match queue.push(a.job, a.payload) {
                true => msgpack_ok(&true),
                false => msgpack_err("queue is full; job was not enqueued".into()),
            },
            Err(e) => msgpack_err(e.to_string()),
        },
        other => msgpack_err(format!("unknown queue op: {other}")),
    }
}

/// `POST /__window/<op>` — window control from the frontend.
pub(super) fn serve_window(runner: &Runner, op: &str, body: Vec<u8>) -> Body {
    let Some(windows) = runner.ctx.try_get::<Windows>() else {
        return msgpack_err("window control unavailable".into());
    };
    let ok = match op {
        "minimize" => windows.minimize(None),
        "toggle_maximize" => windows.toggle_maximize(None),
        "toggle_fullscreen" => windows.toggle_fullscreen(None),
        "close" => windows.close(None),
        "focus" => windows.focus(None),
        "show" => windows.show(None),
        "hide" => windows.hide(None),
        "center" => windows.center(None),
        "set_title" => windows.set_title(
            None,
            rmp_serde::from_slice::<String>(&body).unwrap_or_default(),
        ),
        "set_size" => {
            let (w, h) = rmp_serde::from_slice::<(f64, f64)>(&body).unwrap_or((800.0, 600.0));
            windows.set_size(None, w, h)
        }
        other => return msgpack_err(format!("unknown window op: {other}")),
    };
    msgpack_ok(&ok)
}

/// Dispatch a `/__sys/<op>` native-system call. Args arrive as a MessagePack
/// body; results are MessagePack (errors surface via `x-elyra-status`).
#[cfg(feature = "system")]
pub(super) async fn serve_system(
    policy: &crate::security::Policy,
    op: &str,
    body: Vec<u8>,
) -> Body {
    use crate::system;
    match op {
        "dialog.open" => match rmp_serde::from_slice::<system::OpenDialog>(&body) {
            Ok(opt) => msgpack_ok(&system::open_dialog(opt).await),
            Err(e) => msgpack_err(e.to_string()),
        },
        "dialog.save" => match rmp_serde::from_slice::<system::SaveDialog>(&body) {
            Ok(opt) => msgpack_ok(&system::save_dialog(opt).await),
            Err(e) => msgpack_err(e.to_string()),
        },
        "shell.open" => match rmp_serde::from_slice::<String>(&body) {
            // Handing an arbitrary string to the OS default handler is an
            // execute-anything primitive; the policy decides what may pass.
            Ok(target) => match policy.allows_open(&target) {
                Ok(()) => match system::open_external(&target) {
                    Ok(()) => msgpack_ok(&()),
                    Err(e) => msgpack_err(e),
                },
                Err(e) => msgpack_err(e),
            },
            Err(e) => msgpack_err(e.to_string()),
        },
        "clipboard.read" => match system::clipboard_read() {
            Ok(text) => msgpack_ok(&text),
            Err(e) => msgpack_err(e),
        },
        "clipboard.write" => match rmp_serde::from_slice::<String>(&body) {
            Ok(text) => match system::clipboard_write(&text) {
                Ok(()) => msgpack_ok(&()),
                Err(e) => msgpack_err(e),
            },
            Err(e) => msgpack_err(e.to_string()),
        },
        "notify" => match rmp_serde::from_slice::<system::Notification>(&body) {
            Ok(n) => match system::notify(n) {
                Ok(()) => msgpack_ok(&()),
                Err(e) => msgpack_err(e),
            },
            Err(e) => msgpack_err(e.to_string()),
        },
        "paths" => msgpack_ok(&system::paths()),
        other => msgpack_err(format!("unknown system op: {other}")),
    }
}

#[cfg(feature = "sidecar")]
#[derive(serde::Deserialize)]
struct SidecarSpawn {
    program: String,
    #[serde(default)]
    args: Vec<String>,
}

#[cfg(feature = "sidecar")]
#[derive(serde::Deserialize)]
struct SidecarWrite {
    id: u32,
    data: String,
}

/// `POST /__sidecar/<op>` — spawn / write / kill sidecar processes.
#[cfg(feature = "sidecar")]
pub(super) fn serve_sidecar(runner: &Runner, op: &str, body: Vec<u8>) -> Body {
    let Some(sc) = runner.ctx.try_get::<crate::sidecar::Sidecar>() else {
        return msgpack_err("sidecar unavailable".into());
    };
    match op {
        "spawn" => match rmp_serde::from_slice::<SidecarSpawn>(&body) {
            // Default deny: the frontend may only spawn programs the app named
            // via `App::sidecar_allow`. Rust-side `Sidecar::spawn` is unrestricted.
            Ok(a) => match runner.policy.allows_sidecar(&a.program) {
                Ok(()) => match sc.spawn(&a.program, &a.args) {
                    Ok(id) => msgpack_ok(&id),
                    Err(e) => msgpack_err(e),
                },
                Err(e) => msgpack_err(e),
            },
            Err(e) => msgpack_err(e.to_string()),
        },
        "write" => match rmp_serde::from_slice::<SidecarWrite>(&body) {
            Ok(a) => msgpack_ok(&sc.write(a.id, a.data.into_bytes())),
            Err(e) => msgpack_err(e.to_string()),
        },
        "kill" => match rmp_serde::from_slice::<u32>(&body) {
            Ok(id) => msgpack_ok(&sc.kill(id)),
            Err(e) => msgpack_err(e.to_string()),
        },
        other => msgpack_err(format!("unknown sidecar op: {other}")),
    }
}

/// `POST /__autostart/<op>` — launch-at-login control.
#[cfg(feature = "autostart")]
pub(super) fn serve_autostart(runner: &Runner, op: &str) -> Body {
    let app = &runner.about.name;
    match op {
        "enable" => match crate::autostart::enable(app) {
            Ok(()) => msgpack_ok(&()),
            Err(e) => msgpack_err(e),
        },
        "disable" => match crate::autostart::disable(app) {
            Ok(()) => msgpack_ok(&()),
            Err(e) => msgpack_err(e),
        },
        "status" => match crate::autostart::is_enabled(app) {
            Ok(enabled) => msgpack_ok(&enabled),
            Err(e) => msgpack_err(e),
        },
        other => msgpack_err(format!("unknown autostart op: {other}")),
    }
}
