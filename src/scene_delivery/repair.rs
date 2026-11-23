//! Deterministic candidate controls for an exact still replacement. No asset
//! selection, approval, source edits, rendering or native custody is implied.
use super::{FileRef, Job, PictureKind};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::OpenOptions,
    io::{Read, Write},
    path::Path,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StillReplacement {
    pub schema: String,
    /// Explicit attachments, independent of drawing filename/version.
    pub attachments: Vec<String>,
    pub expected: FileRef,
    pub replacement: FileRef,
}
#[derive(Serialize)]
pub struct Derived {
    pub schema: &'static str,
    pub job: Job,
    pub attachments: Vec<String>,
    /// Preserved camera/crop coordinates may need new-image review. This is
    /// a candidate control, not a claim those coordinates fit the new drawing.
    pub geometry_review_required: bool,
    pub creative_approval: bool,
}
fn reference(value: &FileRef) -> bool {
    let path = value.path.to_str().unwrap_or("");
    !path.is_empty()
        && path.len() <= 512
        && !path.chars().any(char::is_control)
        && value.bytes > 0
        && value.sha256.len() == 64
        && value
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub fn derive(job: &Job, spec: &StillReplacement) -> Result<Derived> {
    ensure!(
        job.schema == "reel.scene-delivery.v0.1"
            && spec.schema == "reel.scene-still-replacement.v1",
        "Unsupported still replacement schema"
    );
    ensure!(
        reference(&spec.expected) && reference(&spec.replacement),
        "Exact named still references required"
    );
    ensure!(
        (spec.expected.sha256.as_str(), spec.expected.bytes)
            != (spec.replacement.sha256.as_str(), spec.replacement.bytes),
        "Replacement must change the selected picture bytes"
    );
    ensure!(
        !spec.attachments.is_empty() && spec.attachments.len() <= 128,
        "Bounded attachment set required"
    );
    let requested: BTreeSet<_> = spec.attachments.iter().collect();
    ensure!(
        requested.len() == spec.attachments.len()
            && requested
                .iter()
                .all(|id| !id.is_empty() && id.len() <= 128 && !id.chars().any(char::is_control)),
        "Unique explicit attachments required"
    );
    let mut ids = BTreeSet::new();
    ensure!(
        job.pictures.iter().all(|p| ids.insert(&p.attachment_id)),
        "Ambiguous picture attachment IDs"
    );
    let mut derived = job.clone();
    let mut found = BTreeSet::new();
    let mut geometry = job.post_compose_camera.is_some();
    for picture in &mut derived.pictures {
        if !requested.contains(&picture.attachment_id) {
            continue;
        }
        ensure!(
            picture.kind == PictureKind::Still && picture.source == spec.expected,
            "Stale or non-still attachment subject"
        );
        geometry |= picture.crop.is_some() || picture.motion.is_some();
        picture.source = spec.replacement.clone();
        found.insert(picture.attachment_id.clone());
    }
    ensure!(
        found.len() == requested.len(),
        "Requested attachment absent"
    );
    Ok(Derived {
        schema: "reel.scene-still-replacement-result.v1",
        job: derived,
        attachments: found.into_iter().collect(),
        geometry_review_required: geometry,
        creative_approval: false,
    })
}

#[derive(Serialize)]
pub struct Receipt {
    pub schema: &'static str,
    pub original_job_sha256: String,
    pub replacement_spec_sha256: String,
    pub derived_job_sha256: String,
    pub derived_job_bytes: u64,
    pub attachments: Vec<String>,
    pub geometry_review_required: bool,
    pub creative_approval: bool,
}
fn hash(raw: &[u8]) -> String {
    Sha256::digest(raw)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn read(path: &Path, pin: &str) -> Result<Vec<u8>> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    let mut file = options.open(path)?;
    ensure!(
        file.metadata()?.is_file(),
        "Ordinary metadata file required"
    );
    let mut raw = Vec::new();
    Read::by_ref(&mut file)
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut raw)?;
    ensure!(
        raw.len() <= 4 * 1024 * 1024 && hash(&raw) == pin,
        "Metadata bound or exact pin mismatch"
    );
    Ok(raw)
}
/// Explicitly create a NEW candidate job. Existing/partial destinations are
/// preserved. The returned receipt is technical lineage, never selection.
pub fn write_candidate(
    job: &Path,
    job_sha256: &str,
    spec: &Path,
    spec_sha256: &str,
    output: &Path,
) -> Result<Receipt> {
    let raw = read(job, job_sha256)?;
    let spec_raw = read(spec, spec_sha256)?;
    let job: Job = serde_json::from_slice(&raw).context("Invalid delivery job")?;
    let spec: StillReplacement =
        serde_json::from_slice(&spec_raw).context("Invalid still replacement")?;
    let derived = derive(&job, &spec)?;
    let bytes = serde_json::to_vec(&derived.job)?;
    ensure!(
        bytes.len() <= 4 * 1024 * 1024,
        "Derived job metadata exceeds bound"
    );
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(Receipt {
        schema: "reel.scene-still-replacement-receipt.v1",
        original_job_sha256: job_sha256.into(),
        replacement_spec_sha256: spec_sha256.into(),
        derived_job_sha256: hash(&bytes),
        derived_job_bytes: bytes.len() as u64,
        attachments: derived.attachments,
        geometry_review_required: derived.geometry_review_required,
        creative_approval: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture() -> (Job, StillReplacement) {
        let old = FileRef {
            path: "old.png".into(),
            sha256: hash(b"old"),
            bytes: 3,
        };
        let next = FileRef {
            path: "replacement.png".into(),
            sha256: hash(b"new"),
            bytes: 3,
        };
        let job:Job = serde_json::from_value(json!({"schema":"reel.scene-delivery.v0.1","id":"scene","contract":{"path":"es-contract.json","sha256":hash(b"clock"),"bytes":5},
            "production_manifest_sha256":hash(b"manifest"),"width":320,"height":180,"max_composition_samples":96000,
            "pictures":[{"attachment_id":"C101","source":old,"kind":"still","attention":"Synthetic","delivery_frame_count":24},{"attachment_id":"C102","source":old,"kind":"still","attention":"Synthetic neighbor","delivery_frame_count":24}],
            "audio":[{"attachment_id":"D1","source":{"path":"selected-take.wav","sha256":hash(b"take"),"bytes":4},"bus":"D","cue_id":"poem"}],"buses":{"D":{"state":"present","reason":"Selected recording"}}})).unwrap();
        (
            job,
            StillReplacement {
                schema: "reel.scene-still-replacement.v1".into(),
                attachments: vec!["C101".into()],
                expected: old,
                replacement: next,
            },
        )
    }
    #[test]
    fn only_explicit_picture_source_changes_and_clock_audio_neighbor_remain_exact() {
        let (job, spec) = fixture();
        let before = serde_json::to_value(&job).unwrap();
        let derived = derive(&job, &spec).unwrap();
        let mut after = serde_json::to_value(&derived.job).unwrap();
        assert_eq!(
            after["pictures"][0]["source"],
            serde_json::to_value(&spec.replacement).unwrap()
        );
        after["pictures"][0]["source"] = before["pictures"][0]["source"].clone();
        assert_eq!(before, after);
        assert!(!derived.creative_approval && !derived.geometry_review_required);
    }
    #[test]
    fn language_specific_contracts_and_selected_recordings_are_never_copied_between_jobs() {
        let (es, spec) = fixture();
        let mut en = es.clone();
        en.id = "scene-en".into();
        en.contract.path = "en-contract.json".into();
        en.contract.sha256 = hash(b"different English trigger clock");
        en.audio[0].source.path = "en-selected-take.wav".into();
        en.audio[0].source.sha256 = hash(b"different English recording");
        en.max_composition_samples = 144000;
        en.pictures[0].delivery_frame_count = Some(48);
        for original in [&es, &en] {
            let derived = derive(original, &spec).unwrap();
            let mut expected = original.clone();
            expected.pictures[0].source = spec.replacement.clone();
            assert_eq!(
                serde_json::to_value(&derived.job).unwrap(),
                serde_json::to_value(&expected).unwrap()
            );
        }
    }
    #[test]
    fn changed_subject_missing_duplicate_and_video_targets_refuse() {
        for mutation in ["subject", "missing", "duplicate", "ambiguous", "video"] {
            let (mut job, mut spec) = fixture();
            match mutation {
                "subject" => spec.expected.sha256 = hash(b"another version"),
                "missing" => spec.attachments[0] = "absent".into(),
                "duplicate" => spec.attachments.push("C101".into()),
                "ambiguous" => job.pictures[1].attachment_id = "C101".into(),
                "video" => job.pictures[0].kind = PictureKind::Video,
                _ => unreachable!(),
            }
            assert!(derive(&job, &spec).is_err(), "{mutation}");
        }
    }
    #[test]
    fn retained_geometry_is_flagged_and_candidate_files_never_overwrite_or_accept_bad_pins() {
        let (mut job, spec) = fixture();
        job.pictures[0].crop = Some(super::super::Crop {
            x: 0,
            y: 0,
            width: 16,
            height: 16,
        });
        assert!(derive(&job, &spec).unwrap().geometry_review_required);
        let root = tempfile::tempdir().unwrap();
        let job_raw = serde_json::to_vec(&job).unwrap();
        let spec_raw = serde_json::to_vec(&spec).unwrap();
        std::fs::write(root.path().join("job.json"), &job_raw).unwrap();
        std::fs::write(root.path().join("spec.json"), &spec_raw).unwrap();
        let output = root.path().join("derived.json");
        assert!(
            write_candidate(
                &root.path().join("job.json"),
                &hash(b"wrong"),
                &root.path().join("spec.json"),
                &hash(&spec_raw),
                &output
            )
            .is_err()
        );
        assert!(!output.exists());
        let receipt = write_candidate(
            &root.path().join("job.json"),
            &hash(&job_raw),
            &root.path().join("spec.json"),
            &hash(&spec_raw),
            &output,
        )
        .unwrap();
        assert_eq!(
            receipt.derived_job_sha256,
            hash(&std::fs::read(&output).unwrap())
        );
        assert!(
            write_candidate(
                &root.path().join("job.json"),
                &hash(&job_raw),
                &root.path().join("spec.json"),
                &hash(&spec_raw),
                &output
            )
            .is_err()
        );
        assert_eq!(
            std::fs::read(root.path().join("job.json")).unwrap(),
            job_raw
        );
    }
}
