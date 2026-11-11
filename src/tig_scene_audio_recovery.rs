//! Exact recovery of a retained native stem through the original shared mixer.
//! Frozen receipts are recovery evidence, never replacement authoring or approval.
use crate::{
    media_runtime::{Bindings, BoundPrograms},
    scene_delivery::{self, FileRef, InputFiles, Job, Receipt},
};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use tig_core::{Object, identity};
use tig_store::existing::{
    ByteSource, Profile as ByteProfile, ReadCache, RequiredProtection, VerifiedRead,
};

const SCHEMA: &str = "reel.tig-scene-audio-recovery.experimental-1";
const CONTROL_MAX: u64 = 4 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    byte_source: ByteProfile,
    job: Object,
    receipt: Object,
    /// Independent expected original bytes, also required on the frozen receipt.
    target: Object,
    output_name: String,
    /// Conservative bound for all native intermediate buses plus the saved stem.
    max_workspace_bytes: u64,
    runtime: Bindings,
}

fn metadata(cache: &mut ReadCache, object: &Object) -> Result<Vec<u8>> {
    if object.bytes > CONTROL_MAX {
        bail!("recovery control exceeds metadata budget");
    }
    let mut held = cache.open_verified(object)?;
    let mut raw = Vec::new();
    held.read_to_end(&mut raw)?;
    Ok(raw)
}

fn admission(profile: &Profile, job: &Job, receipt: &Receipt) -> Result<()> {
    let plan = &receipt.plan;
    let target = receipt.outputs.get(&profile.output_name);
    if profile.schema != SCHEMA
        || profile.byte_source.required != RequiredProtection::PreventMutation
        || !["D.wav", "M.wav", "E.wav", "mix.wav"].contains(&profile.output_name.as_str())
        || receipt.schema != "reel.scene-delivery-receipt.v0.1"
        || job.schema != "reel.scene-delivery.v0.1"
        || plan.schema != "reel.scene-delivery-plan.v0.1"
        || plan.id != job.id
        || plan.job_sha256 != profile.job.sha256.as_str()
        || plan.contract_sha256 != job.contract.sha256
        || plan.production_sha256 != job.production_manifest_sha256
        || receipt.content_samples != plan.duration_samples
        || receipt.delivery_frames != plan.frame_count
        || !(8000..=192000).contains(&plan.sample_rate)
        || plan.duration_samples == 0
        || plan.duration_samples > job.max_composition_samples
        || job.audio.is_empty()
        || job.audio.len() > 256
        || job.audio.len() != plan.audio.len()
        || target.is_none_or(|r| {
            r.path != Path::new(&profile.output_name)
                || r.sha256 != profile.target.sha256.as_str()
                || r.bytes != profile.target.bytes
        })
    {
        bail!("recovery requires matching original job, plan and stem pins");
    }
    // 56 bytes/sample covers four stereo float and four PCM24 buses; include
    // the final retained copy and header headroom. The mixer retires floats,
    // but the deliberately conservative admission does not rely on that detail.
    let required = plan
        .duration_samples
        .checked_mul(56)
        .and_then(|n| n.checked_add(profile.target.bytes))
        .and_then(|n| n.checked_add(1024 * 1024))
        .context("recovery workspace size overflow")?;
    if profile.max_workspace_bytes < required
        || profile.max_workspace_bytes > 4 * 1024 * 1024 * 1024
    {
        bail!("native audio recovery exceeds explicit workspace budget");
    }
    let mut ids = BTreeSet::new();
    for (audio, span) in job.audio.iter().zip(&plan.audio) {
        let duration = span.end_sample.checked_sub(span.start_sample);
        if span.attachment_id != audio.attachment_id
            || !ids.insert(&audio.attachment_id)
            || !["D", "M", "E"].contains(&audio.bus.as_str())
            || !job.buses.contains_key(&audio.bus)
            || !audio.gain_db.is_finite()
            || audio.gain_db.abs() > 60.0
            || span.end_sample > plan.duration_samples
            || duration.is_none_or(|n| {
                n == 0
                    || audio.fade_in_samples.saturating_add(audio.fade_out_samples) > n
                    || audio.source_start_sample.checked_add(n).is_none()
            })
        {
            bail!("invalid original audio placement or gain");
        }
    }
    Ok(())
}

struct AudioInputs {
    // Guards remain live across every decoder/mixer read; locations never come
    // from the job's virtual paths. Only declared audio pins are enrolled.
    held: BTreeMap<String, (u64, VerifiedRead)>,
}
impl InputFiles for AudioInputs {
    fn verified_path(&self, item: &FileRef) -> Result<PathBuf> {
        let (bytes, held) = self
            .held
            .get(&item.sha256)
            .context("audio input not admitted")?;
        if *bytes != item.bytes {
            bail!("audio input length differs from admission");
        }
        Ok(held.with_path(Path::to_path_buf)?)
    }
}

fn save_exact(source: &Path, output: &Path, expected: &Object) -> Result<()> {
    if fs::metadata(source)?.len() != expected.bytes
        || crate::sha256_file(source)? != expected.sha256.as_str()
    {
        bail!("native reconstructed stem differs from original target");
    }
    let parent = output.parent().context("output parent required")?;
    let mut retained = tempfile::NamedTempFile::new_in(parent)?;
    std::io::copy(&mut fs::File::open(source)?, &mut retained)?;
    retained.as_file().sync_all()?;
    if fs::metadata(retained.path())?.len() != expected.bytes
        || crate::sha256_file(retained.path())? != expected.sha256.as_str()
    {
        bail!("saved recovery differs from original target");
    }
    retained.persist_noclobber(output)?;
    Ok(())
}

/// Explicit local reconstruction only. Existing publication remains a separate
/// operation; this command neither initializes stores nor changes selections.
pub fn run(profile_path: &Path, output: &Path) -> Result<Value> {
    if !output.is_absolute() || output.file_name().is_none() || output.symlink_metadata().is_ok() {
        bail!("recovery output must be a new absolute file");
    }
    let parent = output.parent().context("output parent required")?;
    if !parent.is_dir() {
        bail!("existing output parent required");
    }
    let mut raw = Vec::new();
    fs::File::open(profile_path)?
        .take(65537)
        .read_to_end(&mut raw)?;
    if raw.len() > 65536 {
        bail!("recovery profile exceeds budget");
    }
    let profile: Profile = serde_json::from_value(identity::parse_json(&raw)?)?;
    if profile.byte_source.required != RequiredProtection::PreventMutation {
        bail!("native recovery requires protected input reads");
    }
    let mut cache = ReadCache::new(profile.byte_source.clone())?;
    let job: Job = serde_yaml::from_slice(&metadata(&mut cache, &profile.job)?)?;
    let receipt: Receipt = serde_json::from_slice(&metadata(&mut cache, &profile.receipt)?)?;
    admission(&profile, &job, &receipt)?;
    let programs = BoundPrograms::bind(&profile.runtime)?;
    let mut inputs = AudioInputs {
        held: BTreeMap::new(),
    };
    for event in &job.audio {
        if let Some((bytes, _)) = inputs.held.get(&event.source.sha256) {
            if *bytes != event.source.bytes {
                bail!("conflicting audio source pins");
            }
        } else {
            let object = Object {
                sha256: tig_core::Digest::parse(&event.source.sha256)?,
                bytes: event.source.bytes,
            };
            inputs.held.insert(
                event.source.sha256.clone(),
                (object.bytes, cache.open_verified(&object)?),
            );
        }
    }
    let stage = tempfile::Builder::new()
        .prefix(".native-audio-recovery-")
        .tempdir_in(parent)?;
    scene_delivery::render_audio_stems(
        &job,
        &receipt.plan,
        &programs.with_inputs(&inputs),
        stage.path(),
    )?;
    save_exact(
        &stage.path().join(&profile.output_name),
        output,
        &profile.target,
    )?;
    Ok(json!({
        "schema":"reel.tig-scene-audio-recovery-result.experimental-1",
        "job":profile.job,"receipt":profile.receipt,"target":profile.target,
        "verified_original_target":true,"audio_sources":inputs.held.len(),
        "picture_inputs_opened":0,"published":false,"selection_changed":false,
        "original_producer_assertion":false,"semantic_build_validated":false,
        "human_approval":false,"authorizes_eviction":false
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Profile, Job, Receipt) {
        let pin = Object::from_bytes(b"fixture");
        let reference = json!({"path":"source.wav","sha256":pin.sha256,"bytes":pin.bytes});
        let job: Job = serde_json::from_value(json!({
            "schema":"reel.scene-delivery.v0.1","id":"fixture",
            "contract":reference,"production_manifest_sha256":"1".repeat(64),
            "width":16,"height":16,"max_composition_samples":4800,"pictures":[],
            "audio":[{"attachment_id":"dialogue","bus":"D","source":reference}],
            "buses":{"D":{"state":"present","reason":"fixture"}}
        }))
        .unwrap();
        let job_pin = Object::from_bytes(&serde_json::to_vec(&job).unwrap());
        let receipt: Receipt = serde_json::from_value(json!({
            "schema":"reel.scene-delivery-receipt.v0.1","tool_version":"fixture",
            "ffmpeg_version":"fixture","publication":"not-authorized-by-tool",
            "content_samples":4800,"delivery_frames":3,
            "outputs":{"D.wav":{"path":"D.wav","sha256":pin.sha256,"bytes":pin.bytes}},
            "plan":{"schema":"reel.scene-delivery-plan.v0.1","id":"fixture",
                "job_sha256":job_pin.sha256,"contract_sha256":pin.sha256,
                "production_sha256":"1".repeat(64),"compiled_sha256":"2".repeat(64),
                "sample_rate":48000,"fps_numerator":24,"fps_denominator":1,
                "duration_samples":4800,"frame_count":3,"pictures":[],
                "audio":[{"attachment_id":"dialogue","start_sample":0,"end_sample":4800,"start_frame":0,"end_frame":3}],
                "external_layers":[],"buses":{},"creative_authority":"not-granted"}
        })).unwrap();
        let profile = serde_json::from_value(json!({
            "schema":SCHEMA,"job":job_pin,
            "receipt":Object::from_bytes(&serde_json::to_vec(&receipt).unwrap()),
            "target":pin,"output_name":"D.wav","max_workspace_bytes":2*1024*1024,
            "byte_source":{"places":[],"required":"prevent-mutation","limits":{}},
            "runtime":{"host":pin,"ffmpeg":reference,"ffprobe":reference,"dependencies":{}}
        }))
        .unwrap();
        (profile, job, receipt)
    }

    #[test]
    fn stale_lineage_wrong_bus_and_invalid_spans_refuse_before_media_reads() {
        let (mut profile, mut job, mut receipt) = fixture();
        admission(&profile, &job, &receipt).unwrap();
        receipt.plan.job_sha256 = "f".repeat(64);
        assert!(admission(&profile, &job, &receipt).is_err());
        receipt.plan.job_sha256 = profile.job.sha256.as_str().into();
        profile.output_name = "mix.wav".into();
        assert!(admission(&profile, &job, &receipt).is_err());
        profile.output_name = "D.wav".into();
        receipt.plan.audio[0].attachment_id = "other".into();
        assert!(admission(&profile, &job, &receipt).is_err());
        receipt.plan.audio[0].attachment_id = "dialogue".into();
        job.audio[0].fade_out_samples = 4801;
        assert!(admission(&profile, &job, &receipt).is_err());
        job.audio[0].fade_out_samples = 0;
        job.audio[0].gain_db = f64::NAN;
        assert!(admission(&profile, &job, &receipt).is_err());
    }

    #[test]
    fn unsafe_read_protection_and_small_workspace_cannot_start_reconstruction() {
        let (mut profile, job, receipt) = fixture();
        profile.byte_source.required = RequiredProtection::CooperativeImmutable;
        assert!(admission(&profile, &job, &receipt).is_err());
        profile.byte_source.required = RequiredProtection::PreventMutation;
        profile.max_workspace_bytes = 1;
        assert!(admission(&profile, &job, &receipt).is_err());
        profile.max_workspace_bytes = 2 * 1024 * 1024;
        profile.target = Object::from_bytes(b"wrong independently declared target");
        assert!(admission(&profile, &job, &receipt).is_err());
    }
    #[test]
    fn wrong_reconstruction_and_existing_destination_preserve_original_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("candidate.wav");
        let output = tmp.path().join("recovered.wav");
        fs::write(&source, b"original").unwrap();
        let expected = Object::from_bytes(b"original");
        assert!(save_exact(&source, &output, &Object::from_bytes(b"different")).is_err());
        assert!(!output.exists());
        save_exact(&source, &output, &expected).unwrap();
        fs::write(&source, b"changed!").unwrap();
        assert!(save_exact(&source, &output, &Object::from_bytes(b"changed!")).is_err());
        assert_eq!(fs::read(output).unwrap(), b"original");
    }
    #[test]
    fn image_and_undeclared_audio_pins_cannot_reach_the_filesystem() {
        let inputs = AudioInputs {
            held: BTreeMap::new(),
        };
        assert!(
            inputs
                .verified_path(&FileRef {
                    path: "../../picture.png".into(),
                    sha256: "0".repeat(64),
                    bytes: 1
                })
                .is_err()
        );
    }

    #[cfg(windows)]
    #[test]
    fn native_recovery_keeps_dialogue_distinct_from_mix_and_refuses_stale_sources() {
        let (Some(ffmpeg), Some(ffprobe)) = (
            std::env::var_os("REEL_TEST_FFMPEG"),
            std::env::var_os("REEL_TEST_FFPROBE"),
        ) else {
            return;
        };
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source.wav");
        assert!(
            std::process::Command::new(&ffmpeg)
                .args([
                    "-v",
                    "error",
                    "-nostdin",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:sample_rate=48000:duration=0.1",
                    "-c:a",
                    "pcm_s16le"
                ])
                .arg(&source)
                .status()
                .unwrap()
                .success()
        );
        let object = |p: &Path| Object::from_bytes(&fs::read(p).unwrap());
        let (mut profile, mut job, mut receipt) = fixture();
        let source_pin = object(&source);
        job.audio[0].source.sha256 = source_pin.sha256.as_str().into();
        job.audio[0].source.bytes = source_pin.bytes;
        let mut effect = job.audio[0].clone();
        effect.attachment_id = "effect".into();
        effect.bus = "E".into();
        effect.gain_db = -12.0;
        job.audio.push(effect);
        job.buses.insert("E".into(), job.buses["D"].clone());
        let mut effect_span = receipt.plan.audio[0].clone();
        effect_span.attachment_id = "effect".into();
        receipt.plan.audio.push(effect_span);
        // Native audio recovery must work even when a picture is unavailable.
        job.pictures.push(
            serde_json::from_value(json!({
                "attachment_id":"missing-picture", "source":{
                    "path":"missing.png","sha256":"f".repeat(64),"bytes":1
                },"kind":"still","attention":"unavailable synthetic fixture"
            }))
            .unwrap(),
        );
        let baseline = tmp.path().join("baseline");
        fs::create_dir(&baseline).unwrap();
        let reference = |p: &Path| {
            let pin = object(p);
            FileRef {
                path: p.into(),
                sha256: pin.sha256.as_str().into(),
                bytes: pin.bytes,
            }
        };
        profile.runtime.host =
            crate::media_runtime::ByteIdentity::observe_file(&std::env::current_exe().unwrap())
                .unwrap();
        profile.runtime.ffmpeg = reference(Path::new(&ffmpeg));
        profile.runtime.ffprobe = reference(Path::new(&ffprobe));
        let programs = BoundPrograms::bind(&profile.runtime).unwrap();
        scene_delivery::render_audio_stems(
            &job,
            &receipt.plan,
            &programs.with_inputs(tmp.path()),
            &baseline,
        )
        .unwrap();
        profile.target = object(&baseline.join("D.wav"));
        assert_ne!(profile.target, object(&baseline.join("mix.wav")));
        receipt.outputs.get_mut("D.wav").unwrap().sha256 = profile.target.sha256.as_str().into();
        receipt.outputs.get_mut("D.wav").unwrap().bytes = profile.target.bytes;
        let cache_root = tmp.path().join("cache");
        let retain = |raw: &[u8]| {
            let pin = Object::from_bytes(raw);
            let path = tig_store::existing::Layout::ReelCas.object_path(&cache_root, &pin.sha256);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, raw).unwrap();
            pin
        };
        retain(&fs::read(&source).unwrap());
        profile.job = retain(&serde_json::to_vec(&job).unwrap());
        receipt.plan.job_sha256 = profile.job.sha256.as_str().into();
        profile.receipt = retain(&serde_json::to_vec(&receipt).unwrap());
        let profile_json = json!({
            "schema":SCHEMA,"job":profile.job,"receipt":profile.receipt,"target":profile.target,
            "output_name":"D.wav","max_workspace_bytes":profile.max_workspace_bytes,
            "byte_source":{"places":[{"root":cache_root,"layout":"reel-cas"}],"required":"prevent-mutation","limits":{}},
            "runtime":profile.runtime
        });
        let profile_path = tmp.path().join("recovery.json");
        fs::write(&profile_path, serde_json::to_vec(&profile_json).unwrap()).unwrap();
        let output = tmp.path().join("recovered.wav");
        let report = run(&profile_path, &output).unwrap();
        assert_eq!(report["picture_inputs_opened"], 0);
        assert_eq!(object(&output), profile.target);
        assert!(run(&profile_path, &output).is_err());
        let cached_source =
            tig_store::existing::Layout::ReelCas.object_path(&cache_root, &source_pin.sha256);
        fs::write(cached_source, b"stale source").unwrap();
        let failed = tmp.path().join("refused.wav");
        assert!(run(&profile_path, &failed).is_err());
        assert!(!failed.exists());
    }
}
