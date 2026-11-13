//! Bind measured audio anchors to the existing persistent browser frame event.
use super::{BrowserCompositionManifest, HashedAsset};
use crate::{presentation_clock::SourceClock, production};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// Reads only the explicitly named small clock and its declared evidence.
/// Audio stays a separately held primary; these data grant no review authority.
pub(crate) fn source_clock_event(
    manifest: &BrowserCompositionManifest,
    mut read: impl FnMut(&HashedAsset, u64) -> Result<Vec<u8>>,
) -> Result<Option<Value>> {
    let Some(reference) = &manifest.source_clock else {
        return Ok(None);
    };
    super::validate_manifest(manifest.clone())?;
    let raw = read(reference, 65_536)?;
    if raw.len() > 65_536 || production::sha256_bytes(&raw) != reference.sha256 {
        bail!("browser source clock bytes differ");
    }
    let clock: SourceClock = serde_json::from_slice(&raw)?;
    let audio = manifest
        .audio
        .as_ref()
        .context("browser source clock needs audio")?;
    if clock.schema != "reel.presentation-source-clock.v1"
        || clock.source_sha256 != audio.sha256
        || !(8_000..=384_000).contains(&clock.sample_rate)
        || clock.total_samples == 0
        || clock.total_samples > 9_007_199_254_740_991
        || manifest.frame_count as u128 * clock.sample_rate as u128
            > clock.total_samples as u128 * manifest.fps as u128
        || clock.anchors.is_empty()
        || clock.anchors.len() > 4096
        || clock.evidence.bytes == 0
        || clock.evidence.bytes > 1_048_576
    {
        bail!("browser source clock differs from selected audio or frame scope");
    }
    let evidence_path = clock
        .evidence
        .path
        .to_str()
        .context("invalid clock evidence path")?;
    let evidence = manifest
        .assets
        .iter()
        .find(|a| a.path == evidence_path)
        .context("browser clock evidence is not declared")?;
    if evidence.sha256 != clock.evidence.sha256 {
        bail!("browser clock evidence subject differs");
    }
    let evidence_raw = read(evidence, clock.evidence.bytes)?;
    if evidence_raw.len() as u64 != clock.evidence.bytes
        || production::sha256_bytes(&evidence_raw) != clock.evidence.sha256
    {
        bail!("browser clock evidence bytes differ");
    }
    let mut ids = BTreeSet::new();
    let (start, end) = if let Some(window) = &manifest.audio_window {
        if clock.sample_rate != 48_000 || clock.total_samples != window.source_samples {
            bail!("exact PCM window differs from original measured source clock");
        }
        (window.start_sample, window.end_sample)
    } else {
        (0, clock.total_samples)
    };
    let mut previous = None;
    let mut frames = Vec::new();
    for anchor in &clock.anchors {
        if anchor.id.trim().is_empty()
            || anchor.id.len() > 256
            || anchor.id.chars().any(char::is_control)
            || !ids.insert(&anchor.id)
            || anchor.sample < start
            || anchor.sample > end
            || previous.is_some_and(|n| anchor.sample < n)
        {
            bail!("invalid browser source clock anchor");
        }
        previous = Some(anchor.sample);
        // Same single rounding of shared boundaries as still-sequence clocks.
        frames.push(
            (((anchor.sample - start) as u128 * manifest.fps as u128
                + clock.sample_rate as u128 / 2)
                / clock.sample_rate as u128) as u64,
        );
    }
    Ok(Some(json!({"sourceSha256":clock.source_sha256,
        "sampleRate":clock.sample_rate,"totalSamples":clock.total_samples,
        "anchorIds":clock.anchors.iter().map(|a| &a.id).collect::<Vec<_>>(),
        "anchorSamples":clock.anchors.iter().map(|a| a.sample).collect::<Vec<_>>(),
        "anchorFrames":frames})))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn fixture(
        change: impl FnOnce(&mut Value),
    ) -> (BrowserCompositionManifest, BTreeMap<String, Vec<u8>>) {
        let evidence = b"measured timing, not human approval".to_vec();
        let mut clock = json!({"schema":"reel.presentation-source-clock.v1",
            "source_sha256":"a".repeat(64),"sample_rate":48000,"total_samples":4320000,
            "evidence":{"path":"evidence.json","sha256":production::sha256_bytes(&evidence),"bytes":evidence.len()},
            "anchors":[{"id":"pickup","sample":0},{"id":"first","sample":1952},
                {"id":"second","sample":231992},{"id":"third","sample":473912},
                {"id":"end","sample":672000}]});
        change(&mut clock);
        let raw = serde_json::to_vec(&clock).unwrap();
        let clock_ref = HashedAsset {
            path: "clock.json".into(),
            sha256: production::sha256_bytes(&raw),
        };
        let evidence_ref = HashedAsset {
            path: "evidence.json".into(),
            sha256: production::sha256_bytes(&evidence),
        };
        let manifest = BrowserCompositionManifest {
            schema: super::super::MANIFEST_SCHEMA.into(),
            source: HashedAsset {
                path: "opening.svg".into(),
                sha256: "b".repeat(64),
            },
            assets: vec![clock_ref.clone(), evidence_ref],
            width: 8,
            height: 8,
            fps: 24,
            frame_count: 336,
            dynamic_regions: vec![],
            audio: Some(HashedAsset {
                path: "audio.wav".into(),
                sha256: "a".repeat(64),
            }),
            source_clock: Some(clock_ref),
            audio_window: None,
            lossless_master: None,
        };
        (
            manifest,
            BTreeMap::from([
                ("clock.json".into(), raw),
                ("evidence.json".into(), evidence),
            ]),
        )
    }

    fn compile(
        manifest: &BrowserCompositionManifest,
        files: &BTreeMap<String, Vec<u8>>,
    ) -> Result<Option<Value>> {
        source_clock_event(manifest, |asset, bound| {
            let raw = files.get(&asset.path).context("undeclared fixture read")?;
            if raw.len() as u64 > bound {
                bail!("fixture bound exceeded");
            }
            Ok(raw.clone())
        })
    }

    #[test]
    fn measured_clock_rounds_shared_anchors_and_keeps_excerpt_on_full_audio() {
        let (manifest, files) = fixture(|_| {});
        let event = compile(&manifest, &files).unwrap().unwrap();
        assert_eq!(event["anchorFrames"], json!([0, 1, 116, 237, 336]));
        assert_eq!(event["totalSamples"], 4320000);
        let script = super::super::frame_clock_with_source(24, Some(&event));
        assert!(script.contains("sourceClock:"));
        assert!(script.contains("anchorFrames"));
        assert!(script.contains("frame:n,time:n/fps,fps"));
        let mut old = manifest.clone();
        old.source_clock = None;
        assert!(
            source_clock_event(&old, |_, _| panic!("implicit clock read"))
                .unwrap()
                .is_none()
        );
        assert!(
            !serde_json::to_value(&old)
                .unwrap()
                .as_object()
                .unwrap()
                .contains_key("source_clock")
        );
        assert!(!super::super::frame_clock_with_source(24, None).contains("sourceClock"));
    }

    #[test]
    fn exact_audio_window_binds_full_source_and_offsets_anchor_frames() {
        let (mut manifest, files) = fixture(|v| {
            for anchor in v["anchors"].as_array_mut().unwrap() {
                let sample = anchor["sample"].as_u64().unwrap();
                anchor["sample"] = json!(sample + 48_000);
            }
        });
        manifest.audio_window = Some(super::super::PcmWindow {
            source_samples: 4_320_000,
            start_sample: 48_000,
            end_sample: 720_000,
            fade_in_samples: 2400,
            fade_out_samples: 24_000,
        });
        assert_eq!(
            compile(&manifest, &files).unwrap().unwrap()["anchorFrames"],
            json!([0, 1, 116, 237, 336])
        );
        manifest.audio_window.as_mut().unwrap().source_samples += 1;
        assert!(compile(&manifest, &files).is_err());
        manifest.audio_window.as_mut().unwrap().source_samples -= 1;
        manifest.audio_window.as_mut().unwrap().start_sample += 1;
        manifest.audio_window.as_mut().unwrap().end_sample += 1;
        assert!(compile(&manifest, &files).is_err());
    }

    #[test]
    fn source_clock_refuses_wrong_source_bad_anchors_and_evidence_tampering() {
        for change in [
            (|v: &mut Value| v["source_sha256"] = json!("c".repeat(64))) as fn(&mut Value),
            |v| v["anchors"][1]["id"] = json!("pickup"),
            |v| v["anchors"][2]["sample"] = json!(1),
            |v| v["anchors"][4]["sample"] = json!(4320001),
            |v| v["total_samples"] = json!(1),
        ] {
            let (manifest, files) = fixture(change);
            assert!(compile(&manifest, &files).is_err());
        }
        let (manifest, mut files) = fixture(|_| {});
        files.get_mut("evidence.json").unwrap()[0] ^= 1;
        assert!(compile(&manifest, &files).is_err());
        let (manifest, mut files) = fixture(|_| {});
        files.get_mut("clock.json").unwrap().push(b' ');
        assert!(compile(&manifest, &files).is_err());
        let (mut manifest, files) = fixture(|_| {});
        manifest.assets.retain(|a| a.path != "evidence.json");
        assert!(compile(&manifest, &files).is_err());
    }
}
