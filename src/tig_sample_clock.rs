//! Source-bound sample-clock drafts over existing protected Tig readers.
//! This supplies input correspondence, not a production claim or job admission.
use crate::{
    music_sample_clock,
    scene_delivery::InputFiles,
    tig_inputs::{Profile, Source},
};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path},
};
use tig_core::Object;

fn write_new(path: &Path, raw: &[u8]) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(raw)?;
    file.sync_all()?;
    Ok(())
}

fn destination(output: &Path, profile: &Profile) -> Result<()> {
    if !output.is_absolute()
        || fs::symlink_metadata(output).is_ok()
        || output.components().any(|c| {
            matches!(c, Component::ParentDir) || c.as_os_str().eq_ignore_ascii_case(".git")
        })
    {
        bail!("new absolute ordinary output directory required");
    }
    let parent = output
        .parent()
        .ok_or_else(|| anyhow::anyhow!("missing output parent"))?;
    for ancestor in parent.ancestors() {
        let metadata = fs::symlink_metadata(ancestor)?;
        #[cfg(windows)]
        let linked = {
            use std::os::windows::fs::MetadataExt;
            metadata.file_attributes() & 0x400 != 0
        };
        #[cfg(not(windows))]
        let linked = metadata.file_type().is_symlink();
        if linked || !metadata.is_dir() {
            bail!("output ancestor is not an ordinary directory");
        }
    }
    let parent = parent.canonicalize()?;
    for place in &profile.byte_source.places {
        if parent.starts_with(
            place
                .root
                .canonicalize()
                .unwrap_or_else(|_| place.root.clone()),
        ) {
            bail!("output must be outside configured byte-provider roots");
        }
    }
    Ok(())
}

/// Resolve exact logical FileRefs, hold their guards through rendering/checking,
/// then write a final private source correspondence packet. No media copies.
pub fn render(
    profile: Profile,
    request: &Path,
    output: &Path,
    max_output_bytes: u64,
) -> Result<Value> {
    destination(output, &profile)?;
    let mut raw = Vec::new();
    fs::File::open(request)?
        .take(65_537)
        .read_to_end(&mut raw)?;
    let estimate = music_sample_clock::output_byte_bound(&raw)?;
    if estimate > max_output_bytes || max_output_bytes > 512 * 1024 * 1024 {
        bail!("declared sample output budget insufficient or unbounded");
    }
    let mut mapped: Value = serde_json::from_slice(&raw)?;
    let mut pointers = vec!["/bank_audio", "/calibration", "/native_score_midi"];
    if !mapped["vocal"].is_null() {
        pointers.push("/vocal/source");
    }
    let mut source = Source::open(profile).context("capture admitted Tig source")?;
    let mut files = Vec::new();
    let mut correspondence = Vec::new();
    for pointer in &pointers {
        let file: crate::scene_delivery::FileRef = serde_json::from_value(
            mapped
                .pointer(pointer)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("missing input"))?,
        )?;
        let id = file
            .path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("invalid input ID"))?;
        let selected = source
            .authoring_file(id)
            .context("resolve selected Tig input")?;
        if selected.sha256 != file.sha256 || selected.bytes != file.bytes {
            bail!("request differs from selected source input");
        }
        let record = source.record(id)?;
        correspondence.push(
            json!({"pointer":pointer,"id":id,"record":record.reference(),
            "definition":record.definition(),"subject":{"sha256":file.sha256,"bytes":file.bytes}}),
        );
        files.push(file);
    }
    // Existing API refuses where effective mandatory path protection is absent.
    let held = source
        .prepare_files(&files)
        .context("protect exact Tig inputs")?;
    for (pointer, file) in pointers.iter().zip(&files) {
        mapped.pointer_mut(pointer).unwrap()["path"] = json!(held.verified_path(file)?);
    }
    let physical = serde_json::to_vec(&mapped)?;
    let summary = music_sample_clock::render_bytes(&physical, output)?;
    write_new(&output.join("render-request.json"), &physical)?;
    let checked = music_sample_clock::check(&output.join("render-request.json"), output)?;
    write_new(&output.join("tig-request.json"), &raw)?;
    let receipt = json!({"schema":"reel.tig-sample-clock-source.v1",
        "checkpoint":held.checkpoint(),"logical_request":Object::from_bytes(&raw),
        "render_request":Object::from_bytes(&physical),"inputs":correspondence,
        "render_receipt":Object::from_bytes(&fs::read(output.join("receipt.json"))?),
        "check":checked,"protected_inputs":held.objects(),"complete_semantic_coverage":false,
        "production_claim":null,"human_approval":false,"published":false,"authorizes_eviction":false});
    write_new(
        &output.join("tig-source.json"),
        &serde_json::to_vec(&receipt)?,
    )?;
    drop(held);
    Ok(
        json!({"render":summary,"source_inputs":files.len(),"checked":true,
        "production_claim":null,"human_approval":false,"published":false,"git_changed":false}),
    )
}
