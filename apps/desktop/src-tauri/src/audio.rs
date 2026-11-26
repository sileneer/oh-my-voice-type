//! Microphone capture. The cpal stream is owned by a dedicated audio thread
//! (WASAPI streams are not `Send` on Windows). Pending sessions additionally
//! stream PCM to a WAV file, flushing and syncing approximately once per second.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ovt_core::Result;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct CaptureInfo {
    pub sample_rate: u32,
}

enum AudioCmd {
    Start {
        device: Option<String>,
        pending_path: Option<PathBuf>,
        reply: Sender<Result<CaptureInfo>>,
    },
    Stop {
        reply: Sender<(Vec<f32>, u32)>,
    },
    Level {
        reply: Sender<f32>,
    },
    Shutdown,
}

struct SharedLevel(AtomicU32);

#[derive(Clone)]
pub struct AudioHandle {
    sim: Sender<AudioCmd>,
    pending_frames: Arc<AtomicU64>,
    pending_error: Arc<Mutex<Option<String>>>,
}

impl Default for AudioHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioHandle {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        let pending_frames = Arc::new(AtomicU64::new(0));
        let pending_error = Arc::new(Mutex::new(None));
        let frames_for_thread = pending_frames.clone();
        let error_for_thread = pending_error.clone();
        std::thread::Builder::new()
            .name("ovt-audio".into())
            .spawn(move || audio_thread(rx, frames_for_thread, error_for_thread))
            .expect("audio thread");
        Self {
            sim: tx,
            pending_frames,
            pending_error,
        }
    }

    pub fn start_pending(&self, device: Option<&str>, path: PathBuf) -> Result<CaptureInfo> {
        self.start_internal(device, Some(path))
    }

    fn start_internal(
        &self,
        device: Option<&str>,
        pending_path: Option<PathBuf>,
    ) -> Result<CaptureInfo> {
        let (reply, rx) = channel();
        self.pending_frames.store(0, Ordering::SeqCst);
        *self
            .pending_error
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = None;
        self.sim
            .send(AudioCmd::Start {
                device: device.map(str::to_string),
                pending_path,
                reply,
            })
            .map_err(|_| ovt_core::Error::Audio("audio thread unavailable".into()))?;
        rx.recv_timeout(Duration::from_secs(10))
            .map_err(|_| ovt_core::Error::Audio("audio start timed out".into()))?
    }

    pub fn stop(&self) -> (Vec<f32>, u32) {
        let (reply, rx) = channel();
        if self.sim.send(AudioCmd::Stop { reply }).is_err() {
            return (Vec::new(), 16_000);
        }
        rx.recv_timeout(Duration::from_secs(10)).unwrap_or_default()
    }

    pub fn pending_frames(&self) -> u64 {
        self.pending_frames.load(Ordering::SeqCst)
    }

    pub fn pending_error(&self) -> Option<String> {
        self.pending_error
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub fn level(&self) -> f32 {
        let (reply, rx) = channel();
        if self.sim.send(AudioCmd::Level { reply }).is_err() {
            return 0.0;
        }
        rx.recv_timeout(Duration::from_millis(100)).unwrap_or(0.0)
    }

    pub fn shutdown(&self) {
        let _ = self.sim.send(AudioCmd::Shutdown);
    }

    pub fn list_devices() -> Vec<String> {
        cpal::default_host()
            .input_devices()
            .map(|devices| devices.filter_map(|device| device.name().ok()).collect())
            .unwrap_or_default()
    }
}

fn audio_thread(
    rx: Receiver<AudioCmd>,
    pending_frames: Arc<AtomicU64>,
    pending_error: Arc<Mutex<Option<String>>>,
) {
    let shared_level = Arc::new(SharedLevel(AtomicU32::new(0)));
    let samples: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let mut stream: Option<cpal::Stream> = None;
    let mut writer: Option<ovt_core::wav::StreamingWavWriter> = None;
    let mut writer_rx: Option<Receiver<Vec<f32>>> = None;
    let mut writer_tx: Option<Sender<Vec<f32>>> = None;
    let mut last_flush = Instant::now();
    let mut native_rate = 16_000u32;
    let mut recording = false;

    loop {
        let command = match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(command) => Some(command),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        if let Some(command) = command {
            match command {
                AudioCmd::Start {
                    device,
                    pending_path,
                    reply,
                } => {
                    let result = (|| -> Result<CaptureInfo> {
                        if recording {
                            return Err(ovt_core::Error::Audio(
                                "a recording is already active".into(),
                            ));
                        }
                        let host = cpal::default_host();
                        let device = match device.as_deref() {
                            Some(requested) => host
                                .input_devices()
                                .map_err(|error| ovt_core::Error::Audio(error.to_string()))?
                                .find(|device| {
                                    device
                                        .name()
                                        .map(|candidate| candidate == requested)
                                        .unwrap_or(false)
                                })
                                .or_else(|| host.default_input_device()),
                            None => host.default_input_device(),
                        }
                        .ok_or_else(|| ovt_core::Error::Audio("no microphone found".into()))?;
                        let config = device
                            .default_input_config()
                            .map_err(|error| ovt_core::Error::Audio(error.to_string()))?;
                        native_rate = config.sample_rate().0;
                        let channels = config.channels() as usize;
                        samples
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .clear();

                        let audio_tx = if let Some(path) = pending_path {
                            let (tx, rx) = channel();
                            writer = Some(ovt_core::wav::StreamingWavWriter::create(
                                &path,
                                native_rate,
                            )?);
                            writer_rx = Some(rx);
                            writer_tx = Some(tx.clone());
                            Some(tx)
                        } else {
                            None
                        };
                        let buffer = samples.clone();
                        let level = shared_level.clone();
                        let stream_result = device.build_input_stream(
                            &config.config(),
                            move |data: &[f32], _| {
                                let frames = data.len() / channels.max(1);
                                if frames == 0 {
                                    return;
                                }
                                let mut mono = Vec::with_capacity(frames);
                                let mut peak = 0.0f32;
                                for frame in 0..frames {
                                    let mut sum = 0.0;
                                    for channel in 0..channels {
                                        sum += data[frame * channels + channel];
                                    }
                                    let value = sum / channels as f32;
                                    peak = peak.max(value.abs());
                                    mono.push(value);
                                }
                                level.0.store(peak.to_bits(), Ordering::Relaxed);
                                if let Ok(mut buffer) = buffer.lock() {
                                    buffer.extend_from_slice(&mono);
                                }
                                if let Some(tx) = &audio_tx {
                                    let _ = tx.send(mono);
                                }
                            },
                            |error| eprintln!("audio stream error: {error}"),
                            None,
                        );
                        let new_stream = stream_result
                            .map_err(|error| ovt_core::Error::Audio(error.to_string()))?;
                        new_stream
                            .play()
                            .map_err(|error| ovt_core::Error::Audio(error.to_string()))?;
                        stream = Some(new_stream);
                        recording = true;
                        last_flush = Instant::now();
                        Ok(CaptureInfo {
                            sample_rate: native_rate,
                        })
                    })();
                    if result.is_err() {
                        writer = None;
                        writer_rx = None;
                        writer_tx = None;
                    }
                    let _ = reply.send(result);
                }
                AudioCmd::Stop { reply } => {
                    let was_recording = recording;
                    recording = false;
                    stream = None;
                    writer_tx = None;
                    if let Some(mut writer) = writer.take() {
                        if let Some(receiver) = writer_rx.take() {
                            for chunk in receiver.try_iter() {
                                if let Err(error) = writer.write_samples(&chunk) {
                                    set_pending_error(&pending_error, error);
                                }
                            }
                        }
                        match writer.finish() {
                            Ok(frames) => pending_frames.store(frames, Ordering::SeqCst),
                            Err(error) => set_pending_error(&pending_error, error),
                        }
                    }
                    let output = if was_recording {
                        samples
                            .lock()
                            .map(|samples| samples.clone())
                            .unwrap_or_default()
                    } else {
                        Vec::new()
                    };
                    let _ = reply.send((output, native_rate));
                }
                AudioCmd::Level { reply } => {
                    let _ = reply.send(f32::from_bits(shared_level.0.load(Ordering::Relaxed)));
                }
                AudioCmd::Shutdown => break,
            }
        }

        if recording {
            if let Some(receiver) = writer_rx.as_ref() {
                for chunk in receiver.try_iter() {
                    if let Some(writer) = writer.as_mut() {
                        if let Err(error) = writer.write_samples(&chunk) {
                            set_pending_error(&pending_error, error);
                        }
                    }
                }
            }
            if last_flush.elapsed() >= Duration::from_secs(1) {
                if let Some(writer) = writer.as_mut() {
                    if let Err(error) = writer.flush() {
                        set_pending_error(&pending_error, error);
                    }
                }
                last_flush = Instant::now();
            }
        }
    }
    // Best effort final sync if the host is shutting down unexpectedly.
    if let Some(mut writer) = writer.take() {
        if let Some(receiver) = writer_rx.take() {
            for chunk in receiver.try_iter() {
                let _ = writer.write_samples(&chunk);
            }
        }
        if let Ok(frames) = writer.finish() {
            pending_frames.store(frames, Ordering::SeqCst);
        }
    }
}

fn set_pending_error(slot: &Mutex<Option<String>>, error: ovt_core::Error) {
    *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(error.to_string());
}

pub fn to_wav(samples: &[f32], rate: u32) -> (Vec<u8>, f32) {
    ovt_core::dictation::prepare_audio(samples, rate)
}
