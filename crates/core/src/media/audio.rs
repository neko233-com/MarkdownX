//! Audio probe for the preview UI: duration, stream shape and a bounded
//! waveform. Decoding is capped so a long file cannot stall the preview.
use std::fs::File;
use std::io::ErrorKind;
use std::path::Path;
use anyhow::{Context, Result, ensure};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_DECODE_SECONDS: u64 = 600;
const BLOCK_FRAMES: u64 = 2048;
const MAX_PEAKS: usize = 512;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioInfo {
    pub duration_seconds: Option<f64>,
    pub sample_rate: Option<u32>,
    pub channels: Option<usize>,
    /// Normalized 0..=1 peak per decoded block, downsampled to `MAX_PEAKS`.
    pub peaks: Vec<f32>,
}

pub fn inspect(path: &Path) -> Result<AudioInfo> {
    let meta = std::fs::metadata(path).context("cannot inspect audio")?;
    ensure!(meta.is_file(), "audio is not a regular file");
    ensure!(meta.len() <= MAX_FILE_BYTES, "audio file is too large");
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) { hint.with_extension(extension); }
    let stream = MediaSourceStream::new(Box::new(File::open(path).context("cannot open audio")?), Default::default());
    let probed = symphonia::default::get_probe()
        .format(&hint, stream, &FormatOptions::default(), &MetadataOptions::default())
        .context("unsupported audio format")?;
    let mut format = probed.format;
    let track = format.default_track().context("audio has no default track")?;
    let track_id = track.id;
    let sample_rate = track.codec_params.sample_rate;
    let channels = track.codec_params.channels.map(|channels| channels.count());
    let declared_frames = track.codec_params.n_frames;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .context("unsupported audio codec")?;
    let limit = sample_rate.map(|rate| rate as u64 * MAX_DECODE_SECONDS);
    let mut peaks = vec![];
    let mut decoded_frames = 0u64;
    let mut block_peak = 0.0f32;
    let mut block_frames = 0u64;
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(error)) if error.kind() == ErrorKind::UnexpectedEof => break,
            Err(Error::ResetRequired) => break,
            Err(error) => return Err(error).context("cannot read audio"),
        };
        if packet.track_id() != track_id { continue; }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(Error::DecodeError(_)) => continue,
            Err(Error::IoError(error)) if error.kind() == ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(error).context("cannot decode audio"),
        };
        if decoded.frames() == 0 { continue; }
        let spec = *decoded.spec();
        let count = spec.channels.count().max(1);
        let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        buffer.copy_interleaved_ref(decoded);
        for frame in buffer.samples().chunks(count) {
            let peak = frame.iter().fold(0.0f32, |peak, sample| peak.max(sample.abs()));
            block_peak = block_peak.max(peak);
            block_frames += 1;
            if block_frames >= BLOCK_FRAMES {
                peaks.push(block_peak);
                block_peak = 0.0;
                block_frames = 0;
            }
        }
        decoded_frames += buffer.samples().len() as u64 / count as u64;
        if limit.is_some_and(|limit| decoded_frames >= limit) { break; }
    }
    if block_frames > 0 { peaks.push(block_peak); }
    if peaks.len() > MAX_PEAKS {
        let chunk = peaks.len().div_ceil(MAX_PEAKS);
        peaks = peaks.chunks(chunk).map(|chunk| chunk.iter().copied().fold(0.0, f32::max)).collect();
    }
    ensure!(!peaks.is_empty(), "audio has no decoded samples");
    let duration_seconds = match (declared_frames, sample_rate) {
        (Some(frames), Some(rate)) if rate > 0 => Some(frames as f64 / rate as f64),
        (_, Some(rate)) if rate > 0 => Some(decoded_frames as f64 / rate as f64),
        _ => None,
    };
    Ok(AudioInfo { duration_seconds, sample_rate, channels, peaks })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn wav(samples: &[i16], rate: u32, channels: u16) -> Vec<u8> {
        let data = samples.iter().flat_map(|sample| sample.to_le_bytes()).collect::<Vec<u8>>();
        let mut out = vec![];
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
        out.extend_from_slice(&(channels * 2).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        out
    }

    fn write(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("markdownx-audio-{name}-{}.wav", uuid::Uuid::new_v4()));
        let mut file = File::create(&path).unwrap();
        file.write_all(bytes).unwrap();
        path
    }

    #[test]
    fn reads_duration_shape_and_bounded_waveform() {
        let mut samples = vec![];
        for index in 0..8000 {
            samples.push((f32::sin(index as f32 / 8.0) * 12000.0) as i16);
        }
        let path = write("sine", &wav(&samples, 8000, 1));
        let info = inspect(&path).unwrap();
        assert_eq!(info.sample_rate, Some(8000));
        assert_eq!(info.channels, Some(1));
        assert!((info.duration_seconds.unwrap() - 1.0).abs() < 0.05);
        assert!(!info.peaks.is_empty() && info.peaks.len() <= MAX_PEAKS);
        assert!(info.peaks.iter().all(|peak| (0.0..=1.0).contains(peak)));
        assert!(info.peaks.iter().copied().fold(0.0f32, f32::max) > 0.3);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn decodes_a_real_mp3_waveform() {
        let path = std::env::temp_dir().join(format!("markdownx-audio-{}.mp3", uuid::Uuid::new_v4()));
        std::fs::write(&path, include_bytes!("../../tests/fixtures/tone.mp3")).unwrap();
        let info = inspect(&path).unwrap();
        assert!(info.sample_rate.is_some() && info.channels.is_some());
        assert!(info.duration_seconds.is_some_and(|duration| duration > 0.1));
        assert!(!info.peaks.is_empty() && info.peaks.iter().any(|peak| *peak > 0.1));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejects_empty_or_unrelated_files() {
        let path = write("empty", &[]);
        assert!(inspect(&path).is_err());
        std::fs::write(&path, b"not audio").unwrap();
        assert!(inspect(&path).is_err());
        assert!(inspect(&std::env::temp_dir().join("missing-audio.wav")).is_err());
        let _ = std::fs::remove_file(path);
    }
}
