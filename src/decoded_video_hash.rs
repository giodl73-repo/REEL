//! Exact raw-video SHA-256 without carrying uncompressed frames over OS pipes.
use anyhow::{Context, Result, bail};
use std::{
    fs,
    io::{Read, Seek},
    path::Path,
    process::Stdio,
};

#[derive(Debug)]
pub(crate) struct Hashes {
    pub values: Vec<String>,
    pub bytes: u64,
}

pub(crate) fn frame_bytes(path: &Path) -> Result<u64> {
    let output = crate::episode_conform::command("ffprobe")
        .args([
            "-v",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "json",
        ])
        .arg(path)
        .output()?;
    if !output.status.success() {
        bail!("decoded video geometry probe failed");
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let stream = &value["streams"][0];
    let width = stream["width"]
        .as_u64()
        .filter(|v| *v > 0)
        .context("video width missing")?;
    let height = stream["height"]
        .as_u64()
        .filter(|v| *v > 0)
        .context("video height missing")?;
    width
        .checked_mul(height)
        .and_then(|v| v.checked_mul(3))
        .context("video frame byte overflow")
}

/// Hash the same packed raw-video bytes as the historical pipe decoder. The
/// optional cuts count decoded frames, never timestamps or keyframe seeks.
pub(crate) fn hash(
    path: &Path,
    pixel_format: &str,
    filter: Option<&str>,
    frame_counts: Option<&[u64]>,
) -> Result<Hashes> {
    if frame_counts.is_some_and(|counts| counts.is_empty() || counts.contains(&0)) {
        bail!("decoded video segments must contain frames");
    }
    let temp = tempfile::tempdir()?;
    let counter = temp.path().join("frames.crc");
    let first = temp.path().join("part-000000.sha256");
    let pattern = temp.path().join("part-%06d.sha256");
    let filter = match filter {
        Some(filter) => format!("[0:v:0]{filter},split=2[digest][counter]"),
        None => "[0:v:0]split=2[digest][counter]".into(),
    };
    let mut command = crate::episode_conform::command("ffmpeg");
    command
        .args([
            "-v",
            "error",
            "-nostdin",
            "-protocol_whitelist",
            "file,pipe",
            "-i",
        ])
        .arg(path)
        .args([
            "-filter_complex",
            &filter,
            "-map",
            "[digest]",
            "-fps_mode",
            "passthrough",
            "-c:v",
            "rawvideo",
            "-threads:v",
            "1",
            "-pix_fmt",
            pixel_format,
        ]);
    if let Some(counts) = frame_counts.filter(|counts| counts.len() > 1) {
        let mut cursor = 0u64;
        let mut cuts = Vec::new();
        for count in &counts[..counts.len() - 1] {
            cursor = cursor.checked_add(*count).context("video cut overflow")?;
            cuts.push(cursor.to_string());
        }
        command
            .args([
                "-f",
                "segment",
                "-reference_stream",
                "v:0",
                "-segment_frames",
            ])
            .arg(cuts.join(","))
            .args([
                "-segment_format",
                "hash",
                "-segment_format_options",
                "hash=sha256",
            ])
            .arg(pattern);
    } else {
        command.args(["-f", "hash", "-hash", "sha256"]).arg(first);
    }
    command
        .args([
            "-map",
            "[counter]",
            "-fps_mode",
            "passthrough",
            "-c:v",
            "rawvideo",
            "-threads:v",
            "1",
            "-pix_fmt",
            pixel_format,
            "-f",
            "framecrc",
        ])
        .arg(&counter);
    let mut errors = tempfile::tempfile()?;
    let status = command
        .stdout(Stdio::null())
        .stderr(errors.try_clone()?)
        .status()?;
    if !status.success() {
        errors.rewind()?;
        let mut message = String::new();
        errors.take(8192).read_to_string(&mut message)?;
        bail!("decoded video hashing failed: {message}");
    }
    let mut frames = 0u64;
    let mut bytes = 0u64;
    for line in fs::read_to_string(counter)?.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let cells = line.split(',').map(str::trim).collect::<Vec<_>>();
        if cells.len() < 6 || cells[0] != "0" {
            bail!("invalid raw-video packet counter");
        }
        let size = cells[4].parse::<u64>()?;
        if size == 0 {
            bail!("empty raw-video packet");
        }
        bytes = bytes
            .checked_add(size)
            .context("decoded video byte overflow")?;
        frames = frames
            .checked_add(1)
            .context("decoded video frame overflow")?;
    }
    if let Some(counts) = frame_counts {
        let expected = counts
            .iter()
            .try_fold(0u64, |sum, count| sum.checked_add(*count))
            .context("decoded video frame overflow")?;
        if frames < expected {
            bail!("decoded segment is short");
        }
        if frames > expected {
            bail!("decoded media has extra content");
        }
    }
    let count = frame_counts.map_or(1, <[u64]>::len);
    let mut values = Vec::with_capacity(count);
    for index in 0..count {
        let text = fs::read_to_string(temp.path().join(format!("part-{index:06}.sha256")))?;
        let digest = text
            .trim()
            .strip_prefix("SHA256=")
            .context("invalid video SHA-256 result")?;
        if digest.len() != 64 || !digest.bytes().all(|c| c.is_ascii_hexdigit()) {
            bail!("invalid video SHA-256 digest");
        }
        values.push(digest.to_ascii_lowercase());
    }
    Ok(Hashes { values, bytes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn digest(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    fn legacy_bytes(path: &Path, filter: Option<&str>, format: &str) -> Vec<u8> {
        let mut command = crate::episode_conform::command("ffmpeg");
        command.args(["-v", "error", "-nostdin", "-i"]).arg(path);
        if let Some(filter) = filter {
            command.args(["-vf", filter]);
        }
        let result = command
            .args([
                "-map",
                "0:v:0",
                "-fps_mode",
                "passthrough",
                "-pix_fmt",
                format,
                "-f",
                "rawvideo",
                "-",
            ])
            .output()
            .unwrap();
        assert!(result.status.success());
        result.stdout
    }

    #[test]
    fn native_hashes_match_raw_stream_at_variable_frame_cuts_and_reject_wrong_lengths() {
        let temp = tempfile::tempdir().unwrap();
        for codec in ["ffv1", "libx264"] {
            let path = temp.path().join(format!("{codec}.mkv"));
            let mut command = crate::episode_conform::command("ffmpeg");
            command.args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=s=64x48:r=24:d=1",
                "-c:v",
                codec,
                "-pix_fmt",
                "yuv444p",
            ]);
            if codec == "libx264" {
                command.args(["-crf", "0", "-bf", "0"]);
            }
            assert!(command.arg(&path).status().unwrap().success());
            let bytes = legacy_bytes(&path, None, "yuv444p");
            let expected = digest(&bytes);
            let whole = hash(&path, "yuv444p", None, None).unwrap();
            assert_eq!(whole.values, [expected]);
            assert_eq!(whole.bytes, bytes.len() as u64);
            let counts = [3, 1, 13, 7];
            let segments = hash(&path, "yuv444p", None, Some(&counts)).unwrap();
            assert_eq!(segments.bytes, bytes.len() as u64);
            let mut cursor = 0;
            for (index, frames) in counts.iter().enumerate() {
                let end = cursor + *frames as usize * 64 * 48 * 3;
                assert_eq!(segments.values[index], digest(&bytes[cursor..end]));
                cursor = end;
            }
            assert!(
                hash(&path, "yuv444p", None, Some(&[23]))
                    .unwrap_err()
                    .to_string()
                    .contains("extra content")
            );
            assert!(
                hash(&path, "yuv444p", None, Some(&[25]))
                    .unwrap_err()
                    .to_string()
                    .contains("short")
            );
            let filter = "scale=32:24:flags=lanczos";
            let rgb = legacy_bytes(&path, Some(filter), "rgb24");
            let filtered = hash(&path, "rgb24", Some(filter), None).unwrap();
            assert_eq!(filtered.bytes, rgb.len() as u64);
            assert_eq!(filtered.values, [digest(&rgb)]);
        }
    }
}
