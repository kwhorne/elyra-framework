//! A window's picture for a coding agent (RFC 0026), from the webview wry
//! made: `takeSnapshot` on macOS, `CapturePreview` on Windows,
//! `get_snapshot` on Linux. Each gives a PNG at the screen's pixels; it's
//! scaled down here to the CSS pixels asked for, so a picture costs the
//! agent what its size in the page says. Debug builds only.

use tao::window::Window;
use tokio::sync::oneshot;
use wry::WebView;

use crate::dev::Picture;

type Reply = oneshot::Sender<Result<Picture, String>>;

/// Take `window`'s picture, at most `max_width` CSS pixels wide, and answer
/// `reply` when the platform has it (on the event loop, later).
pub(super) fn take(window: &Window, webview: &WebView, max_width: f64, reply: Reply) {
    if !window.is_visible() {
        let _ = reply.send(Err(
            "the window is hidden: a hidden window has no picture (show it first)".into(),
        ));
        return;
    }
    let scale = window.scale_factor();
    let css_width = window.inner_size().to_logical::<f64>(scale).width;
    let width = css_width.min(max_width).max(1.0);
    native(webview, width, scale, reply);
}

/// The picture `png` (at the screen's pixels) at `width` CSS pixels wide.
pub(super) fn scaled(png: &[u8], width: f64) -> Result<Picture, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("the picture isn't a PNG: {e}"))?;
    let mut pixels = vec![
        0;
        reader
            .output_buffer_size()
            .ok_or("the picture is too large")?
    ];
    let info = reader
        .next_frame(&mut pixels)
        .map_err(|e| format!("the picture can't be read: {e}"))?;
    pixels.truncate(info.buffer_size());
    let channels = info.color_type.samples();
    // Whole pixels per CSS pixel (2 on a Retina screen), averaged down.
    let factor = ((f64::from(info.width) / width).round() as u32).max(1);
    let (out_w, out_h) = (info.width / factor, info.height / factor);
    let out = if factor == 1 {
        pixels
    } else {
        let mut out = vec![0u8; (out_w * out_h) as usize * channels];
        let row = info.line_size;
        for y in 0..out_h as usize {
            for x in 0..out_w as usize {
                for c in 0..channels {
                    let mut sum = 0u32;
                    for dy in 0..factor as usize {
                        for dx in 0..factor as usize {
                            let at = (y * factor as usize + dy) * row
                                + (x * factor as usize + dx) * channels
                                + c;
                            sum += u32::from(pixels[at]);
                        }
                    }
                    out[(y * out_w as usize + x) * channels + c] = (sum / (factor * factor)) as u8;
                }
            }
        }
        out
    };
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, out_w, out_h);
        encoder.set_color(info.color_type);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&out).map_err(|e| e.to_string())?;
    }
    Ok(Picture {
        png,
        width: out_w,
        height: out_h,
    })
}

/// One reply, from a callback the platform may call more than once.
fn once(reply: Reply) -> impl Fn(Result<Picture, String>) + Send + Sync + 'static {
    let reply = parking_lot::Mutex::new(Some(reply));
    move |picture| {
        if let Some(reply) = reply.lock().take() {
            let _ = reply.send(picture);
        }
    }
}

#[cfg(target_os = "macos")]
fn native(webview: &WebView, width: f64, _scale: f64, reply: Reply) {
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
    use objc2_foundation::{NSDictionary, NSError, NSNumber};
    use objc2_web_kit::WKSnapshotConfiguration;
    use wry::WebViewExtMacOS;

    let answer = once(reply);
    let Some(main) = objc2::MainThreadMarker::new() else {
        return answer(Err("not on the main thread".into()));
    };
    let view = webview.webview();
    // SAFETY: on the main thread (the event loop's), with live objects.
    unsafe {
        let config = WKSnapshotConfiguration::new(main);
        config.setSnapshotWidth(Some(&NSNumber::new_f64(width)));
        let done = RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
            let picture = (|| {
                if image.is_null() {
                    let why = if error.is_null() {
                        "no picture".to_owned()
                    } else {
                        (*error).localizedDescription().to_string()
                    };
                    return Err(format!("the webview gave no picture: {why}"));
                }
                let tiff = (*image)
                    .TIFFRepresentation()
                    .ok_or("the picture has no bitmap")?;
                let rep: Retained<NSBitmapImageRep> =
                    NSBitmapImageRep::imageRepWithData(&tiff).ok_or("the picture has no bitmap")?;
                let data = rep
                    .representationUsingType_properties(
                        NSBitmapImageFileType::PNG,
                        &NSDictionary::new(),
                    )
                    .ok_or("the picture can't be a PNG")?;
                scaled(&data.to_vec(), width)
            })();
            answer(picture);
        });
        view.takeSnapshotWithConfiguration_completionHandler(Some(&config), &done);
    }
}

#[cfg(target_os = "windows")]
fn native(webview: &WebView, width: f64, _scale: f64, reply: Reply) {
    use webview2_com::CapturePreviewCompletedHandler;
    use webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG;
    use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
    use windows::Win32::System::Com::{IStream, STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET};
    use wry::WebViewExtWindows;

    let answer = std::sync::Arc::new(once(reply));
    let core = webview.webview();
    // SAFETY: COM calls on the UI thread, with the webview alive; the stream
    // is ours until the handler has read it.
    unsafe {
        let stream: IStream = match CreateStreamOnHGlobal(Default::default(), true) {
            Ok(stream) => stream,
            Err(e) => return answer(Err(format!("no stream for the picture: {e}"))),
        };
        let read_from = stream.clone();
        let done = answer.clone();
        let handler = CapturePreviewCompletedHandler::create(Box::new(move |result| {
            let picture = (|| {
                result.map_err(|e| format!("the webview gave no picture: {e}"))?;
                let mut stat = STATSTG::default();
                read_from
                    .Stat(&mut stat, STATFLAG_NONAME)
                    .map_err(|e| e.to_string())?;
                read_from
                    .Seek(0, STREAM_SEEK_SET, None)
                    .map_err(|e| e.to_string())?;
                let mut png = vec![0u8; stat.cbSize as usize];
                let mut read = 0u32;
                read_from
                    .Read(png.as_mut_ptr().cast(), png.len() as u32, Some(&mut read))
                    .ok()
                    .map_err(|e| e.to_string())?;
                png.truncate(read as usize);
                scaled(&png, width)
            })();
            done(picture);
            Ok(())
        }));
        if let Err(e) = core.CapturePreview(
            COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
            &stream,
            &handler,
        ) {
            answer(Err(format!("the webview can't take a picture: {e}")));
        }
    }
}

#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn native(webview: &WebView, width: f64, _scale: f64, reply: Reply) {
    use webkit2gtk::{SnapshotOptions, SnapshotRegion, WebViewExt};
    use wry::WebViewExtUnix;

    let answer = once(reply);
    let view = webview.webview();
    view.snapshot(
        SnapshotRegion::Visible,
        SnapshotOptions::NONE,
        None::<&gio::Cancellable>,
        move |result| {
            let picture = (|| {
                let surface = result.map_err(|e| format!("the webview gave no picture: {e}"))?;
                let mut png = Vec::new();
                surface
                    .write_to_png(&mut png)
                    .map_err(|e| format!("the picture can't be a PNG: {e}"))?;
                scaled(&png, width)
            })();
            answer(picture);
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `w`×`h` RGBA PNG whose pixel (x, y) is `paint(x, y)`.
    fn png_of(w: u32, h: u32, paint: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let mut data = Vec::new();
        for y in 0..h {
            for x in 0..w {
                data.extend_from_slice(&paint(x, y));
            }
        }
        let mut png = Vec::new();
        let mut encoder = png::Encoder::new(&mut png, w, h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&data)
            .unwrap();
        png
    }

    fn pixels(picture: &Picture) -> Vec<u8> {
        let mut reader = png::Decoder::new(std::io::Cursor::new(&picture.png))
            .read_info()
            .unwrap();
        let mut out = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut out).unwrap();
        out.truncate(info.buffer_size());
        out
    }

    #[test]
    fn a_retina_picture_is_scaled_to_css_pixels_by_averaging() {
        // 4×2 screen pixels for 2×1 CSS pixels: each CSS pixel averages four.
        let png = png_of(4, 2, |x, _| {
            if x < 2 {
                [0, 0, 0, 255]
            } else {
                [200, 100, 50, 255]
            }
        });
        let picture = scaled(&png, 2.0).unwrap();
        assert_eq!((picture.width, picture.height), (2, 1));
        assert_eq!(pixels(&picture), vec![0, 0, 0, 255, 200, 100, 50, 255]);
        // A checkerboard averages to grey.
        let png = png_of(2, 2, |x, y| {
            if (x + y) % 2 == 0 {
                [0, 0, 0, 255]
            } else {
                [255, 255, 255, 255]
            }
        });
        assert_eq!(
            pixels(&scaled(&png, 1.0).unwrap()),
            vec![127, 127, 127, 255]
        );
    }

    #[test]
    fn a_picture_at_its_size_is_kept_and_a_wider_one_shrinks() {
        let png = png_of(3, 2, |x, y| [x as u8, y as u8, 0, 255]);
        let same = scaled(&png, 3.0).unwrap();
        assert_eq!((same.width, same.height), (3, 2));
        assert_eq!(
            pixels(&same),
            pixels(&Picture {
                png: png.clone(),
                width: 3,
                height: 2
            })
        );
        // Asked for at most 1280 of a 2560-pixel-wide page: halved.
        let wide = png_of(2560, 4, |_, _| [9, 9, 9, 255]);
        assert_eq!(scaled(&wide, 1280.0).unwrap().width, 1280);
    }

    #[test]
    fn something_that_isnt_a_png_is_said() {
        assert!(scaled(b"GIF89a", 10.0).unwrap_err().contains("isn't a PNG"));
    }
}
