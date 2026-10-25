//! The sound card the player writes to, reopened when its stream dies under it.
//!
//! librespot's rodio sink keeps a stream for life. Once the card under it goes
//! away, or the PipeWire node it plays on fails, ALSA answers POLLERR on every
//! poll: cpal spins on it, never asks for samples again, and the sink waits
//! forever for its buffer to drain. The track then seems to play on in silence,
//! with no player event to tell anyone. This one drops such a stream and opens
//! a new one on the same output, which for the default follows the system's.
use cpal::traits::{DeviceTrait, HostTrait};
use librespot_playback::{
    NUM_CHANNELS, SAMPLE_RATE,
    audio_backend::{Sink, SinkError, SinkResult},
    convert::Converter,
    decoder::AudioPacket,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

/// About half a second of audio, in librespot's packets, as its own sink buffers.
const BUFFERED_PACKETS: usize = 26;
/// How long a buffer may stay full before its stream counts as dead, failing or not.
const STALL: Duration = Duration::from_secs(2);
/// The least time between two openings, so a stream that fails at once does not spin.
const REOPEN_GAP: Duration = Duration::from_secs(1);
const POLL: Duration = Duration::from_millis(10);

pub(crate) struct Output {
    /// The output by name, as `playback.output` names it; `None` for the system's.
    device: Option<String>,
    stream: Option<Stream>,
    opened: Option<Instant>,
}

struct Stream {
    sink: rodio::Sink,
    /// Set by cpal's error callback, which then keeps firing until the stream drops.
    failed: Arc<AtomicBool>,
    _stream: rodio::OutputStream,
}

impl Output {
    pub(crate) fn new(device: Option<String>) -> Self {
        Self {
            device,
            stream: None,
            opened: None,
        }
    }

    /// The live stream, opened again if the last one died.
    fn stream(&mut self) -> SinkResult<&Stream> {
        if self.stream.as_ref().is_some_and(Stream::dead) {
            eprintln!("audio stream died; reopening the output");
            self.stream = None;
        }
        if self.stream.is_none() {
            if let Some(wait) = self.opened.and_then(|at| REOPEN_GAP.checked_sub(at.elapsed())) {
                thread::sleep(wait);
            }
            self.opened = Some(Instant::now());
            self.stream = Some(Stream::open(self.device.as_deref())?);
        }
        Ok(self.stream.as_ref().expect("opened above"))
    }
}

impl Stream {
    fn open(device: Option<&str>) -> SinkResult<Self> {
        let host = cpal::default_host();
        let device = match device {
            Some(name) => host
                .output_devices()
                .map_err(refused)?
                .find(|device| device.name().is_ok_and(|found| found == name))
                .ok_or_else(|| SinkError::ConnectionRefused(format!("no output named {name}")))?,
            None => host
                .default_output_device()
                .ok_or_else(|| SinkError::ConnectionRefused("no default output".into()))?,
        };
        // Native stereo at 44.1 kHz if the card takes it, else its own rate,
        // which rodio resamples to; as librespot's sink chooses.
        let default = device.default_output_config().map_err(refused)?;
        let config = device
            .supported_output_configs()
            .map_err(refused)?
            .find(|config| config.channels() == NUM_CHANNELS as cpal::ChannelCount)
            .and_then(|config| {
                config
                    .try_with_sample_rate(cpal::SampleRate(SAMPLE_RATE))
                    .or_else(|| config.try_with_sample_rate(default.sample_rate()))
            })
            .unwrap_or(default);
        let failed = Arc::new(AtomicBool::new(false));
        let flag = failed.clone();
        let mut stream = rodio::OutputStreamBuilder::from_device(device)
            .map_err(refused)?
            .with_supported_config(&config)
            .with_error_callback(move |error| {
                if !flag.swap(true, Ordering::Relaxed) {
                    eprintln!("audio stream error: {error}");
                }
            })
            .open_stream_or_fallback()
            .map_err(refused)?;
        stream.log_on_drop(false);
        let sink = rodio::Sink::connect_new(stream.mixer());
        Ok(Self {
            sink,
            failed,
            _stream: stream,
        })
    }

    fn dead(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }

    /// Waits until at most `left` packets are queued; false if the stream died first.
    fn drain_to(&self, left: usize) -> bool {
        let mut queued = self.sink.len();
        let mut moved = Instant::now();
        while queued > left {
            if self.dead() || moved.elapsed() > STALL {
                self.failed.store(true, Ordering::Relaxed);
                return false;
            }
            thread::sleep(POLL);
            let now = self.sink.len();
            if now != queued {
                queued = now;
                moved = Instant::now();
            }
        }
        true
    }
}

impl Sink for Output {
    fn start(&mut self) -> SinkResult<()> {
        self.stream()?.sink.play();
        Ok(())
    }

    fn stop(&mut self) -> SinkResult<()> {
        // librespot exits on an error here; a dead stream has nothing left to play anyway.
        if let Some(stream) = &self.stream {
            stream.drain_to(0);
            stream.sink.pause();
        }
        Ok(())
    }

    fn write(&mut self, packet: AudioPacket, converter: &mut Converter) -> SinkResult<()> {
        let samples = packet
            .samples()
            .map_err(|error| SinkError::OnWrite(error.to_string()))?;
        let samples: &[f32] = &converter.f64_to_f32(samples);
        let stream = self.stream()?;
        stream.sink.append(rodio::buffer::SamplesBuffer::new(
            NUM_CHANNELS as cpal::ChannelCount,
            SAMPLE_RATE,
            samples,
        ));
        // A stream that died here loses its half second; the next write reopens it.
        stream.drain_to(BUFFERED_PACKETS);
        Ok(())
    }
}

fn refused(error: impl std::fmt::Display) -> SinkError {
    SinkError::ConnectionRefused(error.to_string())
}
