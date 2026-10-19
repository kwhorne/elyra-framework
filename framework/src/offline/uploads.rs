//! Uploads in the outbox (RFC 0009 step 4). An upload marked
//! `.queue_offline()` that gets no answer keeps a copy of its files in the
//! app's data folder, `<folder>/<owner>/<key>/`, so what's sent later is what
//! the user attached, even if the original is moved, changed or deleted. The
//! outbox row's body is a manifest of the parts, pointing to the copies; the
//! copies go when the write is sent or discarded.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::http::{Part, PartBody};

/// The manifest's one key: no JSON body a Laravel API takes has it.
const KEY: &str = "elyra:multipart";

/// An upload's parts, as the outbox keeps them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Manifest {
    #[serde(rename = "elyra:multipart")]
    pub parts: Vec<Kept>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub(crate) enum Kept {
    Text {
        name: String,
        text: String,
    },
    File {
        name: String,
        /// The copy.
        file: PathBuf,
        file_name: Option<String>,
        content_type: Option<String>,
    },
}

/// The manifest in an outbox row's body, if it's an upload.
pub(crate) fn manifest(body: Option<&str>) -> Option<Manifest> {
    let body = body?;
    if !body.contains(KEY) {
        return None;
    }
    serde_json::from_str(body).ok()
}

/// Copy an upload's files into `folder/owner/key/` and describe it. Refused
/// when the copies would take the user's queued uploads over `limit` bytes.
pub(crate) async fn keep(
    folder: &Path,
    owner: &str,
    key: &str,
    parts: &[Part],
    limit: u64,
) -> Result<String, String> {
    let mine = folder.join(owner);
    let mut adding = 0u64;
    for part in parts {
        adding += match &part.body {
            PartBody::File(path) => tokio::fs::metadata(path)
                .await
                .map(|m| m.len())
                .map_err(|e| format!("{}: {e}", file_name(path)))?,
            PartBody::Bytes(bytes) => bytes.len() as u64,
            PartBody::Text(_) => 0,
        };
    }
    let kept = size_of(&mine).await;
    if kept + adding > limit {
        return Err(format!(
            "the files waiting to be sent would take more than {} MB",
            limit / (1024 * 1024)
        ));
    }
    let here = mine.join(key);
    tokio::fs::create_dir_all(&here)
        .await
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        let copy = |name: &Option<String>| {
            here.join(format!(
                "{i}-{}",
                crate::http::clean_file_name(name.as_deref().unwrap_or("file"))
            ))
        };
        match &part.body {
            PartBody::Text(text) => out.push(Kept::Text {
                name: part.name.clone(),
                text: text.clone(),
            }),
            PartBody::File(path) => {
                let file = copy(&part.file_name);
                if let Err(e) = tokio::fs::copy(path, &file).await {
                    let _ = tokio::fs::remove_dir_all(&here).await;
                    return Err(format!("{}: {e}", file_name(path)));
                }
                out.push(Kept::File {
                    name: part.name.clone(),
                    file,
                    file_name: part.file_name.clone(),
                    content_type: part.content_type.clone(),
                });
            }
            PartBody::Bytes(bytes) => {
                let file = copy(&part.file_name);
                if let Err(e) = tokio::fs::write(&file, bytes).await {
                    let _ = tokio::fs::remove_dir_all(&here).await;
                    return Err(e.to_string());
                }
                out.push(Kept::File {
                    name: part.name.clone(),
                    file,
                    file_name: part.file_name.clone(),
                    content_type: part.content_type.clone(),
                });
            }
        }
    }
    serde_json::to_string(&Manifest { parts: out }).map_err(|e| e.to_string())
}

/// Remove an upload's copies, once it's sent or discarded.
pub(crate) fn remove(body: Option<&str>) {
    let Some(manifest) = manifest(body) else {
        return;
    };
    let mut folders = Vec::new();
    for part in manifest.parts {
        if let Kept::File { file, .. } = part {
            let _ = std::fs::remove_file(&file);
            if let Some(parent) = file.parent() {
                folders.push(parent.to_owned());
            }
        }
    }
    for folder in folders {
        // Only once it's empty.
        let _ = std::fs::remove_dir(folder);
    }
}

/// Remove every copy a user's queued uploads kept.
pub(crate) fn remove_owner(folder: &Path, owner: &str) {
    let _ = std::fs::remove_dir_all(folder.join(owner));
}

/// How the frontend's outbox list shows an upload: its fields, and each
/// file by name.
pub(crate) fn shown(manifest: &Manifest) -> Value {
    let mut map = serde_json::Map::new();
    for part in &manifest.parts {
        match part {
            Kept::Text { name, text } if name != "_method" => {
                map.insert(name.clone(), Value::String(text.clone()));
            }
            Kept::File {
                name, file_name, ..
            } => {
                map.insert(
                    name.clone(),
                    serde_json::json!({ "file": file_name.clone().unwrap_or_default() }),
                );
            }
            Kept::Text { .. } => {}
        }
    }
    Value::Object(map)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "the file".into())
}

async fn size_of(dir: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![dir.to_owned()];
    while let Some(dir) = stack.pop() {
        let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
            continue;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            match entry.metadata().await {
                Ok(m) if m.is_dir() => stack.push(entry.path()),
                Ok(m) => total += m.len(),
                Err(_) => {}
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("elyra-uploads-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn parts(file: &Path) -> Vec<Part> {
        vec![
            Part::text("_method".into(), "PUT".into()),
            Part::file("contract".into(), file),
            Part {
                name: "thumb".into(),
                body: PartBody::Bytes(vec![1, 2, 3]),
                file_name: Some("../thumb.png".into()),
                content_type: Some("image/png".into()),
            },
            Part::text("note".into(), "Signed".into()),
        ]
    }

    #[tokio::test]
    async fn an_upload_is_kept_as_copies_and_a_manifest() {
        let dir = temp("keep");
        let original = dir.join("contract.pdf");
        std::fs::write(&original, "%PDF v1").unwrap();
        let folder = dir.join("outbox-files");
        let body = keep(&folder, "owner1", "key1", &parts(&original), 1 << 20)
            .await
            .unwrap();
        // The original can change or go: the copy is what's sent.
        std::fs::remove_file(&original).unwrap();
        let manifest = manifest(Some(&body)).expect("a manifest");
        let files: Vec<_> = manifest
            .parts
            .iter()
            .filter_map(|p| match p {
                Kept::File { file, .. } => Some(file.clone()),
                Kept::Text { .. } => None,
            })
            .collect();
        assert_eq!(files.len(), 2);
        assert_eq!(std::fs::read(&files[0]).unwrap(), b"%PDF v1");
        assert!(files
            .iter()
            .all(|f| f.starts_with(folder.join("owner1").join("key1"))));
        assert_eq!(
            shown(&manifest),
            serde_json::json!({ "contract": { "file": "contract.pdf" }, "thumb": { "file": "../thumb.png" }, "note": "Signed" })
        );

        remove(Some(&body));
        assert!(!folder.join("owner1").join("key1").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn the_limit_counts_what_already_waits() {
        let dir = temp("limit");
        let original = dir.join("a.bin");
        std::fs::write(&original, vec![0u8; 600]).unwrap();
        let folder = dir.join("outbox-files");
        let one = vec![Part::file("a".into(), &original)];
        keep(&folder, "o", "k1", &one, 1000).await.unwrap();
        let err = keep(&folder, "o", "k2", &one, 1000).await.unwrap_err();
        assert!(err.contains("more than"), "{err}");
        assert!(!folder.join("o").join("k2").exists());
        // Another user's limit is their own.
        keep(&folder, "someone-else", "k3", &one, 1000)
            .await
            .unwrap();
        remove_owner(&folder, "o");
        assert!(!folder.join("o").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_json_body_isnt_a_manifest() {
        assert_eq!(manifest(Some(r#"{"name":"Ada"}"#)), None);
        assert_eq!(manifest(None), None);
    }
}
