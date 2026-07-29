//! Wire-level guards for untrusted IPC bodies.
//!
//! Everything the frontend posts to `/__*` is attacker-controlled if a single
//! script in the webview is. Two cheap, structural limits are applied before any
//! `serde` work happens:
//!
//! * **Size** — a body larger than the configured maximum is refused with `413`
//!   instead of being buffered and decoded (`App::max_request_body`).
//! * **Depth** — MessagePack nests, and `serde`'s derived `Deserialize` recurses
//!   per level, so a *small* body (`[[[[…]]]]`, one byte per level) can overflow
//!   the stack and abort the whole process. [`check_depth`] walks the buffer
//!   iteratively — no recursion — and rejects anything nested deeper than
//!   [`MAX_DEPTH`].
//!
//! The walk only reads structure markers and skips payload bytes, so it's a
//! single linear pass over the body with no allocation.

/// Default maximum request body (16 MiB). Overridable per app.
pub const DEFAULT_MAX_BODY: usize = 16 * 1024 * 1024;

/// Maximum MessagePack nesting depth accepted from the frontend. Real payloads
/// (arguments, JSON-ish settings) are a handful of levels deep; 64 is generous
/// while staying far below what would exhaust the stack in `serde`.
pub const MAX_DEPTH: usize = 64;

/// Why a body was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum WireError {
    /// The body is larger than the configured limit.
    TooLarge { size: usize, limit: usize },
    /// The body nests deeper than [`MAX_DEPTH`].
    TooDeep { limit: usize },
    /// The body isn't valid MessagePack framing (truncated / bad marker).
    Malformed,
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WireError::TooLarge { size, limit } => {
                write!(f, "request body too large: {size} bytes (limit {limit})")
            }
            WireError::TooDeep { limit } => {
                write!(f, "request body nests deeper than {limit} levels")
            }
            WireError::Malformed => write!(f, "request body is not valid MessagePack"),
        }
    }
}

/// Check a body against both limits before decoding it.
pub fn check(body: &[u8], max_body: usize) -> Result<(), WireError> {
    if body.len() > max_body {
        return Err(WireError::TooLarge {
            size: body.len(),
            limit: max_body,
        });
    }
    if body.is_empty() {
        return Ok(()); // zero-arg commands send no body at all
    }
    check_depth(body, MAX_DEPTH)
}

/// Walk `body`'s MessagePack framing iteratively, failing if it nests deeper
/// than `max_depth`.
pub fn check_depth(body: &[u8], max_depth: usize) -> Result<(), WireError> {
    // Each stack entry is the number of *values* still expected at that level.
    let mut stack: Vec<u64> = Vec::new();
    let mut pos = 0usize;
    let mut first = true;

    loop {
        // Close out any finished containers.
        while let Some(remaining) = stack.last_mut() {
            if *remaining == 0 {
                stack.pop();
            } else {
                *remaining -= 1;
                break;
            }
        }
        if !first && stack.is_empty() {
            return Ok(()); // the top-level value is complete
        }
        first = false;

        let marker = *body.get(pos).ok_or(WireError::Malformed)?;
        pos += 1;

        // (extra bytes to skip, child values pushed)
        let (skip, children) = match marker {
            // fixint, nil, bool
            0x00..=0x7f | 0xe0..=0xff | 0xc0 | 0xc2 | 0xc3 => (0, 0),
            0xc4 => (len_of(body, pos, 1)? + 1, 0), // bin8
            0xc5 => (len_of(body, pos, 2)? + 2, 0), // bin16
            0xc6 => (len_of(body, pos, 4)? + 4, 0), // bin32
            0xc7 => (len_of(body, pos, 1)? + 2, 0), // ext8  (+type byte)
            0xc8 => (len_of(body, pos, 2)? + 3, 0), // ext16
            0xc9 => (len_of(body, pos, 4)? + 5, 0), // ext32
            0xca => (4, 0),                         // f32
            0xcb => (8, 0),                         // f64
            0xcc | 0xd0 => (1, 0),                  // u8 / i8
            0xcd | 0xd1 => (2, 0),                  // u16 / i16
            0xce | 0xd2 => (4, 0),                  // u32 / i32
            0xcf | 0xd3 => (8, 0),                  // u64 / i64
            0xd4 => (2, 0),                         // fixext1
            0xd5 => (3, 0),                         // fixext2
            0xd6 => (5, 0),                         // fixext4
            0xd7 => (9, 0),                         // fixext8
            0xd8 => (17, 0),                        // fixext16
            0xd9 => (len_of(body, pos, 1)? + 1, 0), // str8
            0xda => (len_of(body, pos, 2)? + 2, 0), // str16
            0xdb => (len_of(body, pos, 4)? + 4, 0), // str32
            0xa0..=0xbf => ((marker & 0x1f) as usize, 0), // fixstr
            0x90..=0x9f => (0, (marker & 0x0f) as u64), // fixarray
            0xdc => (2, len_of(body, pos, 2)? as u64), // array16
            0xdd => (4, len_of(body, pos, 4)? as u64), // array32
            0x80..=0x8f => (0, (marker & 0x0f) as u64 * 2), // fixmap
            0xde => (2, len_of(body, pos, 2)? as u64 * 2), // map16
            0xdf => (4, len_of(body, pos, 4)? as u64 * 2), // map32
            0xc1 => return Err(WireError::Malformed), // never used
        };

        pos = pos.checked_add(skip).ok_or(WireError::Malformed)?;
        if pos > body.len() {
            return Err(WireError::Malformed);
        }

        if children > 0 {
            if stack.len() >= max_depth {
                return Err(WireError::TooDeep { limit: max_depth });
            }
            // A declared length can't exceed the bytes left (one byte minimum per
            // element), so a bogus header can't make us spin.
            if children > (body.len() - pos) as u64 {
                return Err(WireError::Malformed);
            }
            stack.push(children);
        } else if stack.is_empty() {
            return Ok(()); // scalar top-level value
        }
    }
}

/// Read a big-endian length of `n` bytes at `pos`.
fn len_of(body: &[u8], pos: usize, n: usize) -> Result<usize, WireError> {
    let bytes = body.get(pos..pos + n).ok_or(WireError::Malformed)?;
    Ok(bytes.iter().fold(0usize, |acc, b| (acc << 8) | *b as usize))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_typical_command_arguments() {
        let body = rmp_serde::to_vec(&("hello", 42u32, vec![1u8, 2, 3])).unwrap();
        assert_eq!(check(&body, DEFAULT_MAX_BODY), Ok(()));

        let named = rmp_serde::to_vec_named(&serde_json::json!({
            "key": "value",
            "nested": { "list": [1, 2, 3], "flag": true, "f": 1.5 }
        }))
        .unwrap();
        assert_eq!(check(&named, DEFAULT_MAX_BODY), Ok(()));
    }

    #[test]
    fn accepts_an_empty_body() {
        assert_eq!(check(&[], DEFAULT_MAX_BODY), Ok(()));
    }

    #[test]
    fn rejects_a_body_over_the_limit() {
        let body = vec![0u8; 32];
        assert_eq!(
            check(&body, 16),
            Err(WireError::TooLarge {
                size: 32,
                limit: 16
            })
        );
    }

    #[test]
    fn rejects_deep_nesting() {
        // 10_000 nested one-element arrays: ~10 KB, but enough recursion in serde
        // to blow the stack. Structure is valid, so only the depth check catches it.
        let mut body = vec![0x91u8; 10_000];
        body.push(0xc0); // nil at the bottom
        assert_eq!(
            check_depth(&body, MAX_DEPTH),
            Err(WireError::TooDeep { limit: MAX_DEPTH })
        );
        assert!(matches!(
            check(&body, DEFAULT_MAX_BODY),
            Err(WireError::TooDeep { .. })
        ));
    }

    #[test]
    fn accepts_nesting_up_to_the_limit() {
        let mut body = vec![0x91u8; MAX_DEPTH - 1];
        body.push(0xc0);
        assert_eq!(check_depth(&body, MAX_DEPTH), Ok(()));
    }

    #[test]
    fn rejects_truncated_and_lying_headers() {
        assert_eq!(
            check_depth(&[0x93, 0x01], MAX_DEPTH),
            Err(WireError::Malformed)
        );
        assert_eq!(check_depth(&[0xd9], MAX_DEPTH), Err(WireError::Malformed));
        // str8 claiming 200 bytes in a 3-byte body.
        assert_eq!(
            check_depth(&[0xd9, 0xc8, 0x41], MAX_DEPTH),
            Err(WireError::Malformed)
        );
        // array32 claiming 4 billion elements.
        assert_eq!(
            check_depth(&[0xdd, 0xff, 0xff, 0xff, 0xff], MAX_DEPTH),
            Err(WireError::Malformed)
        );
        assert_eq!(check_depth(&[0xc1], MAX_DEPTH), Err(WireError::Malformed));
    }

    #[test]
    fn walks_maps_and_mixed_structures() {
        let value = serde_json::json!([
            {"a": [1, 2, {"b": "c"}]},
            null,
            [[[1]]],
            "trailing"
        ]);
        let body = rmp_serde::to_vec_named(&value).unwrap();
        assert_eq!(check_depth(&body, MAX_DEPTH), Ok(()));
        assert_eq!(check_depth(&body, 3), Err(WireError::TooDeep { limit: 3 }));
    }
}
