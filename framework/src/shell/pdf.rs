//! The window's page as a PDF, without a dialog (RFC 0062): what the print
//! dialog would print, laid out by the webview itself on pages of the size
//! asked for. Each platform writes it to a file of ours, read back and
//! removed: an `NSPrintOperation` that saves (macOS), WebView2's
//! `PrintToPdf` (Windows), WebKitGTK's print to the file printer (Linux).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use wry::WebView;

use crate::window::{PdfPage, PdfReply};

/// A page's size and margins, in millimetres, as the platform is told them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Sheet {
    /// Width and height upright; `landscape` turns it.
    pub width: f64,
    pub height: f64,
    pub landscape: bool,
    /// Top, right, bottom, left.
    pub margins: [f64; 4],
}

/// The page asked for, with what wasn't said filled in: A4, upright, 10 mm.
pub(crate) fn sheet(page: &PdfPage) -> Result<Sheet, String> {
    let (width, height) = match page.paper.as_deref().unwrap_or("a4") {
        "a4" => (210.0, 297.0),
        "letter" => (215.9, 279.4),
        other => return Err(format!("not a paper size: {other} (a4 or letter)")),
    };
    let m = page.margins.clone().unwrap_or_default();
    let margins = [m.top, m.right, m.bottom, m.left].map(|v| v.unwrap_or(10.0));
    if margins
        .iter()
        .any(|v| !v.is_finite() || *v < 0.0 || *v > 100.0)
    {
        return Err("margins are millimetres, from 0 to 100".into());
    }
    Ok(Sheet {
        width,
        height,
        landscape: page.landscape,
        margins,
    })
}

/// Where this PDF is written before it's read back.
fn scratch() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "elyra-{}-{}.pdf",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

/// What a platform calls when it's done writing `path` (or failed to).
type Done = Box<dyn FnOnce(Result<(), String>)>;

/// The PDF of `webview`'s page, answered on `reply` when the platform has
/// written it (on the event loop, later).
pub(super) fn make(webview: &WebView, page: &PdfPage, reply: PdfReply) {
    let sheet = match sheet(page) {
        Ok(sheet) => sheet,
        Err(e) => {
            let _ = reply.send(Err(e));
            return;
        }
    };
    let path = scratch();
    let read_from = path.clone();
    let done: Done = Box::new(move |written| {
        let pdf = written.and_then(|()| {
            std::fs::read(&read_from).map_err(|e| format!("the PDF wasn't written: {e}"))
        });
        let _ = std::fs::remove_file(&read_from);
        let _ = reply.send(pdf.and_then(|bytes| {
            if bytes.starts_with(b"%PDF") {
                Ok(bytes)
            } else {
                Err("the webview wrote no PDF".into())
            }
        }));
    });
    native(webview, sheet, path, done);
}

#[cfg(target_os = "macos")]
mod mac {
    use std::cell::RefCell;

    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool, NSObject};
    use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};

    use super::Done;

    pub(super) struct Ivars {
        done: RefCell<Option<Done>>,
    }

    define_class!(
        // The print operation's delegate: told when the PDF is saved.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "ElyraPdfSaved"]
        #[ivars = Ivars]
        pub(super) struct Saved;

        impl Saved {
            #[unsafe(method(printOperationDidRun:success:contextInfo:))]
            fn did_run(&self, _operation: *mut AnyObject, success: Bool, _info: *mut std::ffi::c_void) {
                if let Some(done) = self.ivars().done.borrow_mut().take() {
                    done(if success.as_bool() {
                        Ok(())
                    } else {
                        Err("the PDF couldn't be made".into())
                    });
                }
                // Kept alive until now: the operation doesn't keep its delegate.
                let this: *const Self = self;
                KEPT.with(|kept| kept.borrow_mut().retain(|k| !std::ptr::eq(&**k, this)));
            }
        }
    );

    thread_local! {
        static KEPT: RefCell<Vec<Retained<Saved>>> = const { RefCell::new(Vec::new()) };
    }

    impl Saved {
        pub(super) fn keep(main: MainThreadMarker, done: Done) -> Retained<Self> {
            let this = Self::alloc(main).set_ivars(Ivars {
                done: RefCell::new(Some(done)),
            });
            // SAFETY: NSObject's `init`, on a new instance of a subclass.
            let saved: Retained<Self> = unsafe { msg_send![super(this), init] };
            KEPT.with(|kept| kept.borrow_mut().push(saved.clone()));
            saved
        }
    }
}

#[cfg(target_os = "macos")]
fn native(webview: &WebView, sheet: Sheet, path: PathBuf, done: Done) {
    use objc2::runtime::{AnyObject, ProtocolObject};
    use objc2::sel;
    use objc2_app_kit::{
        NSPaperOrientation, NSPrintInfo, NSPrintJobSavingURL, NSPrintSaveJob,
        NSPrintingPaginationMode,
    };
    use objc2_foundation::{NSSize, NSString, NSURL};
    use wry::WebViewExtMacOS;

    let Some(main) = objc2::MainThreadMarker::new() else {
        return done(Err("not on the main thread".into()));
    };
    let view = webview.webview();
    let Some(window) = view.window() else {
        return done(Err("the webview isn't in a window".into()));
    };
    let (w, h) = if sheet.landscape {
        (sheet.height, sheet.width)
    } else {
        (sheet.width, sheet.height)
    };
    let pt = |mm: f64| mm * 72.0 / 25.4;
    let info = NSPrintInfo::new();
    info.setPaperSize(NSSize::new(pt(w), pt(h)));
    info.setOrientation(if sheet.landscape {
        NSPaperOrientation::Landscape
    } else {
        NSPaperOrientation::Portrait
    });
    let [top, right, bottom, left] = sheet.margins;
    info.setTopMargin(pt(top));
    info.setRightMargin(pt(right));
    info.setBottomMargin(pt(bottom));
    info.setLeftMargin(pt(left));
    info.setHorizontalPagination(NSPrintingPaginationMode::Fit);
    info.setVerticalPagination(NSPrintingPaginationMode::Automatic);
    // SAFETY: AppKit's own keys and values, on the main thread; the URL is a
    // file URL, which is what NSPrintJobSavingURL takes.
    unsafe {
        info.setJobDisposition(NSPrintSaveJob);
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        let url: &AnyObject = &url;
        info.dictionary()
            .setObject_forKey(url, ProtocolObject::from_ref(NSPrintJobSavingURL));
        let operation = view.printOperationWithPrintInfo(&info);
        operation.setShowsPrintPanel(false);
        operation.setShowsProgressPanel(false);
        // WebKit's printing view has no size of its own: without one, the
        // pages come out blank.
        if let Some(printing) = operation.view() {
            printing.setFrame(view.bounds());
        }
        let saved = mac::Saved::keep(main, done);
        operation.runOperationModalForWindow_delegate_didRunSelector_contextInfo(
            &window,
            Some(&saved),
            Some(sel!(printOperationDidRun:success:contextInfo:)),
            std::ptr::null_mut(),
        );
    }
}

#[cfg(target_os = "windows")]
fn native(webview: &WebView, sheet: Sheet, path: PathBuf, done: Done) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Environment6, ICoreWebView2_7, COREWEBVIEW2_PRINT_ORIENTATION_LANDSCAPE,
        COREWEBVIEW2_PRINT_ORIENTATION_PORTRAIT,
    };
    use webview2_com::PrintToPdfCompletedHandler;
    use windows::core::{Interface, HSTRING};
    use wry::WebViewExtWindows;

    let inches = |mm: f64| mm / 25.4;
    let done = std::cell::RefCell::new(Some(done));
    let finish = move |result: Result<(), String>| {
        if let Some(done) = done.borrow_mut().take() {
            done(result);
        }
    };
    let finish = std::rc::Rc::new(finish);
    let started = (|| -> windows::core::Result<()> {
        // SAFETY: COM calls on the UI thread, with the webview alive.
        unsafe {
            let core: ICoreWebView2_7 = webview.webview().cast()?;
            let environment: ICoreWebView2Environment6 = webview.environment().cast()?;
            let settings = environment.CreatePrintSettings()?;
            settings.SetOrientation(if sheet.landscape {
                COREWEBVIEW2_PRINT_ORIENTATION_LANDSCAPE
            } else {
                COREWEBVIEW2_PRINT_ORIENTATION_PORTRAIT
            })?;
            settings.SetPageWidth(inches(sheet.width))?;
            settings.SetPageHeight(inches(sheet.height))?;
            let [top, right, bottom, left] = sheet.margins;
            settings.SetMarginTop(inches(top))?;
            settings.SetMarginRight(inches(right))?;
            settings.SetMarginBottom(inches(bottom))?;
            settings.SetMarginLeft(inches(left))?;
            // As the other two: backgrounds only where the page asks
            // (`print-color-adjust: exact`).
            settings.SetShouldPrintBackgrounds(false)?;
            let after = finish.clone();
            let handler = PrintToPdfCompletedHandler::create(Box::new(move |result, ok| {
                after(match (result, ok) {
                    (Ok(()), true) => Ok(()),
                    (Err(e), _) => Err(format!("the PDF couldn't be made: {e}")),
                    (Ok(()), false) => Err("the PDF couldn't be made".into()),
                });
                Ok(())
            }));
            core.PrintToPdf(&HSTRING::from(path.as_os_str()), &settings, &handler)
        }
    })();
    if let Err(e) = started {
        finish(Err(format!("the webview can't make a PDF: {e}")));
    }
}

#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn native(webview: &WebView, sheet: Sheet, path: PathBuf, done: Done) {
    use gtk::{PageOrientation, PageSetup, PaperSize, PrintSettings, Unit};
    use webkit2gtk::{PrintOperation, PrintOperationExt};
    use wry::WebViewExtUnix;

    let Some(printer) = linux::file_printer() else {
        return done(Err("GTK has no Print to File printer here".into()));
    };
    let settings = PrintSettings::new();
    settings.set_printer(&printer);
    settings.set("output-file-format", Some("pdf"));
    let uri = gio::File::for_path(&path).uri();
    settings.set("output-uri", Some(uri.as_str()));

    let setup = PageSetup::new();
    let paper = PaperSize::new_custom("elyra", "elyra", sheet.width, sheet.height, Unit::Mm);
    setup.set_paper_size(&paper);
    setup.set_orientation(if sheet.landscape {
        PageOrientation::Landscape
    } else {
        PageOrientation::Portrait
    });
    let [top, right, bottom, left] = sheet.margins;
    setup.set_top_margin(top, Unit::Mm);
    setup.set_right_margin(right, Unit::Mm);
    setup.set_bottom_margin(bottom, Unit::Mm);
    setup.set_left_margin(left, Unit::Mm);

    let operation = PrintOperation::new(&webview.webview());
    operation.set_print_settings(&settings);
    operation.set_page_setup(&setup);
    let done = std::rc::Rc::new(std::cell::RefCell::new(Some(done)));
    let failed = done.clone();
    operation.connect_failed(move |_, error| {
        if let Some(done) = failed.borrow_mut().take() {
            done(Err(format!("the PDF couldn't be made: {error}")));
        }
    });
    operation.connect_finished(move |_| {
        if let Some(done) = done.borrow_mut().take() {
            done(Ok(()));
        }
    });
    operation.print();
}

/// GTK's file printer has a name in the user's language ("Print to File"),
/// so it's found by its backend, as WebKitGTK's own tests find it.
#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
mod linux {
    use std::ffi::{c_char, c_void, CStr};

    use gtk::glib::ffi::{gboolean, GFALSE, GTRUE};
    use gtk::glib::gobject_ffi::{g_type_name_from_instance, GTypeInstance};

    extern "C" {
        fn gtk_enumerate_printers(
            func: unsafe extern "C" fn(*mut c_void, *mut c_void) -> gboolean,
            data: *mut c_void,
            destroy: Option<unsafe extern "C" fn(*mut c_void)>,
            wait: gboolean,
        );
        fn gtk_printer_get_name(printer: *mut c_void) -> *const c_char;
        fn gtk_printer_get_backend(printer: *mut c_void) -> *mut c_void;
    }

    unsafe extern "C" fn found(printer: *mut c_void, data: *mut c_void) -> gboolean {
        // SAFETY: GTK hands a live printer, and `data` is our `Option<String>`.
        unsafe {
            let backend = gtk_printer_get_backend(printer);
            if backend.is_null() {
                return GFALSE;
            }
            let kind = g_type_name_from_instance(backend as *mut GTypeInstance);
            if kind.is_null() || CStr::from_ptr(kind).to_bytes() != b"GtkPrintBackendFile" {
                return GFALSE;
            }
            let name = gtk_printer_get_name(printer);
            if name.is_null() {
                return GFALSE;
            }
            *(data as *mut Option<String>) =
                Some(CStr::from_ptr(name).to_string_lossy().into_owned());
            GTRUE
        }
    }

    pub(super) fn file_printer() -> Option<String> {
        let mut name: Option<String> = None;
        // SAFETY: `wait` is true, so GTK is done with `name` when this returns.
        unsafe {
            gtk_enumerate_printers(
                found,
                (&mut name as *mut Option<String>).cast(),
                None,
                GTRUE,
            );
        }
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::PdfMargins;

    #[test]
    fn a_sheet_is_a4_upright_with_ten_millimetres_unless_asked() {
        let plain = sheet(&PdfPage::default()).unwrap();
        assert_eq!(
            plain,
            Sheet {
                width: 210.0,
                height: 297.0,
                landscape: false,
                margins: [10.0; 4]
            }
        );
        let asked = sheet(&PdfPage {
            paper: Some("letter".into()),
            landscape: true,
            margins: Some(PdfMargins {
                top: Some(20.0),
                left: Some(0.0),
                ..Default::default()
            }),
        })
        .unwrap();
        assert_eq!(
            (asked.width, asked.height, asked.landscape),
            (215.9, 279.4, true)
        );
        assert_eq!(asked.margins, [20.0, 10.0, 10.0, 0.0]);
    }

    #[test]
    fn a_sheet_refuses_what_isnt_paper() {
        assert!(sheet(&PdfPage {
            paper: Some("a3".into()),
            ..Default::default()
        })
        .unwrap_err()
        .contains("a4 or letter"));
        assert!(sheet(&PdfPage {
            margins: Some(PdfMargins {
                top: Some(-1.0),
                ..Default::default()
            }),
            ..Default::default()
        })
        .is_err());
    }

    #[test]
    fn each_pdf_has_a_file_of_its_own() {
        assert_ne!(scratch(), scratch());
    }
}
