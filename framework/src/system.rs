//! Native system integration (behind the `system` feature).
//!
//! The desktop capabilities almost every app needs: native **file dialogs**,
//! **opening** URLs/files in the OS, the **clipboard**, OS **notifications**,
//! and standard **paths**. The shell exposes these at `/__sys/*` and
//! `@elyra/runtime` wraps them as `dialog`, `shell`, `clipboard`, `notify`, and
//! `paths`.
//!
//! File dialogs use `rfd`'s async API (which marshals to the platform's main
//! thread internally), so they're safe to call from Elyra's tokio-driven IPC.

use serde::{Deserialize, Serialize};

/// A name + extensions pair for a file-dialog filter.
#[derive(Debug, Deserialize)]
pub struct Filter {
    pub name: String,
    #[serde(default)]
    pub extensions: Vec<String>,
}

/// Options for an open dialog (file or folder, single or multiple).
#[derive(Debug, Default, Deserialize)]
pub struct OpenDialog {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub directory: bool,
    #[serde(default)]
    pub multiple: bool,
    #[serde(default)]
    pub filters: Vec<Filter>,
    #[serde(default)]
    pub start_dir: Option<String>,
}

/// Options for a save dialog.
#[derive(Debug, Default, Deserialize)]
pub struct SaveDialog {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub default_name: Option<String>,
    #[serde(default)]
    pub filters: Vec<Filter>,
    #[serde(default)]
    pub start_dir: Option<String>,
}

/// A desktop notification.
#[derive(Debug, Deserialize)]
pub struct Notification {
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
}

/// Standard OS directories + the running executable, as strings.
#[derive(Debug, Serialize)]
pub struct Paths {
    pub home: Option<String>,
    pub config: Option<String>,
    pub data: Option<String>,
    pub cache: Option<String>,
    /// The user's Downloads folder.
    pub downloads: Option<String>,
    pub temp: Option<String>,
    pub exe: Option<String>,
}

fn base(
    mut d: rfd::AsyncFileDialog,
    title: &Option<String>,
    start_dir: &Option<String>,
) -> rfd::AsyncFileDialog {
    if let Some(t) = title {
        d = d.set_title(t.as_str());
    }
    if let Some(s) = start_dir {
        d = d.set_directory(s.as_str());
    }
    d
}

fn with_filters(mut d: rfd::AsyncFileDialog, filters: &[Filter]) -> rfd::AsyncFileDialog {
    for f in filters {
        d = d.add_filter(f.name.as_str(), &f.extensions);
    }
    d
}

/// Show an open dialog. Returns the selected paths (empty if cancelled).
pub async fn open_dialog(opt: OpenDialog) -> Vec<String> {
    let d = with_filters(
        base(rfd::AsyncFileDialog::new(), &opt.title, &opt.start_dir),
        &opt.filters,
    );
    let to_str = |h: &rfd::FileHandle| h.path().display().to_string();
    match (opt.directory, opt.multiple) {
        (true, true) => d
            .pick_folders()
            .await
            .map(|v| v.iter().map(to_str).collect())
            .unwrap_or_default(),
        (true, false) => d
            .pick_folder()
            .await
            .map(|h| vec![to_str(&h)])
            .unwrap_or_default(),
        (false, true) => d
            .pick_files()
            .await
            .map(|v| v.iter().map(to_str).collect())
            .unwrap_or_default(),
        (false, false) => d
            .pick_file()
            .await
            .map(|h| vec![to_str(&h)])
            .unwrap_or_default(),
    }
}

/// Show a save dialog. Returns the chosen path, or `None` if cancelled.
pub async fn save_dialog(opt: SaveDialog) -> Option<String> {
    let mut d = with_filters(
        base(rfd::AsyncFileDialog::new(), &opt.title, &opt.start_dir),
        &opt.filters,
    );
    if let Some(name) = &opt.default_name {
        d = d.set_file_name(name.as_str());
    }
    d.save_file().await.map(|h| h.path().display().to_string())
}

/// Open a URL or path with the OS default handler.
pub fn open_external(target: &str) -> Result<(), String> {
    open::that(target).map_err(|e| e.to_string())
}

/// Read the clipboard's text contents.
pub fn clipboard_read() -> Result<String, String> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.get_text())
        .map_err(|e| e.to_string())
}

/// Replace the clipboard's text contents.
pub fn clipboard_write(text: &str) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(text.to_owned()))
        .map_err(|e| e.to_string())
}

/// Put a PNG on the clipboard as an image (RFC 0060), for Word, Keynote or
/// Mail to paste.
pub fn clipboard_write_image(png: &[u8]) -> Result<(), String> {
    let (width, height, rgba) = decode_png(png)?;
    arboard::Clipboard::new()
        .and_then(|mut c| {
            c.set_image(arboard::ImageData {
                width,
                height,
                bytes: rgba.into(),
            })
        })
        .map_err(|e| e.to_string())
}

/// A PNG as 8-bit RGBA, whatever its colour type.
pub(crate) fn decode_png(png: &[u8]) -> Result<(usize, usize, Vec<u8>), String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| format!("not a PNG: {e}"))?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("a PNG too large")?];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("not a PNG: {e}"))?;
    let pixels = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => pixels.to_vec(),
        png::ColorType::Rgb => pixels
            .chunks(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => pixels
            .chunks(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => pixels.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err("an indexed PNG wasn't expanded".into()),
    };
    Ok((info.width as usize, info.height as usize, rgba))
}

/// Bytes as the page sends them: a MessagePack `bin`, or a list of numbers.
pub(crate) struct Bytes(pub Vec<u8>);

impl<'de> serde::Deserialize<'de> for Bytes {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Bytes;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("bytes")
            }
            fn visit_bytes<E>(self, v: &[u8]) -> Result<Bytes, E> {
                Ok(Bytes(v.to_vec()))
            }
            fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Bytes, E> {
                Ok(Bytes(v))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Bytes, A::Error> {
                let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0));
                while let Some(b) = seq.next_element::<u8>()? {
                    out.push(b);
                }
                Ok(Bytes(out))
            }
        }
        deserializer.deserialize_byte_buf(Visitor)
    }
}

/// `files.save`'s write (RFC 0060): the grant's id and what to write.
#[derive(serde::Deserialize)]
pub(crate) struct FileWrite {
    pub id: String,
    pub bytes: Bytes,
}

/// Show an OS notification. (On macOS, delivery requires a bundled app.)
/// In an app, the container's [`Notifier`](crate::notifier::Notifier) does the
/// same and can be faked in a test.
pub fn notify(n: Notification) -> Result<(), String> {
    crate::notifier::Notifier::system().show(
        crate::notifier::Shown {
            title: n.title,
            body: n.body,
        },
        None,
    )
}

/// Standard OS directories + the running executable.
pub fn paths() -> Paths {
    let s = |p: std::path::PathBuf| p.display().to_string();
    Paths {
        home: dirs::home_dir().map(s),
        config: dirs::config_dir().map(s),
        data: dirs::data_dir().map(s),
        cache: dirs::cache_dir().map(s),
        downloads: dirs::download_dir().map(s),
        temp: Some(std::env::temp_dir().display().to_string()),
        exe: std::env::current_exe().ok().map(s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_write_arrives_with_its_bytes_as_msgpack_bin() {
        struct Bin<'a>(&'a [u8]);
        impl serde::Serialize for Bin<'_> {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_bytes(self.0)
            }
        }
        #[derive(serde::Serialize)]
        struct Sent<'a> {
            id: &'a str,
            bytes: Bin<'a>,
        }
        let body = rmp_serde::to_vec_named(&Sent {
            id: "g1",
            bytes: Bin(&[137, 80, 78, 71]),
        })
        .unwrap();
        let write: FileWrite = rmp_serde::from_slice(&body).unwrap();
        assert_eq!(write.id, "g1");
        assert_eq!(write.bytes.0, [137, 80, 78, 71]);
    }

    #[test]
    fn a_png_is_decoded_to_rgba_whatever_its_colours() {
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, 2, 1);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[255, 0, 0, 0, 0, 255]).unwrap();
        }
        let (width, height, rgba) = decode_png(&png).unwrap();
        assert_eq!((width, height), (2, 1));
        assert_eq!(rgba, [255, 0, 0, 255, 0, 0, 255, 255]);
        assert!(decode_png(b"not a png").unwrap_err().contains("not a PNG"));
    }

    #[test]
    fn paths_reports_a_temp_dir() {
        // A pure, non-interactive smoke test: temp is always available.
        assert!(paths().temp.is_some());
    }
}
