//! Minimal video metadata for the preview card. Only the ISO base media
//! (MP4/MOV/M4V) box headers are read; other containers fall back to size.
use std::path::Path;
use anyhow::{Context, Result, ensure};

const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_DEPTH: usize = 8;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VideoInfo {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_seconds: Option<f64>,
    pub bytes: u64,
}

pub fn inspect(path: &Path) -> Result<VideoInfo> {
    let meta = std::fs::metadata(path).context("cannot inspect video")?;
    ensure!(meta.is_file(), "video is not a regular file");
    ensure!(meta.len() <= MAX_FILE_BYTES, "video file is too large");
    let mut info = VideoInfo { bytes: meta.len(), ..Default::default() };
    if !matches!(path.extension().and_then(|value| value.to_str()).map(str::to_ascii_lowercase).as_deref(), Some("mp4" | "m4v" | "mov")) {
        return Ok(info);
    }
    let bytes = std::fs::read(path).context("cannot read video")?;
    let mut timescale = None;
    let mut duration = None;
    let mut width = None;
    let mut height = None;
    walk(&bytes, 0, bytes.len(), 0, &mut |kind, payload| match kind {
        b"mvhd" => {
            if let Some((scale, length)) = movie_header(payload) {
                timescale = Some(scale);
                duration = Some(length);
            }
        }
        b"tkhd" => {
            if let Some((w, h)) = track_size(payload) {
                width = Some(w);
                height = Some(h);
            }
        }
        _ => {}
    });
    info.width = width;
    info.height = height;
    info.duration_seconds = match (timescale, duration) {
        (Some(scale), Some(length)) if scale > 0 => Some(length as f64 / scale as f64),
        _ => None,
    };
    Ok(info)
}

fn walk(bytes: &[u8], mut offset: usize, end: usize, depth: usize, found: &mut impl FnMut(&[u8; 4], &[u8])) {
    if depth > MAX_DEPTH { return; }
    while offset + 8 <= end {
        let size = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        let kind: [u8; 4] = bytes[offset + 4..offset + 8].try_into().unwrap();
        let (header, size) = match size {
            0 => (8, end - offset),
            1 if offset + 16 <= end => (16, u64::from_be_bytes(bytes[offset + 8..offset + 16].try_into().unwrap()) as usize),
            size if size >= 8 => (8, size),
            _ => return,
        };
        if size < header || offset + size > end { return; }
        let payload = &bytes[offset + header..offset + size];
        if matches!(&kind, b"moov" | b"trak" | b"mdia" | b"minf") {
            walk(bytes, offset + header, offset + size, depth + 1, found);
        } else {
            found(&kind, payload);
        }
        offset += size;
    }
}

fn movie_header(payload: &[u8]) -> Option<(u32, u64)> {
    let version = *payload.first()?;
    if version == 1 {
        Some((u32::from_be_bytes(payload.get(20..24)?.try_into().ok()?), u64::from_be_bytes(payload.get(24..32)?.try_into().ok()?)))
    } else {
        Some((u32::from_be_bytes(payload.get(12..16)?.try_into().ok()?), u32::from_be_bytes(payload.get(16..20)?.try_into().ok()?) as u64))
    }
}

fn track_size(payload: &[u8]) -> Option<(u32, u32)> {
    let len = payload.len();
    if len < 8 { return None; }
    let width = u32::from_be_bytes(payload[len - 8..len - 4].try_into().ok()?);
    let height = u32::from_be_bytes(payload[len - 4..].try_into().ok()?);
    let width = width >> 16;
    let height = height >> 16;
    (width > 0 && height > 0).then_some((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    fn boxed(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(payload);
        out
    }

    fn mvhd(timescale: u32, duration: u32) -> Vec<u8> {
        let mut payload = vec![0u8; 20];
        payload[12..16].copy_from_slice(&timescale.to_be_bytes());
        payload[16..20].copy_from_slice(&duration.to_be_bytes());
        boxed(b"mvhd", &payload)
    }

    fn tkhd(width: u32, height: u32) -> Vec<u8> {
        let mut payload = vec![0u8; 84];
        payload[76..80].copy_from_slice(&(width << 16).to_be_bytes());
        payload[80..84].copy_from_slice(&(height << 16).to_be_bytes());
        boxed(b"tkhd", &payload)
    }

    #[test]
    fn reads_iso_box_dimensions_and_duration() {
        let mut moov = mvhd(1000, 5000);
        let trak = tkhd(1920, 1080);
        moov.extend(boxed(b"trak", &trak));
        let bytes = boxed(b"moov", &moov);
        let path = std::env::temp_dir().join(format!("markdownx-video-{}.mp4", uuid::Uuid::new_v4()));
        File::create(&path).unwrap().write_all(&bytes).unwrap();
        let info = inspect(&path).unwrap();
        assert_eq!((info.width, info.height), (Some(1920), Some(1080)));
        assert_eq!(info.duration_seconds, Some(5.0));
        assert_eq!(info.bytes, bytes.len() as u64);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn unknown_containers_only_report_size() {
        let path = std::env::temp_dir().join(format!("markdownx-video-{}.webm", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"not a box").unwrap();
        let info = inspect(&path).unwrap();
        assert_eq!((info.width, info.height, info.duration_seconds), (None, None, None));
        assert_eq!(info.bytes, 9);
        let _ = std::fs::remove_file(path);
        assert!(inspect(&std::env::temp_dir().join("missing-video.mp4")).is_err());
    }
}
