//! Replacing a whole macOS `.app` (RFC 0024).
//!
//! A Developer ID signature seals the bundle: `Info.plist`, the resources and
//! every executable under `Contents/`. Writing a new executable into it breaks
//! the seal, and macOS kills a process whose signed executable is overwritten
//! under it. So an update is a zip of the whole stapled bundle: it is expanded
//! into a private directory, its signature checked the way Gatekeeper checks
//! one, the installed bundle moved aside, the new one moved in by a rename,
//! and the old one removed. Relaunching is a separate step.
//!
//! Everything that asks the system (`ditto`, `codesign`, extended attributes,
//! renames, starting the new copy) is behind [`BundleSystem`], so the decisions
//! here are unit-tested with a fake; [`NativeSystem`] is the real one.

use std::path::{Component, Path, PathBuf};

use super::Error;

/// The Developer ID an update must be signed with: a certificate Apple issued
/// to `team_id`, over `bundle_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeveloperId {
    /// The team's ten-character identifier (`52AT44QFCM`), as in
    /// `codesign -dv`'s `TeamIdentifier=`.
    pub team_id: String,
    /// The app's `CFBundleIdentifier` (`works.felagi.desktop`).
    pub bundle_id: String,
}

impl DeveloperId {
    pub fn new(team_id: impl Into<String>, bundle_id: impl Into<String>) -> Self {
        Self {
            team_id: team_id.into(),
            bundle_id: bundle_id.into(),
        }
    }

    /// Whether both are plausible, so neither can change the meaning of the
    /// [`requirement`](Self::requirement) they are written into.
    pub fn check(&self) -> Result<(), String> {
        let team_ok = !self.team_id.is_empty()
            && self.team_id.len() <= 20
            && self.team_id.chars().all(|c| c.is_ascii_alphanumeric());
        if !team_ok {
            return Err(format!("`{}` isn't a team identifier", self.team_id));
        }
        let bundle_ok = !self.bundle_id.is_empty()
            && self.bundle_id.len() <= 255
            && self
                .bundle_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
        if !bundle_ok {
            return Err(format!("`{}` isn't a bundle identifier", self.bundle_id));
        }
        Ok(())
    }

    /// The code requirement Apple writes for a Developer ID application,
    /// narrowed to this team and identifier: anchored at Apple's root, a
    /// Developer ID intermediate, a Developer ID Application leaf issued to
    /// the team, over the identifier.
    pub fn requirement(&self) -> String {
        format!(
            "anchor apple generic \
             and certificate 1[field.1.2.840.113635.100.6.2.6] \
             and certificate leaf[field.1.2.840.113635.100.6.1.13] \
             and certificate leaf[subject.OU] = \"{}\" \
             and identifier \"{}\"",
            self.team_id, self.bundle_id
        )
    }
}

/// What the system says about a bundle's code signature.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodeSignature {
    /// Signed at all (an ad-hoc signature counts).
    pub signed: bool,
    /// Why `codesign --verify --strict --deep` refused it, or `None` when it
    /// passed: the bundle is unaltered since it was signed.
    pub invalid: Option<String>,
    /// The signature's identifier (`Identifier=`).
    pub identifier: Option<String>,
    /// The signing team (`TeamIdentifier=`), `None` for an ad-hoc signature.
    pub team_id: Option<String>,
    /// It satisfies [`DeveloperId::requirement`] for the expected team and
    /// identifier: the certificate chain is Apple's Developer ID one.
    pub developer_id: bool,
}

/// What a bundle's `Info.plist` says.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BundleInfo {
    pub identifier: Option<String>,
    /// `CFBundleShortVersionString`.
    pub version: Option<String>,
}

/// Everything the bundle update asks of the system. [`NativeSystem`] does it for
/// real; a test passes a fake.
pub trait BundleSystem: Send + Sync {
    /// Expand the zip at `zip` into the directory `into` (`ditto -x -k`).
    fn expand(&self, zip: &Path, into: &Path) -> Result<(), String>;
    /// Check the signature of the bundle at `app`; with `expected`, also
    /// whether it satisfies that Developer ID's requirement.
    fn code_signature(&self, app: &Path, expected: Option<&DeveloperId>) -> CodeSignature;
    /// Read the bundle's `Info.plist`.
    fn bundle_info(&self, app: &Path) -> BundleInfo;
    /// Remove the quarantine attribute from the bundle and everything in it.
    fn clear_quarantine(&self, app: &Path) -> Result<(), String>;
    /// Whether this process may create and rename entries in `dir`.
    fn can_write(&self, dir: &Path) -> bool;
    /// Move `from` to `to`: a rename, or a copy and a removal across volumes.
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()>;
    /// Remove a directory tree, if it's there.
    fn remove(&self, path: &Path);
    /// Start `app` (a bundle, or a loose executable) once this process has
    /// exited. Returns once that's arranged; the caller then quits.
    fn relaunch(&self, app: &Path) -> Result<(), String>;
}

/// Why the running app can't be replaced where it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotInstalled {
    /// Started from a cargo build (`target/debug`, `target/release`).
    FromBuild,
    /// Not inside a `.app` bundle.
    NotABundle,
    /// macOS runs it from a randomized read-only copy (App Translocation),
    /// because it was opened where it was downloaded.
    Translocated,
}

impl NotInstalled {
    pub fn message(self) -> &'static str {
        match self {
            NotInstalled::FromBuild => {
                "this app was started from a build, not from an installed copy, \
                 so there is nothing to replace"
            }
            NotInstalled::NotABundle => {
                "this app isn't running from a .app bundle, so there is nothing to replace"
            }
            NotInstalled::Translocated => {
                "macOS is running this app from a temporary, read-only copy; \
                 move it to Applications and open it from there"
            }
        }
    }
}

/// The `.app` the running executable `exe` belongs to, when it is an
/// installed one: `Foo.app/Contents/MacOS/foo`, not translocated, not in a
/// cargo build directory.
pub fn installed_bundle(exe: &Path) -> Result<PathBuf, NotInstalled> {
    if exe
        .components()
        .any(|c| c.as_os_str() == "AppTranslocation")
    {
        return Err(NotInstalled::Translocated);
    }
    let Some(app) = bundle_root(exe) else {
        return Err(if from_build(exe) {
            NotInstalled::FromBuild
        } else {
            NotInstalled::NotABundle
        });
    };
    if from_build(&app) {
        return Err(NotInstalled::FromBuild);
    }
    Ok(app)
}

/// The `.app` bundle the executable belongs to, if any: a macOS bundle always
/// lays out as `Foo.app/Contents/MacOS/foo`.
pub fn bundle_root(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    if macos.file_name()? != "MacOS" {
        return None;
    }
    let contents = macos.parent()?;
    if contents.file_name()? != "Contents" {
        return None;
    }
    let app = contents.parent()?;
    if app.extension()? != "app" {
        return None;
    }
    Some(app.to_path_buf())
}

/// Whether `path` is inside a cargo target directory: `target/debug`,
/// `target/release`, or the same under a target triple.
pub fn from_build(path: &Path) -> bool {
    let parts: Vec<&std::ffi::OsStr> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s),
            _ => None,
        })
        .collect();
    let profile =
        |s: Option<&&std::ffi::OsStr>| s.is_some_and(|s| *s == "debug" || *s == "release");
    parts.iter().enumerate().any(|(i, part)| {
        *part == "target" && (profile(parts.get(i + 1)) || profile(parts.get(i + 2)))
    })
}

/// Decide whether a signature is the expected Developer ID's: signed, intact,
/// by that team, over that identifier, with Apple's Developer ID chain. The
/// reason, in a sentence, when it isn't.
pub fn judge(sig: &CodeSignature, expected: &DeveloperId) -> Result<(), String> {
    if !sig.signed {
        return Err("the update is not signed".into());
    }
    if let Some(why) = &sig.invalid {
        return Err(format!(
            "the update's signature does not match its contents ({why})"
        ));
    }
    match sig.team_id.as_deref() {
        None => {
            return Err("the update is not signed with a Developer ID".into());
        }
        Some(team) if team != expected.team_id => {
            return Err(format!(
                "the update is signed by team {team}, not {}",
                expected.team_id
            ));
        }
        Some(_) => {}
    }
    match sig.identifier.as_deref() {
        Some(id) if id == expected.bundle_id => {}
        other => {
            return Err(format!(
                "the update is a different application ({}, expected {})",
                other.unwrap_or("no identifier"),
                expected.bundle_id
            ));
        }
    }
    if !sig.developer_id {
        return Err("the update's certificate is not an Apple-issued Developer ID".into());
    }
    Ok(())
}

/// Without a Developer ID configured: signed (ad hoc will do) and intact. The
/// ed25519 signature over the archive is what says it's the app's.
pub fn judge_intact(sig: &CodeSignature) -> Result<(), String> {
    if !sig.signed {
        return Err("the update is not signed".into());
    }
    if let Some(why) = &sig.invalid {
        return Err(format!(
            "the update's code signature does not verify ({why})"
        ));
    }
    Ok(())
}

/// What an installed bundle has to pass before it replaces the running one.
pub struct BundleTrust<'a> {
    /// Require this Developer ID; without one, a signed, intact bundle with
    /// the installed bundle's identifier.
    pub developer_id: Option<&'a DeveloperId>,
    /// Refuse a bundle whose version isn't newer than this.
    pub current: Option<&'a semver::Version>,
}

/// Local zip files start with the `PK\x03\x04` local-file-header magic.
pub(crate) fn is_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06")
}

/// Expand the zip at `zip` into `work` (a fresh private directory), check the
/// bundle in it, and put it where `installed` is. Nothing is moved unless
/// every check passed; `installed` is untouched on any error. `work` is the
/// caller's to remove.
pub fn install_bundle(
    sys: &dyn BundleSystem,
    zip: &Path,
    installed: &Path,
    work: &Path,
    trust: &BundleTrust<'_>,
) -> Result<(), Error> {
    let mut magic = [0u8; 4];
    {
        use std::io::Read;
        let mut f = std::fs::File::open(zip).map_err(|e| Error::Io(e.to_string()))?;
        let _ = f.read(&mut magic).map_err(|e| Error::Io(e.to_string()))?;
    }
    if !is_zip(&magic) {
        // Applying a bare binary here would break the bundle's signature and
        // leave an app macOS refuses to open. Refuse instead of bricking it.
        return Err(Error::Io(
            "update artifact is not a .app zip; refusing to modify a signed bundle".into(),
        ));
    }

    sys.expand(zip, work)
        .map_err(|why| Error::Io(format!("could not expand the update archive: {why}")))?;
    let fresh = only_app(work)?;

    let sig = sys.code_signature(&fresh, trust.developer_id);
    let verdict = match trust.developer_id {
        Some(expected) => judge(&sig, expected),
        None => judge_intact(&sig),
    };
    verdict.map_err(Error::CodeSignature)?;

    let new_info = sys.bundle_info(&fresh);
    if let (Some(a), Some(b)) = (sys.bundle_info(installed).identifier, &new_info.identifier) {
        if &a != b {
            return Err(Error::CodeSignature(format!(
                "the update is a different application ({b}, expected {a})"
            )));
        }
    }
    if let (Some(current), Some(found)) = (trust.current, &new_info.version) {
        if let Ok(version) = semver::Version::parse(found.trim()) {
            if &version <= current {
                return Err(Error::NotNewer {
                    found: found.trim().to_owned(),
                    current: current.to_string(),
                });
            }
        }
    }

    // Verified as Gatekeeper would: the "downloaded from the internet"
    // question on first launch would only ask about what was just checked.
    if let Err(why) = sys.clear_quarantine(&fresh) {
        crate::debug!(target: "elyra::updater", "couldn't clear the quarantine: {why}");
    }
    swap(sys, &fresh, installed)
}

/// The one `.app` directly inside `dir`.
fn only_app(dir: &Path) -> Result<PathBuf, Error> {
    let apps: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| Error::Io(e.to_string()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.extension().is_some_and(|x| x == "app"))
        .collect();
    match apps.as_slice() {
        [one] => Ok(one.clone()),
        [] => Err(Error::Io("update archive contains no .app bundle".into())),
        _ => Err(Error::Io(
            "update archive contains more than one .app bundle".into(),
        )),
    }
}

/// The name the outgoing bundle is moved to, beside it: hidden, and not
/// ending in `.app`, so nothing treats it as an application meanwhile.
fn outgoing_name(installed: &Path) -> Result<PathBuf, Error> {
    let parent = installed
        .parent()
        .ok_or_else(|| Error::Io("bundle has no parent directory".into()))?;
    let name = installed
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Error::Io("bundle has no name".into()))?;
    Ok(parent.join(format!(".{name}.old-{}", std::process::id())))
}

/// Put `fresh` where `installed` is. The old bundle is moved aside, not
/// written over: this process runs from it, and a rename leaves the running
/// executable's file untouched. If the new one can't move in, the old one is
/// put back.
pub fn swap(sys: &dyn BundleSystem, fresh: &Path, installed: &Path) -> Result<(), Error> {
    let outgoing = outgoing_name(installed)?;
    sys.remove(&outgoing);
    sys.rename(installed, &outgoing).map_err(|e| {
        Error::NotInstallable(format!(
            "could not move the current app aside: {}",
            plain_io(&e)
        ))
    })?;
    if let Err(e) = sys.rename(fresh, installed) {
        if let Err(back) = sys.rename(&outgoing, installed) {
            crate::error!(
                target: "elyra::updater",
                "couldn't put the old app back at {}: {back}",
                installed.display()
            );
        }
        return Err(Error::NotInstallable(format!(
            "could not move the update into place: {}",
            plain_io(&e)
        )));
    }
    sys.remove(&outgoing);
    Ok(())
}

/// Remove what an interrupted swap left beside `installed` (`.Foo.app.old-*`).
pub fn sweep_leftovers(sys: &dyn BundleSystem, installed: &Path) {
    let (Some(parent), Some(name)) = (
        installed.parent(),
        installed.file_name().and_then(|n| n.to_str()),
    ) else {
        return;
    };
    let prefix = format!(".{name}.old-");
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            sys.remove(&entry.path());
        }
    }
}

fn plain_io(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::PermissionDenied => "permission denied".into(),
        _ => e.to_string(),
    }
}

/// The real system: Apple's `ditto` and `codesign`, extended attributes, and
/// `open`. Only reached on macOS; elsewhere its calls fail.
#[derive(Debug, Clone, Copy, Default)]
pub struct NativeSystem;

impl BundleSystem for NativeSystem {
    fn expand(&self, zip: &Path, into: &Path) -> Result<(), String> {
        // `ditto` is Apple's own tool: it keeps symlinks, modes and extended
        // attributes, which a bundle's signature depends on.
        let out = std::process::Command::new("/usr/bin/ditto")
            .args(["-x", "-k"])
            .arg(zip)
            .arg(into)
            .output()
            .map_err(|e| format!("could not run ditto: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
        }
    }

    fn code_signature(&self, app: &Path, expected: Option<&DeveloperId>) -> CodeSignature {
        // Note: `codesign` has no `--quiet` flag; passing one makes it exit 2
        // and every update would look unsigned.
        let verify = std::process::Command::new("/usr/bin/codesign")
            .args(["--verify", "--strict", "--deep"])
            .arg(app)
            .output();
        let (signed, invalid) = match verify {
            Ok(out) if out.status.success() => (true, None),
            Ok(out) => {
                let reason = String::from_utf8_lossy(&out.stderr).trim().to_owned();
                let unsigned = reason.contains("not signed at all");
                let reason = if reason.is_empty() {
                    format!("codesign exited with {}", out.status)
                } else {
                    reason
                };
                (!unsigned, (!unsigned).then_some(reason))
            }
            Err(e) => (true, Some(format!("could not run codesign: {e}"))),
        };
        let mut sig = CodeSignature {
            signed,
            invalid,
            ..CodeSignature::default()
        };
        if !signed {
            return sig;
        }
        if let Ok(out) = std::process::Command::new("/usr/bin/codesign")
            .args(["-d", "--verbose=2"])
            .arg(app)
            .output()
        {
            // `codesign -d` writes what it found to stderr.
            let text = String::from_utf8_lossy(&out.stderr);
            sig.identifier = field(&text, "Identifier=");
            sig.team_id = field(&text, "TeamIdentifier=").filter(|t| t != "not set");
        }
        if let Some(expected) = expected {
            sig.developer_id = std::process::Command::new("/usr/bin/codesign")
                .args(["--verify", "--strict", "--deep"])
                .arg(format!("--test-requirement=={}", expected.requirement()))
                .arg(app)
                .output()
                .is_ok_and(|out| out.status.success());
        }
        sig
    }

    fn bundle_info(&self, app: &Path) -> BundleInfo {
        let read = |key: &str| {
            let out = std::process::Command::new("/usr/libexec/PlistBuddy")
                .args(["-c", &format!("Print :{key}")])
                .arg(app.join("Contents/Info.plist"))
                .output()
                .ok()?;
            if !out.status.success() {
                return None;
            }
            let value = String::from_utf8_lossy(&out.stdout).trim().to_owned();
            (!value.is_empty()).then_some(value)
        };
        BundleInfo {
            identifier: read("CFBundleIdentifier"),
            version: read("CFBundleShortVersionString"),
        }
    }

    fn clear_quarantine(&self, app: &Path) -> Result<(), String> {
        clear_quarantine(app)
    }

    fn can_write(&self, dir: &Path) -> bool {
        let probe = dir.join(format!(
            ".elyra-update-probe-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe)
        {
            Ok(_) => {
                let _ = std::fs::remove_file(&probe);
                true
            }
            Err(_) => false,
        }
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        match std::fs::rename(from, to) {
            Ok(()) => Ok(()),
            // Another volume: copy with ditto (symlinks, modes, attributes
            // kept), then remove the original.
            Err(e) if e.raw_os_error() == Some(EXDEV) => {
                let status = std::process::Command::new("/usr/bin/ditto")
                    .arg(from)
                    .arg(to)
                    .status()?;
                if !status.success() {
                    let _ = std::fs::remove_dir_all(to);
                    return Err(std::io::Error::other("ditto could not copy it"));
                }
                let _ = std::fs::remove_dir_all(from);
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    fn remove(&self, path: &Path) {
        let _ = std::fs::remove_dir_all(path);
    }

    fn relaunch(&self, app: &Path) -> Result<(), String> {
        relaunch_after_exit(app)
    }
}

/// `EXDEV`, "cross-device link", on macOS and Linux.
const EXDEV: i32 = 18;

/// The value after `key` on its own line of `codesign -d` output.
fn field(text: &str, key: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.strip_prefix(key))
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

/// Start `target` once this process has exited: a single-instance app would
/// otherwise hand the new copy's launch to this one, and quit both. A bundle
/// is started with `open -n`, which registers it with LaunchServices; a loose
/// executable is started directly. Gives up waiting after a minute.
pub(crate) fn relaunch_after_exit(target: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        let is_app = target.extension().is_some_and(|x| x == "app");
        // The pid and the path are arguments, never part of the script.
        let start = if is_app {
            r#"exec /usr/bin/open -n "$2""#
        } else {
            r#"exec "$2""#
        };
        let script = format!(
            r#"i=0; while kill -0 "$1" 2>/dev/null && [ $i -lt 600 ]; do sleep 0.1; i=$((i+1)); done; {start}"#
        );
        std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(script)
            .arg("sh")
            .arg(std::process::id().to_string())
            .arg(target)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("could not start the new version: {e}"))
    }
    #[cfg(not(unix))]
    {
        std::process::Command::new(target)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("could not start the new version: {e}"))
    }
}

/// Remove `com.apple.quarantine` from `path` and everything under it.
#[cfg(target_os = "macos")]
fn clear_quarantine(path: &Path) -> Result<(), String> {
    use std::ffi::{c_char, c_int, CString};
    use std::os::unix::ffi::OsStrExt;
    extern "C" {
        fn removexattr(path: *const c_char, name: *const c_char, options: c_int) -> c_int;
    }
    const XATTR_NOFOLLOW: c_int = 0x0001;
    const ENOATTR: i32 = 93;

    let mut stack = vec![path.to_path_buf()];
    while let Some(p) = stack.pop() {
        let c = CString::new(p.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
        // SAFETY: both strings are NUL-terminated and live across the call.
        let done =
            unsafe { removexattr(c.as_ptr(), c"com.apple.quarantine".as_ptr(), XATTR_NOFOLLOW) };
        if done != 0 {
            let e = std::io::Error::last_os_error();
            if e.raw_os_error() != Some(ENOATTR) {
                return Err(format!("{}: {e}", p.display()));
            }
        }
        let meta = std::fs::symlink_metadata(&p).map_err(|e| e.to_string())?;
        if meta.is_dir() {
            for entry in std::fs::read_dir(&p).map_err(|e| e.to_string())?.flatten() {
                stack.push(entry.path());
            }
        }
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn clear_quarantine(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn team() -> DeveloperId {
        DeveloperId::new("52AT44QFCM", "works.felagi.desktop")
    }

    fn good() -> CodeSignature {
        CodeSignature {
            signed: true,
            invalid: None,
            identifier: Some("works.felagi.desktop".into()),
            team_id: Some("52AT44QFCM".into()),
            developer_id: true,
        }
    }

    #[test]
    fn a_developer_id_signature_from_the_team_over_the_app_is_accepted() {
        assert_eq!(judge(&good(), &team()), Ok(()));
    }

    #[test]
    fn an_unsigned_bundle_is_refused() {
        let sig = CodeSignature::default();
        assert_eq!(judge(&sig, &team()), Err("the update is not signed".into()));
        assert!(judge_intact(&sig).is_err());
    }

    #[test]
    fn another_team_is_refused() {
        let sig = CodeSignature {
            team_id: Some("7G383N3VY7".into()),
            developer_id: false,
            ..good()
        };
        assert_eq!(
            judge(&sig, &team()),
            Err("the update is signed by team 7G383N3VY7, not 52AT44QFCM".into())
        );
    }

    #[test]
    fn another_bundle_id_is_refused() {
        let sig = CodeSignature {
            identifier: Some("dev.eterm.eterm".into()),
            developer_id: false,
            ..good()
        };
        let err = judge(&sig, &team()).unwrap_err();
        assert!(err.contains("different application"), "{err}");
        assert!(err.contains("dev.eterm.eterm"), "{err}");
    }

    #[test]
    fn an_ad_hoc_or_self_made_certificate_is_refused() {
        // Ad hoc: no team at all.
        let adhoc = CodeSignature {
            team_id: None,
            developer_id: false,
            ..good()
        };
        assert_eq!(
            judge(&adhoc, &team()),
            Err("the update is not signed with a Developer ID".into())
        );
        // A certificate that names the team but isn't Apple's Developer ID
        // chain: the requirement check is what catches it.
        let forged = CodeSignature {
            developer_id: false,
            ..good()
        };
        assert!(judge(&forged, &team())
            .unwrap_err()
            .contains("Apple-issued Developer ID"));
    }

    #[test]
    fn an_altered_bundle_is_refused() {
        let sig = CodeSignature {
            invalid: Some("a sealed resource is missing or invalid".into()),
            ..good()
        };
        assert!(judge(&sig, &team())
            .unwrap_err()
            .contains("does not match its contents"));
        assert!(judge_intact(&sig).is_err());
    }

    #[test]
    fn identifiers_that_could_rewrite_the_requirement_are_refused() {
        assert!(team().check().is_ok());
        assert!(DeveloperId::new("52AT\" or true", "x").check().is_err());
        assert!(DeveloperId::new("52AT44QFCM", "a\" or identifier \"b")
            .check()
            .is_err());
        assert!(DeveloperId::new("", "x").check().is_err());
        let req = team().requirement();
        assert!(req.contains("certificate leaf[subject.OU] = \"52AT44QFCM\""));
        assert!(req.contains("identifier \"works.felagi.desktop\""));
        assert!(req.starts_with("anchor apple generic"));
    }

    #[test]
    fn the_installed_bundle_is_found_from_the_executable() {
        assert_eq!(
            installed_bundle(Path::new("/Applications/Félagi.app/Contents/MacOS/felagi")),
            Ok(PathBuf::from("/Applications/Félagi.app"))
        );
        assert_eq!(
            installed_bundle(Path::new(
                "/Users/ada/Applications/Félagi.app/Contents/MacOS/felagi"
            )),
            Ok(PathBuf::from("/Users/ada/Applications/Félagi.app"))
        );
    }

    #[test]
    fn running_from_a_build_is_refused() {
        for exe in [
            "/Users/ada/felagi/target/debug/felagi-desktop",
            "/Users/ada/felagi/target/release/felagi-desktop",
            "/Users/ada/felagi/target/aarch64-apple-darwin/release/felagi-desktop",
            "/Users/ada/felagi/target/release/bundle/Félagi.app/Contents/MacOS/felagi",
        ] {
            assert_eq!(
                installed_bundle(Path::new(exe)),
                Err(NotInstalled::FromBuild),
                "{exe}"
            );
        }
        assert_eq!(
            installed_bundle(Path::new("/usr/local/bin/felagi")),
            Err(NotInstalled::NotABundle)
        );
        assert_eq!(
            installed_bundle(Path::new("/tmp/Foo.bundle/Contents/MacOS/foo")),
            Err(NotInstalled::NotABundle)
        );
        // A folder that only happens to be called `target` isn't a build.
        assert!(!from_build(Path::new(
            "/Users/ada/target/Félagi.app/Contents/MacOS/felagi"
        )));
    }

    #[test]
    fn a_translocated_app_is_refused() {
        assert_eq!(
            installed_bundle(Path::new(
                "/private/var/folders/xy/T/AppTranslocation/1234-ABCD/d/Félagi.app/Contents/MacOS/felagi"
            )),
            Err(NotInstalled::Translocated)
        );
    }

    /// A fake system: `expand` lays out the bundle it's told to, the
    /// signature is whatever the test says, renames are real but can be made
    /// to fail.
    struct Fake {
        expands_to: Vec<(&'static str, &'static str, &'static str)>,
        signature: CodeSignature,
        fail_rename_into: bool,
        quarantine_cleared: Mutex<Vec<PathBuf>>,
    }

    impl Fake {
        fn new(signature: CodeSignature) -> Self {
            Self {
                expands_to: vec![("Félagi.app", "works.felagi.desktop", "2.0.0")],
                signature,
                fail_rename_into: false,
                quarantine_cleared: Mutex::new(Vec::new()),
            }
        }
    }

    fn write_app(dir: &Path, name: &str, identifier: &str, version: &str) -> PathBuf {
        let app = dir.join(name);
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        std::fs::write(
            app.join("Contents/Info.plist"),
            format!("{identifier}\n{version}"),
        )
        .unwrap();
        app
    }

    impl BundleSystem for Fake {
        fn expand(&self, _zip: &Path, into: &Path) -> Result<(), String> {
            for (name, id, version) in &self.expands_to {
                write_app(into, name, id, version);
            }
            Ok(())
        }
        fn code_signature(&self, _app: &Path, _expected: Option<&DeveloperId>) -> CodeSignature {
            self.signature.clone()
        }
        fn bundle_info(&self, app: &Path) -> BundleInfo {
            let text = std::fs::read_to_string(app.join("Contents/Info.plist")).unwrap_or_default();
            let mut lines = text.lines().map(str::to_owned);
            BundleInfo {
                identifier: lines.next(),
                version: lines.next(),
            }
        }
        fn clear_quarantine(&self, app: &Path) -> Result<(), String> {
            self.quarantine_cleared.lock().unwrap().push(app.to_owned());
            Ok(())
        }
        fn can_write(&self, _dir: &Path) -> bool {
            true
        }
        fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
            // The new bundle, moving in from the work directory.
            if self.fail_rename_into && from.components().any(|c| c.as_os_str() == "work") {
                return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
            }
            std::fs::rename(from, to)
        }
        fn remove(&self, path: &Path) {
            let _ = std::fs::remove_dir_all(path);
        }
        fn relaunch(&self, _app: &Path) -> Result<(), String> {
            Ok(())
        }
    }

    struct Scene {
        root: PathBuf,
        installed: PathBuf,
        zip: PathBuf,
        work: PathBuf,
    }

    impl Drop for Scene {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn scene(tag: &str) -> Scene {
        let root = super::super::temp_dir(&format!("elyra-bundle-{tag}")).unwrap();
        let apps = root.join("Applications");
        std::fs::create_dir_all(&apps).unwrap();
        let installed = write_app(&apps, "Félagi.app", "works.felagi.desktop", "1.0.0");
        let zip = root.join("update.zip");
        std::fs::write(&zip, b"PK\x03\x04 a zip, as far as the magic goes").unwrap();
        let work = root.join("work");
        std::fs::create_dir_all(&work).unwrap();
        Scene {
            root,
            installed,
            zip,
            work,
        }
    }

    fn installed_version(s: &Scene) -> String {
        std::fs::read_to_string(s.installed.join("Contents/Info.plist"))
            .unwrap()
            .lines()
            .nth(1)
            .unwrap()
            .to_owned()
    }

    fn trust<'a>(id: &'a DeveloperId, current: &'a semver::Version) -> BundleTrust<'a> {
        BundleTrust {
            developer_id: Some(id),
            current: Some(current),
        }
    }

    #[test]
    fn a_verified_bundle_replaces_the_installed_one_and_the_old_one_goes() {
        let s = scene("ok");
        let sys = Fake::new(good());
        let (id, v) = (team(), semver::Version::new(1, 0, 0));
        install_bundle(&sys, &s.zip, &s.installed, &s.work, &trust(&id, &v)).unwrap();
        assert_eq!(installed_version(&s), "2.0.0");
        assert_eq!(sys.quarantine_cleared.lock().unwrap().len(), 1);
        let leftovers: Vec<_> = std::fs::read_dir(s.installed.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(leftovers, vec![std::ffi::OsString::from("Félagi.app")]);
    }

    #[test]
    fn a_refused_signature_leaves_the_installed_bundle_alone() {
        for sig in [
            CodeSignature::default(),
            CodeSignature {
                team_id: Some("7G383N3VY7".into()),
                ..good()
            },
            CodeSignature {
                identifier: Some("dev.eterm.eterm".into()),
                ..good()
            },
        ] {
            let s = scene("refused");
            let sys = Fake::new(sig);
            let (id, v) = (team(), semver::Version::new(1, 0, 0));
            let err =
                install_bundle(&sys, &s.zip, &s.installed, &s.work, &trust(&id, &v)).unwrap_err();
            assert!(matches!(err, Error::CodeSignature(_)), "{err}");
            assert_eq!(installed_version(&s), "1.0.0");
            assert!(sys.quarantine_cleared.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn a_bare_binary_is_never_applied_to_a_bundle() {
        let s = scene("bare");
        std::fs::write(&s.zip, [0xcf, 0xfa, 0xed, 0xfe]).unwrap();
        let sys = Fake::new(good());
        let (id, v) = (team(), semver::Version::new(1, 0, 0));
        let err = install_bundle(&sys, &s.zip, &s.installed, &s.work, &trust(&id, &v)).unwrap_err();
        assert!(err.to_string().contains("not a .app zip"), "{err}");
        assert_eq!(installed_version(&s), "1.0.0");
    }

    #[test]
    fn an_archive_with_two_apps_or_none_is_refused() {
        let s = scene("two");
        let mut sys = Fake::new(good());
        sys.expands_to = vec![
            ("Félagi.app", "works.felagi.desktop", "2.0.0"),
            ("Other.app", "works.felagi.desktop", "2.0.0"),
        ];
        let (id, v) = (team(), semver::Version::new(1, 0, 0));
        assert!(
            install_bundle(&sys, &s.zip, &s.installed, &s.work, &trust(&id, &v))
                .unwrap_err()
                .to_string()
                .contains("more than one")
        );
        let s = scene("none");
        sys.expands_to = vec![];
        assert!(
            install_bundle(&sys, &s.zip, &s.installed, &s.work, &trust(&id, &v))
                .unwrap_err()
                .to_string()
                .contains("no .app")
        );
    }

    #[test]
    fn an_older_or_same_version_is_refused() {
        let s = scene("old");
        let mut sys = Fake::new(good());
        sys.expands_to = vec![("Félagi.app", "works.felagi.desktop", "1.0.0")];
        let (id, v) = (team(), semver::Version::new(1, 0, 0));
        let err = install_bundle(&sys, &s.zip, &s.installed, &s.work, &trust(&id, &v)).unwrap_err();
        assert!(matches!(err, Error::NotNewer { .. }), "{err}");
        assert_eq!(installed_version(&s), "1.0.0");
    }

    #[test]
    fn without_a_developer_id_the_identifier_must_match_the_installed_one() {
        let s = scene("legacy");
        let mut sys = Fake::new(CodeSignature {
            team_id: None,
            developer_id: false,
            ..good()
        });
        sys.expands_to = vec![("Félagi.app", "com.example.intruder", "2.0.0")];
        let trust = BundleTrust {
            developer_id: None,
            current: None,
        };
        let err = install_bundle(&sys, &s.zip, &s.installed, &s.work, &trust).unwrap_err();
        assert!(err.to_string().contains("different application"), "{err}");
    }

    #[test]
    fn the_old_bundle_is_put_back_if_the_new_one_cannot_move_in() {
        let s = scene("rollback");
        let mut sys = Fake::new(good());
        sys.fail_rename_into = true;
        let (id, v) = (team(), semver::Version::new(1, 0, 0));
        let err = install_bundle(&sys, &s.zip, &s.installed, &s.work, &trust(&id, &v)).unwrap_err();
        assert!(
            err.to_string()
                .contains("could not move the update into place"),
            "{err}"
        );
        assert_eq!(installed_version(&s), "1.0.0");
    }

    #[test]
    fn leftovers_of_an_interrupted_swap_are_swept() {
        let s = scene("sweep");
        let parent = s.installed.parent().unwrap();
        std::fs::create_dir_all(parent.join(".Félagi.app.old-123/Contents")).unwrap();
        std::fs::create_dir_all(parent.join("Other.app")).unwrap();
        sweep_leftovers(&NativeSystem, &s.installed);
        assert!(!parent.join(".Félagi.app.old-123").exists());
        assert!(parent.join("Other.app").exists());
        assert!(s.installed.exists());
    }

    #[test]
    fn codesign_output_is_read_by_field() {
        let text = "Executable=/A.app/Contents/MacOS/a\nIdentifier=works.felagi.desktop\nTeamIdentifier=52AT44QFCM\n";
        assert_eq!(
            field(text, "Identifier=").as_deref(),
            Some("works.felagi.desktop")
        );
        assert_eq!(
            field(text, "TeamIdentifier=").as_deref(),
            Some("52AT44QFCM")
        );
        assert_eq!(field(text, "Authority="), None);
    }

    /// The real `codesign`, against apps that are on this machine: an
    /// installed Developer ID app satisfies its own team's requirement and
    /// not another's; an Apple app is no Developer ID. Passes trivially where
    /// none is installed.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_real_check_tells_a_developer_id_app_from_others() {
        let eterm = Path::new("/Applications/eterm.app");
        if eterm.is_dir() {
            let sig = NativeSystem.code_signature(
                eterm,
                Some(&DeveloperId::new("52AT44QFCM", "dev.eterm.eterm")),
            );
            if sig.team_id.as_deref() == Some("52AT44QFCM") {
                assert_eq!(
                    judge(&sig, &DeveloperId::new("52AT44QFCM", "dev.eterm.eterm")),
                    Ok(())
                );
                let other = DeveloperId::new("7G383N3VY7", "dev.eterm.eterm");
                let sig = NativeSystem.code_signature(eterm, Some(&other));
                assert!(judge(&sig, &other).is_err());
            }
        }
        let calculator = Path::new("/System/Applications/Calculator.app");
        if calculator.is_dir() {
            let id = DeveloperId::new("52AT44QFCM", "com.apple.calculator");
            let sig = NativeSystem.code_signature(calculator, Some(&id));
            assert!(sig.signed);
            assert!(!sig.developer_id);
            assert!(judge(&sig, &id).is_err());
        }
    }
}
