//! The file that starts the app, as the system should start it again: at
//! login (`autostart`), for a link (`App::deep_link`), and after an update.
//!
//! Usually the executable. Inside an AppImage the executable is in the
//! image's mount, a path that changes every run and vanishes when the app
//! quits; what starts the app is the AppImage itself, which its runtime
//! names in `$APPIMAGE`.

use std::ffi::OsStr;
use std::path::PathBuf;

/// The file to start the app with.
pub(crate) fn launcher() -> std::io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    Ok(launcher_of(
        exe,
        std::env::var_os("APPIMAGE").as_deref(),
        cfg!(target_os = "linux"),
    ))
}

/// [`launcher`], from what it reads: on Linux, a `$APPIMAGE` that names a
/// file by its full path; else the executable.
fn launcher_of(exe: PathBuf, appimage: Option<&OsStr>, linux: bool) -> PathBuf {
    match appimage.map(PathBuf::from) {
        Some(image) if linux && image.is_absolute() => image,
        _ => exe,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str = "/tmp/.mount_NotesAb12/usr/bin/notes";

    #[test]
    fn inside_an_appimage_it_is_the_appimage() {
        let image = OsStr::new("/home/ada/Applications/notes_1.2.0_x86_64.AppImage");
        assert_eq!(
            launcher_of(EXE.into(), Some(image), true),
            PathBuf::from(image)
        );
    }

    #[test]
    fn elsewhere_it_is_the_executable() {
        assert_eq!(launcher_of(EXE.into(), None, true), PathBuf::from(EXE));
        // Only Linux has AppImages; a stray variable elsewhere means nothing.
        let image = OsStr::new("/x/notes.AppImage");
        assert_eq!(
            launcher_of(EXE.into(), Some(image), false),
            PathBuf::from(EXE)
        );
        // Nor does a value that isn't a full path.
        for odd in ["", "notes.AppImage"] {
            assert_eq!(
                launcher_of(EXE.into(), Some(OsStr::new(odd)), true),
                PathBuf::from(EXE)
            );
        }
    }
}
