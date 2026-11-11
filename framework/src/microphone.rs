//! The microphone (RFC 0032): the shell records, the frontend gets a file.
//!
//! The page never listens itself. `/__mic/start` opens the system's default
//! input, `/__mic/stop` writes what was heard as a WAV (16 kHz, mono,
//! 16-bit) and hands the frontend a [`ChosenFile`], as a file the user
//! picked: a command can attach it, the outbox can queue it, and
//! `/__mic/transcribe` can turn it into text through the app's server.
//!
//! ```ignore
//! App::new()
//!     .microphone("Notes uses the microphone to let you dictate.")
//!     .backend(Backend::new(url).transcription())
//! ```
//!
//! One recording at a time, for the whole app. Only the window that started
//! it may stop or cancel it, and it's cancelled when that window closes. It
//! stops by itself after [`Microphone::max_seconds`]. While it runs, the
//! window hears [`MicrophoneEvent`]s on [`CHANNEL`].
//!
//! Recording needs elyra's `microphone` feature. Tests put
//! [`Microphone::fake`] in its place with `App::swap`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use parking_lot::Mutex;
use serde::Serialize;

use crate::event::EventBus;
use crate::files::ChosenFile;

/// The channel a recording window hears [`MicrophoneEvent`]s on.
pub const CHANNEL: &str = "elyra:microphone";

/// The longest a recording runs unless the app says less.
pub const DEFAULT_MAX_SECONDS: u32 = 300;

/// The rate recordings are written at: what speech models take.
pub const SAMPLE_RATE: u32 = 16_000;

/// The largest file `/__mic/transcribe` sends: OpenAI's limit.
pub const MAX_TRANSCRIBE_BYTES: u64 = 25 * 1024 * 1024;

/// How often the window hears the level.
const LEVEL_EVERY: Duration = Duration::from_millis(66);

/// Recordings older than this are removed when the next one starts.
const KEEP_FOR: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// What a recording window hears on [`CHANNEL`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MicrophoneEvent {
    /// How loud it is now, 0 to 1, for a meter.
    Level { level: f32 },
    /// It reached its longest and stopped: what was heard.
    Stopped { file: ChosenFile },
    /// The input failed (unplugged, taken away): nothing was kept.
    Failed { message: String },
}

/// Where a recording's samples go as they arrive, from the input's thread.
#[derive(Default)]
pub struct Buffer(Mutex<BufferInner>);

#[derive(Default)]
struct BufferInner {
    /// Interleaved, `channels` to a frame, at `rate`.
    samples: Vec<f32>,
    rate: u32,
    channels: u16,
    /// The loudest since the level was last read.
    peak: f32,
    failed: Option<String>,
}

impl Buffer {
    /// The input's format, before its first samples.
    pub fn set_format(&self, rate: u32, channels: u16) {
        let mut inner = self.0.lock();
        inner.rate = rate;
        inner.channels = channels.max(1);
    }

    /// Samples as the input hears them, interleaved.
    pub fn push(&self, data: &[f32]) {
        let mut inner = self.0.lock();
        let peak = data.iter().fold(0f32, |m, s| m.max(s.abs()));
        inner.peak = inner.peak.max(peak);
        inner.samples.extend_from_slice(data);
    }

    /// The input failed: the recording ends with `message`.
    pub fn fail(&self, message: impl Into<String>) {
        self.0.lock().failed.get_or_insert(message.into());
    }

    fn take_peak(&self) -> f32 {
        std::mem::take(&mut self.0.lock().peak).min(1.0)
    }

    fn failed(&self) -> Option<String> {
        self.0.lock().failed.clone()
    }

    fn wav(&self) -> Vec<u8> {
        let inner = self.0.lock();
        wav(&inner.samples, inner.rate.max(1), inner.channels.max(1))
    }
}

/// A source of samples: the system's default input, or a test's.
pub trait Input: Send + Sync + 'static {
    /// Start hearing into `buffer` (its format set first). Recording lasts
    /// until the returned capture is dropped.
    fn open(&self, buffer: Arc<Buffer>) -> Result<Box<dyn Capture>, String>;
}

/// A running input; dropping it stops it.
pub trait Capture: Send {}

/// The app's microphone: the input, its limit, and the recording in progress.
#[derive(Clone)]
pub struct Microphone {
    inner: Arc<Inner>,
}

struct Inner {
    input: Arc<dyn Input>,
    max_seconds: u32,
    dir: Mutex<Option<PathBuf>>,
    recording: Mutex<Option<Recording>>,
    /// Each recording's number, so a ticker outliving its recording stops.
    next: std::sync::atomic::AtomicU64,
}

struct Recording {
    number: u64,
    client: String,
    buffer: Arc<Buffer>,
    /// Kept to keep the input running; dropped to stop it.
    _capture: Box<dyn Capture>,
    started: Instant,
    max: Duration,
}

impl std::fmt::Debug for Microphone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Microphone")
            .field("max_seconds", &self.inner.max_seconds)
            .field("recording", &self.is_recording())
            .finish()
    }
}

impl Microphone {
    /// The system's default input.
    #[cfg(feature = "microphone")]
    pub fn system() -> Self {
        Self::with_input(system::SystemInput)
    }

    /// Recording from `input`.
    pub fn with_input(input: impl Input) -> Self {
        Self {
            inner: Arc::new(Inner {
                input: Arc::new(input),
                max_seconds: DEFAULT_MAX_SECONDS,
                dir: Mutex::new(None),
                recording: Mutex::new(None),
                next: std::sync::atomic::AtomicU64::new(1),
            }),
        }
    }

    /// A stand-in for tests: each recording hears `samples` (interleaved,
    /// `channels` to a frame, at `rate`) at once, then silence.
    pub fn fake(rate: u32, channels: u16, samples: Vec<f32>) -> Self {
        Self::with_input(Fake::Samples {
            rate,
            channels,
            samples,
        })
    }

    /// A stand-in whose input can't be opened, as when the user said no.
    pub fn fake_unavailable(message: impl Into<String>) -> Self {
        Self::with_input(Fake::Unavailable(message.into()))
    }

    /// Stop each recording after `seconds` (at most, and by default, 300).
    pub fn max_seconds(self, seconds: u32) -> Self {
        let inner = Arc::try_unwrap(self.inner).unwrap_or_else(|_| {
            panic!("Microphone::max_seconds: set it before the microphone is used")
        });
        Self {
            inner: Arc::new(Inner {
                max_seconds: seconds.clamp(1, DEFAULT_MAX_SECONDS),
                ..inner
            }),
        }
    }

    /// Write recordings to `dir` (by default the app's cache folder).
    pub fn folder(self, dir: impl Into<PathBuf>) -> Self {
        *self.inner.dir.lock() = Some(dir.into());
        self
    }

    /// Whether something is being recorded now.
    pub fn is_recording(&self) -> bool {
        self.inner.recording.lock().is_some()
    }

    /// Where recordings go, for the app `name`, unless set.
    pub(crate) fn for_app(&self, name: &str) {
        let mut dir = self.inner.dir.lock();
        if dir.is_none() {
            *dir = Some(default_folder(name));
        }
    }

    /// Start recording for window `client`, telling it on `bus`.
    pub(crate) fn start(
        &self,
        bus: EventBus,
        client: &str,
        max_seconds: Option<u32>,
    ) -> Result<(), String> {
        let mut recording = self.inner.recording.lock();
        if recording.is_some() {
            return Err("the microphone is already recording".into());
        }
        let buffer = Arc::new(Buffer::default());
        let capture = self.inner.input.open(buffer.clone())?;
        let number = self
            .inner
            .next
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let seconds = max_seconds
            .unwrap_or(self.inner.max_seconds)
            .clamp(1, self.inner.max_seconds);
        *recording = Some(Recording {
            number,
            client: client.to_owned(),
            buffer,
            _capture: capture,
            started: Instant::now(),
            max: Duration::from_secs(u64::from(seconds)),
        });
        drop(recording);
        self.tidy();
        self.tick(bus, number);
        Ok(())
    }

    /// Stop window `client`'s recording and write it.
    pub(crate) fn stop(&self, client: &str) -> Result<ChosenFile, String> {
        let recording = self.take(client)?;
        self.write(recording)
    }

    /// Stop window `client`'s recording and keep nothing.
    pub(crate) fn cancel(&self, client: &str) -> Result<(), String> {
        self.take(client).map(drop)
    }

    /// Windows that used `clients` closed: a recording of theirs is dropped.
    pub(crate) fn released(&self, clients: &[String]) {
        let mut recording = self.inner.recording.lock();
        if recording
            .as_ref()
            .is_some_and(|r| clients.contains(&r.client))
        {
            *recording = None;
        }
    }

    fn take(&self, client: &str) -> Result<Recording, String> {
        let mut recording = self.inner.recording.lock();
        match recording.as_ref() {
            None => Err("the microphone isn't recording".into()),
            Some(r) if r.client != client => {
                Err("another window is recording; only it can stop".into())
            }
            Some(_) => Ok(recording.take().expect("checked above")),
        }
    }

    fn write(&self, recording: Recording) -> Result<ChosenFile, String> {
        // The input stops before its samples are read.
        let Recording {
            buffer, _capture, ..
        } = recording;
        drop(_capture);
        let dir = self
            .inner
            .dir
            .lock()
            .clone()
            .unwrap_or_else(|| default_folder("elyra-app"));
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default();
        let id = &crate::security::random_token()[..8];
        let path = dir.join(format!("recording-{stamp}-{id}.wav"));
        std::fs::write(&path, buffer.wav()).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(ChosenFile::from_path(path))
    }

    /// Tell the window the level, and end the recording at its longest or
    /// when the input fails.
    fn tick(&self, bus: EventBus, number: u64) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let mic = self.clone();
        runtime.spawn(async move {
            let mut every = tokio::time::interval(LEVEL_EVERY);
            every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                every.tick().await;
                let mut recording = mic.inner.recording.lock();
                let Some(now) = recording.as_ref().filter(|r| r.number == number) else {
                    return;
                };
                let client = now.client.clone();
                if let Some(message) = now.buffer.failed() {
                    *recording = None;
                    drop(recording);
                    let _ = bus.emit_to(&client, CHANNEL, &MicrophoneEvent::Failed { message });
                    return;
                }
                if now.started.elapsed() >= now.max {
                    let done = recording.take().expect("checked above");
                    drop(recording);
                    let event = match mic.write(done) {
                        Ok(file) => MicrophoneEvent::Stopped { file },
                        Err(message) => MicrophoneEvent::Failed { message },
                    };
                    let _ = bus.emit_to(&client, CHANNEL, &event);
                    return;
                }
                let level = now.buffer.take_peak();
                drop(recording);
                let _ = bus.emit_to(&client, CHANNEL, &MicrophoneEvent::Level { level });
            }
        });
    }

    /// Remove recordings older than a week.
    fn tidy(&self) {
        let Some(dir) = self.inner.dir.lock().clone() else {
            return;
        };
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !(name.starts_with("recording-") && name.ends_with(".wav")) {
                continue;
            }
            let old = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > KEEP_FOR);
            if old {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

#[cfg(feature = "ai")]
const NO_TRANSCRIBER: &str = "nothing transcribes here: add Backend::transcription() \
(with Route::elyraTranscription() on the server), or AiProvider with OPENAI_API_KEY";

/// `file` (one the user chose or recorded) as text: through the app's
/// server when the backend has [`transcription`](crate::backend::Backend::transcription),
/// or else the container's `Ai`. `language` is a hint (ISO-639-1, `"no"`).
#[cfg(feature = "ai")]
pub async fn transcribe(
    ctx: &crate::Ctx,
    file: &ChosenFile,
    language: Option<&str>,
) -> Result<String, String> {
    // Only what may be read: opened, dropped or recorded, not a path chosen
    // to save to.
    if file.kind() != crate::files::FileChoice::Open {
        return Err(format!(
            "{} wasn't chosen to be read: only a file opened, dropped or recorded is transcribed",
            file.name()
        ));
    }
    let size = file
        .size()
        .ok_or_else(|| format!("{} isn't there any more", file.name()))?;
    if size > MAX_TRANSCRIBE_BYTES {
        return Err(format!(
            "{} is too large to transcribe: {} MB, and at most 25 MB is sent",
            file.name(),
            size / (1024 * 1024)
        ));
    }
    let ai = transcriber(ctx).ok_or(NO_TRANSCRIBER)?;
    let bytes = std::fs::read(file.path()).map_err(|e| format!("{}: {e}", file.name()))?;
    let mut request = ai.transcribe(bytes, file.name());
    if let Some(language) = language.filter(|l| !l.is_empty()) {
        request = request.language(language);
    }
    request.generate().await.map_err(|e| e.to_string())
}

/// The server first (`Backend::transcription()`), then the container's `Ai`.
#[cfg(feature = "ai")]
fn transcriber(ctx: &crate::Ctx) -> Option<crate::ai::Ai> {
    #[cfg(feature = "backend")]
    if let Some(backend) = ctx.try_get::<crate::backend::Backend>() {
        if let Some(route) = backend.transcription_config() {
            let base = backend.base_url()?;
            let session = (*backend).clone();
            return Some(
                crate::ai::Ai::builder()
                    .base_url(crate::ai::Provider::OpenAI, format!("{base}{route}"))
                    .bearer(crate::ai::Provider::OpenAI, move || session.current_token())
                    .build(),
            );
        }
    }
    ctx.try_get::<crate::ai::Ai>().map(|ai| (*ai).clone())
}

/// `<cache>/<app-slug>/recordings`, or under the temp folder without one.
fn default_folder(name: &str) -> PathBuf {
    let slug = crate::winstate::app_dir(name)
        .and_then(|d| d.file_name().map(|n| n.to_owned()))
        .unwrap_or_else(|| "elyra-app".into());
    cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(slug)
        .join("recordings")
}

#[cfg(feature = "microphone")]
fn cache_dir() -> Option<PathBuf> {
    dirs::cache_dir()
}

#[cfg(not(feature = "microphone"))]
fn cache_dir() -> Option<PathBuf> {
    None
}

enum Fake {
    Samples {
        rate: u32,
        channels: u16,
        samples: Vec<f32>,
    },
    Unavailable(String),
}

struct FakeCapture;
impl Capture for FakeCapture {}

impl Input for Fake {
    fn open(&self, buffer: Arc<Buffer>) -> Result<Box<dyn Capture>, String> {
        match self {
            Fake::Samples {
                rate,
                channels,
                samples,
            } => {
                buffer.set_format(*rate, *channels);
                buffer.push(samples);
                Ok(Box::new(FakeCapture))
            }
            Fake::Unavailable(message) => Err(message.clone()),
        }
    }
}

/// `samples` (interleaved, `channels` to a frame, at `rate`) as a WAV file:
/// mixed down to mono, resampled to [`SAMPLE_RATE`], 16-bit.
pub fn wav(samples: &[f32], rate: u32, channels: u16) -> Vec<u8> {
    let mono = downmix(samples, channels.max(1));
    let pcm = resample(&mono, rate.max(1), SAMPLE_RATE);
    let data_len = (pcm.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + pcm.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // the fmt chunk's size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // bytes a second
    out.extend_from_slice(&2u16.to_le_bytes()); // bytes a frame
    out.extend_from_slice(&16u16.to_le_bytes()); // bits a sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in pcm {
        let v = (s.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// The frames' average across channels.
fn downmix(samples: &[f32], channels: u16) -> Vec<f32> {
    if channels == 1 {
        return samples.to_vec();
    }
    samples
        .chunks_exact(usize::from(channels))
        .map(|frame| frame.iter().sum::<f32>() / f32::from(channels))
        .collect()
}

/// Linear resampling from `from` to `to`; speech needs no better.
fn resample(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || samples.is_empty() {
        return samples.to_vec();
    }
    let step = f64::from(from) / f64::from(to);
    let len = ((samples.len() as f64) / step).floor() as usize;
    (0..len)
        .map(|i| {
            let at = i as f64 * step;
            let j = at.floor() as usize;
            let frac = (at - j as f64) as f32;
            let a = samples[j];
            let b = samples.get(j + 1).copied().unwrap_or(a);
            a + (b - a) * frac
        })
        .collect()
}

/// The system's default input, through `cpal`. Its stream lives on a thread
/// of its own (it isn't `Send` everywhere), until the capture is dropped.
#[cfg(feature = "microphone")]
mod system {
    use std::sync::mpsc;
    use std::sync::Arc;

    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{FromSample, SampleFormat, SizedSample};

    use super::{Buffer, Capture, Input};

    pub(super) struct SystemInput;

    struct Running {
        stop: Option<mpsc::Sender<()>>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl Capture for Running {}

    impl Drop for Running {
        fn drop(&mut self) {
            drop(self.stop.take());
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    impl Input for SystemInput {
        fn open(&self, buffer: Arc<Buffer>) -> Result<Box<dyn Capture>, String> {
            let (stop_tx, stop_rx) = mpsc::channel::<()>();
            let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
            let thread = std::thread::Builder::new()
                .name("elyra-microphone".into())
                .spawn(move || match open_stream(buffer) {
                    Ok(stream) => {
                        let _ = ready_tx.send(Ok(()));
                        // Until the capture is dropped.
                        let _ = stop_rx.recv();
                        drop(stream);
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                    }
                })
                .map_err(|e| format!("the microphone's thread: {e}"))?;
            match ready_rx.recv() {
                Ok(Ok(())) => Ok(Box::new(Running {
                    stop: Some(stop_tx),
                    thread: Some(thread),
                })),
                Ok(Err(e)) => {
                    let _ = thread.join();
                    Err(e)
                }
                Err(_) => Err("the microphone stopped before it started".into()),
            }
        }
    }

    fn open_stream(buffer: Arc<Buffer>) -> Result<cpal::Stream, String> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or("there's no microphone: no input device was found")?;
        let config = device
            .default_input_config()
            .map_err(|e| format!("the microphone can't be used: {e}"))?;
        buffer.set_format(config.sample_rate(), config.channels());
        let format = config.sample_format();
        let config: cpal::StreamConfig = config.into();
        let stream = match format {
            SampleFormat::I8 => build::<i8>(&device, &config, buffer),
            SampleFormat::I16 => build::<i16>(&device, &config, buffer),
            SampleFormat::I32 => build::<i32>(&device, &config, buffer),
            SampleFormat::U8 => build::<u8>(&device, &config, buffer),
            SampleFormat::U16 => build::<u16>(&device, &config, buffer),
            SampleFormat::F32 => build::<f32>(&device, &config, buffer),
            SampleFormat::F64 => build::<f64>(&device, &config, buffer),
            other => Err(format!("the microphone's format ({other}) isn't supported")),
        }?;
        stream
            .play()
            .map_err(|e| format!("the microphone can't be started: {e}"))?;
        Ok(stream)
    }

    fn build<T>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        buffer: Arc<Buffer>,
    ) -> Result<cpal::Stream, String>
    where
        T: SizedSample,
        f32: FromSample<T>,
    {
        let failed = buffer.clone();
        device
            .build_input_stream(
                *config,
                move |data: &[T], _: &cpal::InputCallbackInfo| {
                    let samples: Vec<f32> = data.iter().map(|s| s.to_sample::<f32>()).collect();
                    buffer.push(&samples);
                },
                move |err| failed.fail(format!("the microphone stopped: {err}")),
                None,
            )
            .map_err(|e| format!("the microphone can't be opened: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(bytes: &[u8]) -> (u32, u16, u32, u16, u32) {
        let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
        let u32_at =
            |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
        (u32_at(4), u16_at(22), u32_at(24), u16_at(34), u32_at(40))
    }

    #[test]
    fn a_wav_is_16_khz_mono_16_bit() {
        // One second of stereo at 48 kHz.
        let samples = vec![0.5f32; 48_000 * 2];
        let bytes = wav(&samples, 48_000, 2);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..16], b"WAVEfmt ");
        let (riff, channels, rate, bits, data) = header(&bytes);
        assert_eq!((channels, rate, bits), (1, SAMPLE_RATE, 16));
        assert_eq!(data, SAMPLE_RATE * 2);
        assert_eq!(riff, 36 + data);
        assert_eq!(bytes.len(), 44 + data as usize);
    }

    #[test]
    fn channels_are_averaged_and_samples_clamped() {
        // Left loud, right silent: half. Then beyond full scale: clamped.
        let bytes = wav(&[1.0, 0.0, 3.0, 3.0], SAMPLE_RATE, 2);
        let first = i16::from_le_bytes([bytes[44], bytes[45]]);
        let second = i16::from_le_bytes([bytes[46], bytes[47]]);
        assert_eq!(first, (f32::from(i16::MAX) * 0.5).round() as i16);
        assert_eq!(second, i16::MAX);
    }

    #[test]
    fn resampling_keeps_the_duration_and_the_shape() {
        let ramp: Vec<f32> = (0..44_100).map(|i| i as f32 / 44_100.0).collect();
        let out = resample(&ramp, 44_100, SAMPLE_RATE);
        assert_eq!(out.len(), 16_000);
        assert!(out.windows(2).all(|w| w[1] >= w[0]), "still rising");
        assert!((out[8_000] - 0.5).abs() < 0.001, "{}", out[8_000]);
        assert_eq!(resample(&[], 48_000, SAMPLE_RATE), Vec::<f32>::new());
        assert_eq!(resample(&[0.25], SAMPLE_RATE, SAMPLE_RATE), vec![0.25]);
    }

    #[test]
    fn an_empty_recording_is_a_valid_wav() {
        let bytes = wav(&[], 48_000, 1);
        assert_eq!(bytes.len(), 44);
        assert_eq!(header(&bytes).4, 0);
    }

    #[test]
    fn max_seconds_is_kept_within_the_cap() {
        assert_eq!(
            Microphone::fake(16_000, 1, vec![])
                .max_seconds(0)
                .inner
                .max_seconds,
            1
        );
        assert_eq!(
            Microphone::fake(16_000, 1, vec![])
                .max_seconds(900)
                .inner
                .max_seconds,
            300
        );
    }
}
