//! Minimal WAV writer for 16-bit PCM mono audio (ASR payload format).
//! Older/newer providers accept WAV/FLAC/MP3; WAV keeps v1 dependency-free.

use crate::error::{Error, Result};

pub const TARGET_SAMPLE_RATE: u32 = 16_000;

/// Simple resampler sample-by-sample (linear interpolation). Good enough as an
/// interim until a proper resampler is introduced; quality loss is negligible
/// for speech at 16 kHz.
pub fn resample(samples: &[f32], in_rate: u32, out_rate: u32) -> Vec<f32> {
    if in_rate == out_rate || samples.is_empty() {
        return samples.to_vec();
    }
    let ratio = out_rate as f64 / in_rate as f64;
    let out_len = ((samples.len() as f64) * ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let pos = i as f64 / ratio;
        let i0 = pos.floor() as usize;
        let frac = pos - i0 as f64;
        let s0 = samples[i0.min(samples.len() - 1)];
        let s1 = samples[(i0 + 1).min(samples.len() - 1)];
        out.push(s0 + (s1 - s0) * frac as f32);
    }
    out
}

pub fn encode_wav_16k_mono(samples: &[f32], sample_rate: u32) -> Result<Vec<u8>> {
    if sample_rate == 0 {
        return Err(Error::Audio("sample rate must be > 0".into()));
    }
    Ok(encode_wav(samples, sample_rate))
}

pub fn encode_wav_mono(samples: &[f32], rate: u32) -> Vec<u8> {
    encode_wav(samples, rate)
}

fn encode_wav(samples: &[f32], rate: u32) -> Vec<u8> {
    let bits = 16u16;
    let channels = 1u16;
    let data_len = (samples.len() * 2) as u32;
    let byte_rate = rate * u32::from(channels) * u32::from(bits) / 8;
    let block_align = channels * bits / 8;

    let mut out = Vec::with_capacity(44 + samples.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for &s in samples {
        let clamped = s.clamp(-1.0, 1.0);
        let v = (clamped * i16::MAX as f32) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// RMS in dBFS over a window, used by the silence watchdog.
pub fn peak_dbfs(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return -120.0;
    }
    samples
        .iter()
        .fold(0.0f32, |m, s| m.max(s.abs()))
        .max(1e-9)
        .log10()
        * 20.0
}

/// Trim leading/trailing near-silence. Keeps a 150ms air cushion around speech.
pub fn trim_silence(samples: Vec<f32>, rate: u32, floor_db: f32) -> (Vec<f32>, bool) {
    if samples.len() < (rate / 10) as usize {
        return (samples, false);
    }
    let win = (rate / 100) as usize; // 10ms windows
    let total = samples.len() / win;
    let mut first = None;
    let mut last = 0usize;
    for i in 0..total {
        let s = &samples[i * win..(i + 1) * win];
        if peak_dbfs(s) > floor_db {
            first.get_or_insert(i);
            last = i;
        }
    }
    let Some(first) = first else {
        return (Vec::new(), false);
    };
    let pad = (rate / 6) as usize; // 166ms
    let start = (first * win).saturating_sub(pad);
    let end = ((last + 1) * win + pad).min(samples.len());
    let trimmed: Vec<f32> = samples[start..end].to_vec();
    let has_speech = peak_dbfs(&trimmed) > floor_db;
    (trimmed, has_speech)
}

/// Incremental mono PCM16 writer used by the durable pending recorder. The
/// header is patched on every flush so a killed process leaves a readable WAV
/// containing all fully synced frames.
pub struct StreamingWavWriter {
    file: std::fs::File,
    sample_rate: u32,
    frames: u64,
    finished: bool,
}

impl StreamingWavWriter {
    pub fn create(path: &std::path::Path, sample_rate: u32) -> Result<Self> {
        if sample_rate == 0 {
            return Err(Error::Audio("sample rate must be > 0".into()));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| Error::Audio(format!("create audio directory: {error}")))?;
        }
        let mut file = std::fs::File::create(path)
            .map_err(|error| Error::Audio(format!("create pending audio: {error}")))?;
        use std::io::Write;
        file.write_all(&header(sample_rate, 0))
            .map_err(|error| Error::Audio(format!("write WAV header: {error}")))?;
        file.sync_all()
            .map_err(|error| Error::Audio(format!("sync WAV header: {error}")))?;
        Ok(Self {
            file,
            sample_rate,
            frames: 0,
            finished: false,
        })
    }

    pub fn write_samples(&mut self, samples: &[f32]) -> Result<()> {
        if self.finished {
            return Err(Error::Audio("WAV writer is already finished".into()));
        }
        use std::io::Write;
        let mut bytes = Vec::with_capacity(samples.len() * 2);
        for sample in samples {
            let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        self.file
            .write_all(&bytes)
            .map_err(|error| Error::Audio(format!("write pending audio: {error}")))?;
        self.frames = self.frames.saturating_add(samples.len() as u64);
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        use std::io::{Seek, SeekFrom, Write};
        self.file
            .flush()
            .map_err(|error| Error::Audio(format!("flush pending audio: {error}")))?;
        self.file
            .seek(SeekFrom::Start(0))
            .map_err(|error| Error::Audio(format!("seek WAV header: {error}")))?;
        self.file
            .write_all(&header(self.sample_rate, self.frames))
            .map_err(|error| Error::Audio(format!("patch WAV header: {error}")))?;
        self.file
            .seek(SeekFrom::End(0))
            .map_err(|error| Error::Audio(format!("seek WAV end: {error}")))?;
        self.file
            .sync_all()
            .map_err(|error| Error::Audio(format!("sync pending audio: {error}")))?;
        Ok(())
    }

    pub fn finish(mut self) -> Result<u64> {
        self.flush()?;
        self.finished = true;
        Ok(self.frames)
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }
}

fn header(sample_rate: u32, frames: u64) -> [u8; 44] {
    let data_len = (frames.saturating_mul(2)).min(u32::MAX as u64) as u32;
    let mut out = [0u8; 44];
    out[0..4].copy_from_slice(b"RIFF");
    out[4..8].copy_from_slice(&(36u32.saturating_add(data_len)).to_le_bytes());
    out[8..12].copy_from_slice(b"WAVE");
    out[12..16].copy_from_slice(b"fmt ");
    out[16..20].copy_from_slice(&16u32.to_le_bytes());
    out[20..22].copy_from_slice(&1u16.to_le_bytes());
    out[22..24].copy_from_slice(&1u16.to_le_bytes());
    out[24..28].copy_from_slice(&sample_rate.to_le_bytes());
    out[28..32].copy_from_slice(&(sample_rate.saturating_mul(2)).to_le_bytes());
    out[32..34].copy_from_slice(&2u16.to_le_bytes());
    out[34..36].copy_from_slice(&16u16.to_le_bytes());
    out[36..40].copy_from_slice(b"data");
    out[40..44].copy_from_slice(&data_len.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_wav_flushes_readable_prefix() {
        let path = std::env::temp_dir().join(format!("ovt-stream-{}.wav", uuid::Uuid::new_v4()));
        {
            let mut writer = StreamingWavWriter::create(&path, 16_000).unwrap();
            writer.write_samples(&[0.0, 0.5, -0.5]).unwrap();
            writer.flush().unwrap();
            assert_eq!(writer.frames(), 3);
        }
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 6);
        assert_eq!(bytes.len(), 50);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn wav_header_is_correct() {
        let samples = vec![0.0f32; 1600];
        let wav = encode_wav(&samples, 16000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(wav.len(), 44 + 3200);
    }

    #[test]
    fn resample_doubles_rate() {
        let out = resample(&[0.0, 1.0, 0.0, 1.0], 16000, 32000);
        assert_eq!(out.len(), 8);
    }

    #[test]
    fn trimming_removes_edges() {
        let rate = 16000u32;
        let mut s = vec![0.0f32; rate as usize / 2]; // 0.5 s silence
        s.extend(vec![0.2f32; rate as usize / 2]); // 0.5 s speech
        s.extend(vec![0.0f32; rate as usize / 2]);
        let (trimmed, has) = trim_silence(s, rate, -40.0);
        assert!(has);
        assert!(trimmed.len() < rate as usize);
        assert!(trimmed.len() > rate as usize / 3);
    }

    #[test]
    fn digital_silence_reports_no_speech() {
        let (trimmed, has) = trim_silence(vec![0.0f32; 16000], 16000, -40.0);
        assert!(!has);
        assert!(trimmed.is_empty());
    }
}
