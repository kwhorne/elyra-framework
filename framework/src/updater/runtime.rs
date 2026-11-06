//! The built-in update flow, bound in the container by [`crate::App`]: the
//! check (from a manifest URL or the backend), the schedule, installing, and
//! relaunching, with each phase on `elyra:update` for the runtime's toast.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::Serialize;
use tokio::sync::Notify;

use super::bundle::{self, BundleSystem, BundleTrust, DeveloperId, NotInstalled};
use super::{temp_dir, Error, UpdateCheck, UpdateInfo, UpdateStatus, Updater, UpdaterConfig};
use crate::event::EventBus;

/// The channel the runtime's toast follows.
pub const CHANNEL: &str = "elyra:update";

/// From the backend: the first check, this long after start…
const FIRST_CHECK: Duration = Duration::from_secs(10);
/// …this long after a sign-in or a switch to another server…
const AFTER_SIGN_IN: Duration = Duration::from_secs(3);
/// …and then this often.
const EVERY: Duration = Duration::from_secs(24 * 60 * 60);

/// Where the update goes: the running app's bundle, or its executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A macOS `.app`, replaced whole.
    Bundle(PathBuf),
    /// A loose executable, swapped in place.
    Executable(PathBuf),
}

impl Target {
    pub fn path(&self) -> &Path {
        match self {
            Target::Bundle(p) | Target::Executable(p) => p,
        }
    }
}

enum Source {
    Url(String),
    #[cfg(feature = "backend")]
    Backend {
        backend: Box<crate::backend::Backend>,
        path: String,
    },
    /// `from_backend` in an app without a backend.
    #[cfg_attr(feature = "backend", allow(dead_code))]
    NoBackend,
}

enum State {
    Idle,
    Installing,
    Installed { version: String, target: Target },
}

type Quit = Arc<dyn Fn() + Send + Sync>;

/// A phase on `elyra:update`, consumed by the runtime toast.
#[derive(Serialize)]
pub(crate) struct UpdatePhase {
    phase: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    progress: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    /// `"ask"` or `"now"`: whether installing restarts the app.
    #[serde(skip_serializing_if = "Option::is_none")]
    restart: Option<&'static str>,
}

impl UpdatePhase {
    fn of(phase: &'static str) -> Self {
        Self {
            phase,
            version: None,
            notes: None,
            progress: None,
            message: None,
            restart: None,
        }
    }
}

/// The assembled update flow, bound in the container by [`crate::App`] and
/// driven by the shell's `/__update/*` routes and its schedule.
pub struct UpdaterRuntime {
    updater: Updater,
    source: Source,
    target: String,
    auto_check: bool,
    relaunch_when_asked: bool,
    developer_id: Option<DeveloperId>,
    #[cfg_attr(not(feature = "backend"), allow(dead_code))]
    max_artifact_bytes: u64,
    bus: Option<EventBus>,
    system: Arc<dyn BundleSystem>,
    exe: Option<PathBuf>,
    state: Mutex<State>,
    /// The last version announced by the schedule.
    announced: Mutex<Option<String>>,
    wake: Arc<Notify>,
    quit: Mutex<Option<Quit>>,
    delays: (Duration, Duration, Duration),
}

impl UpdaterRuntime {
    /// The flow `config` describes. One from the backend needs
    /// [`with_backend`](Self::with_backend), which `App` does.
    pub fn new(config: &UpdaterConfig, bus: Option<EventBus>) -> Result<Self, Error> {
        let updater = config.build()?;
        let source = if config.is_from_backend() {
            Source::NoBackend
        } else {
            Source::Url(config.manifest_url.clone())
        };
        Ok(Self {
            updater,
            source,
            target: Updater::current_target(),
            auto_check: config.auto_check,
            relaunch_when_asked: config.relaunch_when_asked,
            developer_id: config.developer_id.clone(),
            max_artifact_bytes: config.max_artifact_bytes,
            bus,
            system: Arc::new(bundle::NativeSystem),
            exe: None,
            state: Mutex::new(State::Idle),
            announced: Mutex::new(None),
            wake: Arc::new(Notify::new()),
            quit: Mutex::new(None),
            delays: (FIRST_CHECK, AFTER_SIGN_IN, EVERY),
        })
    }

    /// Ask `backend`'s active server, at `config`'s `backend_path`.
    #[cfg(feature = "backend")]
    pub fn with_backend(
        mut self,
        config: &UpdaterConfig,
        backend: crate::backend::Backend,
    ) -> Self {
        if let Some(path) = &config.backend_path {
            self.source = Source::Backend {
                backend: Box::new(backend),
                path: path.clone(),
            };
        }
        self
    }

    /// The system the bundle steps run on (a fake, in a test).
    pub fn with_system(mut self, system: Arc<dyn BundleSystem>) -> Self {
        self.system = system;
        self
    }

    /// Treat `exe` as the running executable (a test's stand-in for an
    /// installed app).
    pub fn with_executable(mut self, exe: impl Into<PathBuf>) -> Self {
        self.exe = Some(exe.into());
        self
    }

    #[cfg(all(test, feature = "backend"))]
    pub(crate) fn with_delays(mut self, first: Duration, after: Duration, every: Duration) -> Self {
        self.delays = (first, after, every);
        self
    }

    /// The platform asked about: `macos-aarch64`.
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Whether installing waits for a separate restart.
    pub fn relaunch_when_asked(&self) -> bool {
        self.relaunch_when_asked
    }

    fn restart_word(&self) -> &'static str {
        if self.relaunch_when_asked {
            "ask"
        } else {
            "now"
        }
    }

    /// How the app quits for a relaunch (the shell's, which saves the window
    /// first); without one, the process exits.
    pub fn set_quit(&self, quit: impl Fn() + Send + Sync + 'static) {
        *self.quit.lock() = Some(Arc::new(quit));
    }

    /// A sign-in, a switch or a sign-out: check again shortly.
    pub fn wake(&self) {
        self.wake.notify_one();
    }

    /// The waker, for what can't hold the runtime (`Auth::watch`).
    #[cfg_attr(not(feature = "backend"), allow(dead_code))]
    pub(crate) fn waker(&self) -> Arc<Notify> {
        self.wake.clone()
    }

    fn emit(&self, phase: UpdatePhase) {
        if let Some(bus) = &self.bus {
            let _ = bus.emit(CHANNEL, &phase);
        }
    }

    fn asks_backend(&self) -> bool {
        !matches!(self.source, Source::Url(_))
    }

    /// Whether there's someone to ask: always for a manifest URL; for the
    /// backend, a user signed in to the active server.
    pub fn can_check(&self) -> bool {
        match &self.source {
            Source::Url(_) => true,
            #[cfg(feature = "backend")]
            Source::Backend { backend, .. } => backend.has_token() && backend.base_url().is_some(),
            Source::NoBackend => false,
        }
    }

    /// Ask whether a newer release exists. From the backend: the active
    /// server, as the signed-in user; signed out, [`Error::SignedOut`] and
    /// nothing sent.
    pub async fn check(&self) -> Result<UpdateStatus, Error> {
        match &self.source {
            Source::Url(url) => {
                let (updater, url, target) =
                    (self.updater.clone(), url.clone(), self.target.clone());
                tokio::task::spawn_blocking(move || updater.check(&url, &target))
                    .await
                    .map_err(|e| Error::Io(e.to_string()))?
            }
            #[cfg(feature = "backend")]
            Source::Backend { backend, path } => {
                super::feed::check(backend, path, self.updater.current_version(), &self.target)
                    .await
            }
            Source::NoBackend => Err(Error::Config(
                "UpdaterConfig::from_backend(..) needs App::backend(..)".into(),
            )),
        }
    }

    /// `/__update/check`: what the toast needs. An update already installed
    /// is `ready`.
    pub async fn check_for_frontend(&self) -> UpdateCheck {
        if let State::Installed { version, .. } = &*self.state.lock() {
            return UpdateCheck {
                available: true,
                version: Some(version.clone()),
                notes: None,
                error: None,
                ready: true,
                restart: Some(self.restart_word().into()),
            };
        }
        let mut check = match self.check().await {
            Ok(status) => UpdateCheck::from(status),
            Err(e) => UpdateCheck {
                available: false,
                version: None,
                notes: None,
                error: Some(e.to_string()),
                ready: false,
                restart: None,
            },
        };
        if check.available {
            check.restart = Some(self.restart_word().into());
        }
        check
    }

    /// A check of the schedule's: `available` when there's a newer release
    /// (again on `force`, else only when it's another version than the last
    /// announced), and `up-to-date` when an announced one is gone (signed
    /// out, or another server with nothing newer). Failures are logged.
    pub async fn announce(&self, force: bool) {
        if !matches!(*self.state.lock(), State::Idle) {
            return;
        }
        if !self.can_check() {
            if self.announced.lock().take().is_some() {
                self.emit(UpdatePhase::of("up-to-date"));
            }
            return;
        }
        match self.check().await {
            Ok(UpdateStatus::Available(info)) => {
                let mut announced = self.announced.lock();
                if force || announced.as_deref() != Some(info.version.as_str()) {
                    *announced = Some(info.version.clone());
                    drop(announced);
                    self.emit(UpdatePhase {
                        version: Some(info.version),
                        notes: info.notes,
                        restart: Some(self.restart_word()),
                        ..UpdatePhase::of("available")
                    });
                }
            }
            Ok(UpdateStatus::UpToDate) => {
                if self.announced.lock().take().is_some() {
                    self.emit(UpdatePhase::of("up-to-date"));
                }
            }
            Err(Error::SignedOut) => {}
            Err(e) => crate::warn!(target: "elyra::updater", "update check failed: {e}"),
        }
    }

    /// Where an update would go, or why it can't be installed: checked
    /// before anything is downloaded.
    pub fn install_target(&self) -> Result<Target, Error> {
        let exe = match &self.exe {
            Some(exe) => exe.clone(),
            None => std::env::current_exe().map_err(|e| Error::Io(e.to_string()))?,
        };
        let require_bundle = self.developer_id.is_some() || self.asks_backend();
        target_for(
            &exe,
            cfg!(target_os = "macos"),
            require_bundle,
            self.updater.has_public_key(),
            &*self.system,
        )
    }

    /// Download `info`'s artifact into a private directory and verify its
    /// bytes — the size and SHA-256 the manifest gave, and the ed25519
    /// signature when there's a key — returning the file. Nothing unverified
    /// is returned; the directory is the caller's to remove (the file's
    /// parent).
    pub async fn download(
        &self,
        info: &UpdateInfo,
        progress: impl Fn(u64, Option<u64>) + Send + Sync + 'static,
    ) -> Result<PathBuf, Error> {
        match &self.source {
            Source::Url(_) => {
                let (updater, info) = (self.updater.clone(), info.clone());
                let staged = tokio::task::spawn_blocking(move || {
                    updater.download_verified_with_progress(&info, progress)
                })
                .await
                .map_err(|e| Error::Io(e.to_string()))??;
                // Into a directory of its own, as the backend's.
                let dir = temp_dir("elyra-update")?;
                let dest = dir.join("update.download");
                if let Err(e) = std::fs::rename(&staged, &dest) {
                    let _ = std::fs::remove_file(&staged);
                    let _ = std::fs::remove_dir_all(&dir);
                    return Err(Error::Io(e.to_string()));
                }
                Ok(dest)
            }
            #[cfg(feature = "backend")]
            Source::Backend { backend, .. } => {
                if let Some(size) = info.size {
                    if size > self.max_artifact_bytes {
                        return Err(Error::TooLarge {
                            limit: self.max_artifact_bytes,
                        });
                    }
                }
                let sha256 = info
                    .sha256
                    .clone()
                    .ok_or_else(|| Error::Manifest("no `sha256`".into()))?;
                let dir = temp_dir("elyra-update")?;
                let dest = dir.join("update.download");
                let verified = async {
                    super::feed::download(
                        backend,
                        &info.url,
                        &dest,
                        self.max_artifact_bytes,
                        progress,
                    )
                    .await?;
                    if let Some(expected) = info.size {
                        let actual = std::fs::metadata(&dest)
                            .map_err(|e| Error::Io(e.to_string()))?
                            .len();
                        if actual != expected {
                            return Err(Error::SizeMismatch { expected, actual });
                        }
                    }
                    super::verify_sha256(&dest, &sha256)?;
                    if self.updater.has_public_key() {
                        if info.signature.is_empty() {
                            return Err(Error::Signature);
                        }
                        self.updater.verify_file(&dest, &info.signature)?;
                    }
                    Ok(())
                }
                .await;
                match verified {
                    Ok(()) => Ok(dest),
                    Err(e) => {
                        let _ = std::fs::remove_dir_all(&dir);
                        Err(e)
                    }
                }
            }
            Source::NoBackend => Err(Error::Config(
                "UpdaterConfig::from_backend(..) needs App::backend(..)".into(),
            )),
        }
    }

    /// Install: check again, download, verify, and put the new version where
    /// the running one is, with `downloading`, then `ready` (or `error`, or
    /// `up-to-date`) on `elyra:update`. With
    /// [`relaunch_when_asked`](UpdaterConfig::relaunch_when_asked) it stops
    /// there; otherwise it relaunches.
    pub async fn install(&self) -> Result<(), Error> {
        let outcome = self.install_once().await;
        match &outcome {
            Ok(()) | Err(Error::Busy) => {}
            Err(e) => {
                crate::warn!(target: "elyra::updater", "update not installed: {e}");
                self.emit(UpdatePhase {
                    message: Some(e.to_string()),
                    ..UpdatePhase::of("error")
                });
            }
        }
        outcome
    }

    async fn install_once(&self) -> Result<(), Error> {
        let installed = {
            let mut state = self.state.lock();
            match &*state {
                State::Installing => return Err(Error::Busy),
                State::Installed { version, .. } => Some(version.clone()),
                State::Idle => {
                    *state = State::Installing;
                    None
                }
            }
        };
        if let Some(version) = installed {
            self.emit_ready(version);
            return Ok(());
        }
        let steps = self.install_steps().await;
        let version = match steps {
            Ok(Some((version, target))) => {
                *self.state.lock() = State::Installed {
                    version: version.clone(),
                    target,
                };
                version
            }
            Ok(None) => {
                *self.state.lock() = State::Idle;
                *self.announced.lock() = None;
                self.emit(UpdatePhase::of("up-to-date"));
                return Ok(());
            }
            Err(e) => {
                *self.state.lock() = State::Idle;
                return Err(e);
            }
        };
        *self.announced.lock() = None;
        self.emit_ready(version);
        if !self.relaunch_when_asked {
            // Let the frontend paint "Restarting…" first.
            tokio::time::sleep(Duration::from_millis(400)).await;
            self.relaunch()?;
        }
        Ok(())
    }

    fn emit_ready(&self, version: String) {
        self.emit(UpdatePhase {
            version: Some(version),
            restart: Some(self.restart_word()),
            ..UpdatePhase::of("ready")
        });
    }

    async fn install_steps(&self) -> Result<Option<(String, Target)>, Error> {
        // Before anything is downloaded: there must be somewhere to put it.
        let target = self.install_target()?;
        let info = match self.check().await? {
            UpdateStatus::Available(info) => info,
            UpdateStatus::UpToDate => return Ok(None),
        };
        self.emit(UpdatePhase {
            progress: Some(0),
            ..UpdatePhase::of("downloading")
        });
        let bus = self.bus.clone();
        let last = Arc::new(AtomicU8::new(0));
        let staged = self
            .download(&info, move |got, total| {
                let pct = match total {
                    Some(t) if t > 0 => (got.saturating_mul(100) / t).min(100) as u8,
                    _ => 0,
                };
                if pct != last.swap(pct, Ordering::Relaxed) {
                    if let Some(bus) = &bus {
                        let _ = bus.emit(
                            CHANNEL,
                            &UpdatePhase {
                                progress: Some(pct),
                                ..UpdatePhase::of("downloading")
                            },
                        );
                    }
                }
            })
            .await?;
        let dir = staged
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(std::env::temp_dir);
        // An older signed release is refused from the backend or with a
        // Developer ID; a manifest URL with ed25519 alone works as before.
        let current = (self.developer_id.is_some() || self.asks_backend())
            .then(|| self.updater.current_version().clone());
        let (system, applying, developer_id) = (
            self.system.clone(),
            target.clone(),
            self.developer_id.clone(),
        );
        let work = dir.join("expanded");
        let applied = tokio::task::spawn_blocking(move || {
            std::fs::create_dir(&work).map_err(|e| Error::Io(e.to_string()))?;
            apply(
                &*system,
                &applying,
                &staged,
                &work,
                developer_id.as_ref(),
                current.as_ref(),
            )
        })
        .await
        .map_err(|e| Error::Io(e.to_string()));
        let _ = std::fs::remove_dir_all(&dir);
        applied??;
        crate::info!(
            target: "elyra::updater",
            "version {} is in place at {}",
            info.version,
            target.path().display()
        );
        Ok(Some((info.version, target)))
    }

    /// Start the installed version once this process has quit, and quit.
    pub fn relaunch(&self) -> Result<(), Error> {
        let target = match &*self.state.lock() {
            State::Installed { target, .. } => target.clone(),
            _ => return Err(Error::NothingInstalled),
        };
        self.system.relaunch(target.path()).map_err(Error::Io)?;
        let quit = self.quit.lock().clone();
        match quit {
            Some(quit) => quit(),
            None => std::process::exit(0),
        }
        Ok(())
    }

    /// The schedule (with `auto_check`): from a manifest URL, one check at
    /// start, as before; from the backend, a check shortly after start, and
    /// after each sign-in or switch, and every 24 hours.
    pub(crate) fn start(self: &Arc<Self>, handle: &tokio::runtime::Handle) {
        // What an interrupted install left beside the bundle.
        if let Ok(Target::Bundle(app)) = self.install_target() {
            bundle::sweep_leftovers(&*self.system, &app);
        }
        if !self.auto_check {
            return;
        }
        let me = Arc::clone(self);
        handle.spawn(async move { me.schedule().await });
    }

    async fn schedule(&self) {
        if !self.asks_backend() {
            self.announce(true).await;
            return;
        }
        let (first, after, every) = self.delays;
        let mut force = true;
        tokio::select! {
            () = tokio::time::sleep(first) => {}
            () = self.wake.notified() => tokio::time::sleep(after).await,
        }
        loop {
            self.announce(force).await;
            tokio::select! {
                () = tokio::time::sleep(every) => force = true,
                () = self.wake.notified() => {
                    tokio::time::sleep(after).await;
                    force = false;
                }
            }
        }
    }
}

/// Where an update for the executable at `exe` goes. On macOS, an installed
/// bundle (not a build, not translocated, in a folder this user can write);
/// a loose executable only when no bundle is required (a manifest URL and an
/// ed25519 key, as before). Elsewhere, the executable, which only an ed25519
/// key can verify.
pub(crate) fn target_for(
    exe: &Path,
    macos: bool,
    require_bundle: bool,
    has_key: bool,
    sys: &dyn BundleSystem,
) -> Result<Target, Error> {
    let writable = |dir: Option<&Path>| -> Result<(), Error> {
        let dir = dir.ok_or_else(|| Error::NotInstallable("the app has no folder".into()))?;
        if sys.can_write(dir) {
            return Ok(());
        }
        let volume = if dir.starts_with("/Volumes") {
            " (a disk image or a read-only volume? Drag the app to Applications first)"
        } else {
            ""
        };
        Err(Error::NotInstallable(format!(
            "no permission to replace the app in {}{volume}",
            dir.display()
        )))
    };
    if macos {
        return match bundle::installed_bundle(exe) {
            Ok(app) => {
                writable(app.parent())?;
                Ok(Target::Bundle(app))
            }
            Err(NotInstalled::NotABundle) if !require_bundle && has_key => {
                writable(exe.parent())?;
                Ok(Target::Executable(exe.to_path_buf()))
            }
            Err(why) => Err(Error::NotInstallable(why.message().into())),
        };
    }
    // A build can't be updated whatever the trust: said first, being the
    // answer that holds on every platform.
    if bundle::from_build(exe) {
        return Err(Error::NotInstallable(
            NotInstalled::FromBuild.message().into(),
        ));
    }
    if !has_key {
        return Err(Error::NotInstallable(
            "a Developer ID only verifies a macOS app; updates here need an ed25519 key \
             (UpdaterConfig::public_key)"
                .into(),
        ));
    }
    writable(exe.parent())?;
    Ok(Target::Executable(exe.to_path_buf()))
}

/// Put the verified artifact at `staged` in place at `target`.
pub(crate) fn apply(
    sys: &dyn BundleSystem,
    target: &Target,
    staged: &Path,
    work: &Path,
    developer_id: Option<&DeveloperId>,
    current: Option<&semver::Version>,
) -> Result<(), Error> {
    match target {
        Target::Bundle(app) => bundle::install_bundle(
            sys,
            staged,
            app,
            work,
            &BundleTrust {
                developer_id,
                current,
            },
        ),
        Target::Executable(exe) => swap_executable(exe, staged),
    }
}

/// Swap a loose executable for the staged one, keeping the old one beside it
/// as `<exe>.old` (a running executable can be renamed, not always deleted).
fn swap_executable(exe: &Path, staged: &Path) -> Result<(), Error> {
    let backup = exe.with_extension("old");
    let _ = std::fs::remove_file(&backup);
    std::fs::rename(exe, &backup).map_err(|e| Error::Io(e.to_string()))?;
    if let Err(e) = std::fs::copy(staged, exe) {
        let _ = std::fs::rename(&backup, exe); // roll back the swap
        return Err(Error::Io(e.to_string()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(exe, std::fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bundle::{BundleInfo, CodeSignature};

    struct Writable(bool);

    impl BundleSystem for Writable {
        fn expand(&self, _: &Path, _: &Path) -> Result<(), String> {
            Ok(())
        }
        fn code_signature(&self, _: &Path, _: Option<&DeveloperId>) -> CodeSignature {
            CodeSignature::default()
        }
        fn bundle_info(&self, _: &Path) -> BundleInfo {
            BundleInfo::default()
        }
        fn clear_quarantine(&self, _: &Path) -> Result<(), String> {
            Ok(())
        }
        fn can_write(&self, _: &Path) -> bool {
            self.0
        }
        fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
            std::fs::rename(from, to)
        }
        fn remove(&self, _: &Path) {}
        fn relaunch(&self, _: &Path) -> Result<(), String> {
            Ok(())
        }
    }

    const APP: &str = "/Applications/Félagi.app/Contents/MacOS/felagi";

    #[test]
    fn an_installed_bundle_is_the_target_on_macos() {
        assert_eq!(
            target_for(Path::new(APP), true, true, false, &Writable(true)).unwrap(),
            Target::Bundle(PathBuf::from("/Applications/Félagi.app"))
        );
    }

    #[test]
    fn running_from_a_build_is_refused_before_anything_is_downloaded() {
        let exe = Path::new("/Users/ada/felagi/target/debug/felagi-desktop");
        for (macos, require, key) in [
            (true, true, false),
            (true, false, true),
            (false, false, true),
            (false, true, false),
        ] {
            let err = target_for(exe, macos, require, key, &Writable(true)).unwrap_err();
            assert!(err.to_string().contains("started from a build"), "{err}");
        }
    }

    #[test]
    fn a_loose_executable_on_macos_needs_the_old_kind_of_update() {
        let exe = Path::new("/usr/local/bin/felagi");
        assert!(matches!(
            target_for(exe, true, true, true, &Writable(true)),
            Err(Error::NotInstallable(_))
        ));
        assert_eq!(
            target_for(exe, true, false, true, &Writable(true)).unwrap(),
            Target::Executable(exe.to_path_buf())
        );
    }

    #[test]
    fn a_folder_the_user_cant_write_is_refused() {
        let err = target_for(Path::new(APP), true, true, false, &Writable(false)).unwrap_err();
        assert!(err.to_string().contains("no permission"), "{err}");
        let dmg = Path::new("/Volumes/Félagi 2.0/Félagi.app/Contents/MacOS/felagi");
        let err = target_for(dmg, true, true, false, &Writable(false)).unwrap_err();
        assert!(err.to_string().contains("disk image"), "{err}");
    }

    #[test]
    fn elsewhere_a_developer_id_alone_cant_verify_anything() {
        let exe = Path::new("/opt/felagi/felagi");
        assert!(matches!(
            target_for(exe, false, true, false, &Writable(true)),
            Err(Error::NotInstallable(_))
        ));
        assert_eq!(
            target_for(exe, false, true, true, &Writable(true)).unwrap(),
            Target::Executable(exe.to_path_buf())
        );
    }

    #[cfg(feature = "backend")]
    #[tokio::test]
    async fn the_schedule_checks_soon_after_start_and_again_when_woken() {
        use crate::http::{HttpFake, Method};
        let route = "https://acme.felagi.test/api/desktop/update";
        let fake = HttpFake::new().get(
            route,
            200,
            serde_json::json!({
                "version": "2.0.0",
                "url": "/desktop/F.zip",
                "sha256": "ab".repeat(32),
            }),
        );
        let backend =
            crate::backend::Backend::new("https://acme.felagi.test").with_fake_for_tests(&fake);
        let bus = EventBus::new();
        bus.register_client("window");
        let config = UpdaterConfig::from_backend("/api/desktop/update", "1.0.0")
            .developer_id("52AT44QFCM", "works.felagi.desktop")
            .auto_check(true);
        let rt = Arc::new(
            UpdaterRuntime::new(&config, Some(bus.clone()))
                .unwrap()
                .with_backend(&config, backend.clone())
                .with_delays(
                    Duration::from_millis(20),
                    Duration::from_millis(20),
                    Duration::from_secs(3600),
                ),
        );
        rt.start(&tokio::runtime::Handle::current());

        // Signed out: the first check asks nobody.
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert!(fake.sent_to(Method::Get.as_str(), route).is_empty());
        assert_eq!(bus.pending_for("window"), 0);

        // Signed in (as `Auth::watch` would tell it): asked, and offered.
        backend.use_token(Some("acme-token".into()));
        rt.wake();
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(fake.sent_to("GET", route).len(), 1);
        assert_eq!(bus.pending_for("window"), 1);

        // Woken again: asked again, but the same version isn't offered twice.
        rt.wake();
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(fake.sent_to("GET", route).len(), 2);
        assert_eq!(bus.pending_for("window"), 1);
    }
}
