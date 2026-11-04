//! Fresh browser primary-reference comparison inside caller-admitted execution.
//! This low-level result grants no source ownership, approval or eviction rights.
use crate::{
    media_parity,
    media_runtime::{BoundPrograms, BoundTool},
    scene_delivery::{FileRef, InputFiles},
    tig_browser::{self, Prepared},
};
use anyhow::{Context, Result, bail};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tig_core::Object;

/// Constructed only after rendering current primaries and comparing every native
/// picture/sample/clock against the separately held selected master.
/// The caller must retain source/runtime/output guards and source authority.
pub struct PrimaryComparison {
    manifest: Object,
    master: FileRef,
    parity: media_parity::Report,
}
impl PrimaryComparison {
    pub fn manifest(&self) -> &Object {
        &self.manifest
    }
    pub fn master(&self) -> &FileRef {
        &self.master
    }
    pub fn picture_sha256(&self) -> (&str, &str) {
        (
            &self.parity.candidate.yuv444p_frame_sequence_sha256,
            &self.parity.candidate.rgb24_frame_sequence_sha256,
        )
    }
    pub fn audio_sha256(&self) -> &str {
        &self.parity.candidate.pcm24_sha256
    }
    pub fn frames(&self) -> u64 {
        self.parity.shape.frames
    }
    pub fn samples(&self) -> u64 {
        self.parity.shape.samples
    }
}
struct LocalReference {
    file: FileRef,
    path: PathBuf,
    _held: fs::File,
}
impl InputFiles for LocalReference {
    fn verified_path(&self, file: &FileRef) -> Result<PathBuf> {
        if file != &self.file {
            bail!("fresh browser reference differs from admission");
        }
        Ok(self.path.clone())
    }
}
fn shape(plan: &Prepared) -> Result<media_parity::Shape> {
    let m = &plan.manifest;
    if m.audio.is_none() || m.fps == 0 {
        bail!("fresh browser comparison requires an audio clock");
    }
    let numerator = m
        .frame_count
        .checked_mul(48_000)
        .context("browser sample count overflow")?;
    if numerator % u64::from(m.fps) != 0 {
        bail!("browser duration is not an integral sample count");
    }
    Ok(media_parity::Shape {
        width: m.width,
        height: m.height,
        fps_numerator: u64::from(m.fps),
        fps_denominator: 1,
        frames: m.frame_count,
        sample_rate: 48_000,
        samples: numerator / u64::from(m.fps),
    })
}
/// Explicit CPU/media action, never invoked by planning or ordinary resolution.
/// The scheduler must reserve the declared workspace/output peak first. No CAS
/// initialization, source adoption, retained publication or Git mutation occurs.
#[allow(clippy::too_many_arguments)]
pub fn compare_master_to_primaries(
    plan: &Prepared,
    inputs: &dyn InputFiles,
    programs: &BoundPrograms,
    browser: &BoundTool,
    limits: &tig_browser::render::Limits,
    clean_picture: bool,
    master: &FileRef,
    outputs: &dyn InputFiles,
    stage_parent: &Path,
    authority: impl FnMut() -> Result<()>,
) -> Result<PrimaryComparison> {
    compare_master_to_primaries_with_geometry(
        plan,
        inputs,
        programs,
        browser,
        limits,
        clean_picture,
        master,
        outputs,
        stage_parent,
        media_parity::GeometryAdmission::default(),
        authority,
    )
}
#[allow(clippy::too_many_arguments)]
pub fn compare_master_to_primaries_with_geometry(
    plan: &Prepared,
    inputs: &dyn InputFiles,
    programs: &BoundPrograms,
    browser: &BoundTool,
    limits: &tig_browser::render::Limits,
    clean_picture: bool,
    master: &FileRef,
    outputs: &dyn InputFiles,
    stage_parent: &Path,
    geometry: media_parity::GeometryAdmission,
    mut authority: impl FnMut() -> Result<()>,
) -> Result<PrimaryComparison> {
    authority()?;
    limits.validate()?;
    let shape = shape(plan)?;
    if !stage_parent.is_absolute() || !fs::symlink_metadata(stage_parent)?.is_dir() {
        bail!("fresh browser comparison requires existing absolute admitted scratch");
    }
    // Refuse incompatible historical stream declarations before starting a fresh
    // browser, rather than guessing pixel aspect or reducing audio precision.
    media_parity::admit_with_geometry(master, &programs.with_inputs(outputs), &shape, geometry)?;
    authority()?;
    let stage = tempfile::Builder::new()
        .prefix("browser-primary-reference-")
        .tempdir_in(stage_parent)?;
    let path = stage.path().join("reference.mkv");
    let rendered = tig_browser::render::native(
        plan,
        inputs,
        programs,
        browser,
        stage.path(),
        &path,
        limits,
        clean_picture,
        &mut authority,
    );
    authority()?;
    let rendered = rendered?;
    use std::os::windows::fs::OpenOptionsExt;
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)?;
    if held.metadata()?.len() != rendered.output.bytes
        || crate::sha256_file(&path)? != rendered.output.sha256.as_str()
        || held.metadata()?.len() != rendered.output.bytes
    {
        bail!("fresh browser reference changed before comparison");
    }
    let reference = LocalReference {
        file: FileRef {
            path: "fresh-reference.mkv".into(),
            sha256: rendered.output.sha256.as_str().into(),
            bytes: rendered.output.bytes,
        },
        path: path.clone(),
        _held: held,
    };
    let compared = media_parity::compare_with_geometry(
        &reference.file,
        &reference,
        master,
        &programs.with_inputs(outputs),
        &shape,
        geometry,
    );
    authority()?;
    let parity = compared?;
    if !parity.equal {
        bail!(
            "selected browser master differs from fresh primary reference: picture_planes_equal={}, displayed_rgb_equal={}, color_metadata_equal={}, pcm_equal={}, native_clock_checked={}",
            parity.picture_planes_equal,
            parity.displayed_rgb_equal,
            parity.color_metadata_equal,
            parity.pcm_equal,
            parity.native_clock_checked
        );
    }
    drop(reference);
    stage.close()?;
    Ok(PrimaryComparison {
        manifest: Object::from_bytes(&plan.manifest_bytes),
        master: master.clone(),
        parity,
    })
}
