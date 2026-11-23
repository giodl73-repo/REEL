//! Complete, nonrecursive source sets. Original proofs are never rewritten.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
pub const COMPOSED_RECEIPT: &str = "reel.episode-conform-receipt.v3";
pub const COMPOSED_FINALIZATION: &str = "reel.programme-composition-finalization.v1";
const HISTORICAL_PRODUCERS: &[&str] =
    &["11da1731ddff8ab6df0cf93ad04596a950204b1dcd41699ff11e5b5b43e3b227"];
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Composition {
    pub schema: String,
    pub manifest: scene_delivery::FileRef,
    pub sources: Vec<Source>,
    pub order: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub id: String,
    pub asset_root: PathBuf,
    pub set: scene_delivery::FileRef,
    pub producer: scene_delivery::FileRef,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceIdentity {
    pub source: Source,
    pub manifest: scene_delivery::FileRef,
    pub proofs: Vec<scene_delivery::FileRef>,
    pub episode_id: String,
    pub language: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    pub source_id: String,
    pub source_index: usize,
    pub source_start_frame: u64,
    pub source_start_sample: u64,
    pub target_index: usize,
    pub start_frame: u64,
    pub start_sample: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionProvenance {
    pub composition: scene_delivery::FileRef,
    pub sources: Vec<SourceIdentity>,
    pub placements: Vec<Placement>,
    pub finalizer_sha256: String,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComposedReceipt {
    pub schema: String,
    pub finalization: Receipt,
    pub provenance: CompositionProvenance,
    pub coded_video_packets_verified: bool,
}
fn read(root: &Path, f: &scene_delivery::FileRef) -> Result<Vec<u8>> {
    Ok(fs::read(scene_delivery::checked_file(root, f)?)?)
}
fn verify_controls(root: &Path, m: &Manifest) -> Result<()> {
    if m.schema != "reel.episode-conform.v1" || m.preassembled_master.is_some() {
        bail!("unsupported composition target");
    }
    for f in [
        &m.catalog,
        &m.master_template_definition,
        &m.episode,
        &m.season_bindings,
        &m.episode_bindings,
    ]
    .into_iter()
    .chain(m.source_master_template.iter())
    {
        scene_delivery::checked_file(root, f)?;
    }
    let template_bytes = read(root, &m.master_template_definition)?;
    let template: MasterTemplate = serde_json::from_slice(&template_bytes)?;
    let catalog: TemplateCatalog = serde_json::from_slice(&read(root, &m.catalog)?)?;
    let episode: Episode = serde_json::from_slice(&read(root, &m.episode)?)?;
    let season: ScopedBindings = serde_json::from_slice(&read(root, &m.season_bindings)?)?;
    let bindings: ScopedBindings = serde_json::from_slice(&read(root, &m.episode_bindings)?)?;
    let presentation = resolve_episode_presentation(&catalog, &episode, &season, &bindings)?;
    let selected = catalog
        .templates
        .iter()
        .find(|t| t.template_id == episode.master_template_id && t.kind == "episode-master")
        .context("target master not selected")?;
    if episode.episode_id != m.episode_id
        || template.template_id != episode.master_template_id
        || selected.definition_sha256 != sha(&template_bytes)
        || template.schema != "reel.episode-master-template.v2"
        || m.audio_frame_conform.as_deref() != Some("pad-silence-to-picture-boundaries")
    {
        bail!("invalid composition authoring controls");
    }
    match (&template.source_template_sha256, &m.source_master_template) {
        (None, None) => {}
        (Some(expected), Some(f)) => {
            let bytes = read(root, f)?;
            let v: serde_json::Value = serde_json::from_slice(&bytes)?;
            let units: Vec<MasterUnit> = serde_json::from_value(v["ordered_units"].clone())?;
            if sha(&bytes) != *expected || units != template.ordered_units {
                bail!("composition source template mismatch");
            }
        }
        _ => bail!("incomplete composition source template"),
    }
    let scenes: Vec<_> = m
        .segments
        .iter()
        .filter(|s| s.kind == SegmentKind::Scene)
        .map(|s| s.id.clone())
        .collect();
    if scenes != episode.scene_ids {
        bail!("composition episode scene order mismatch");
    }
    for segment in m
        .segments
        .iter()
        .filter(|s| s.kind == SegmentKind::EpisodePresentation)
    {
        let invocation = episode
            .presentation
            .iter()
            .find(|p| p.role == segment.id)
            .context("unknown composition presentation role")?;
        let binding = invocation
            .asset_binding
            .as_ref()
            .context("composition presentation binding missing")?;
        let asset = presentation
            .selected_inputs
            .get(binding)
            .context("composition selected presentation missing")?;
        if asset.sha256 != segment.master.sha256 || asset.bytes != segment.master.bytes {
            bail!("composition presentation binding differs from selected master");
        }
    }
    validate_v2_order(&template, m)?;
    Ok(())
}
pub fn compose(path: &Path, root: &Path, output: &Path) -> Result<Receipt> {
    if output.exists() {
        bail!("programme output must be a new directory");
    }
    let scope = checkpoint::snapshot_job_scope(root)?;
    declared_control(path, root, &scope)?;
    if fs::metadata(path)?.len() > 4 * 1024 * 1024 {
        bail!("composition control too large");
    }
    let c: Composition = serde_json::from_slice(&fs::read(path)?)?;
    if c.schema != "reel.programme-part-composition.v1"
        || c.sources.len() != 2
        || c.order.len() != 2
    {
        bail!("composition requires a complete single-prefix set followed by a complete body set");
    }
    if c.manifest.bytes > 4 * 1024 * 1024 {
        bail!("target manifest too large");
    }
    let bytes = read(root, &c.manifest)?;
    let target: Manifest = serde_json::from_slice(&bytes)?;
    let target_value: serde_json::Value = serde_json::from_slice(&bytes)?;
    verify_controls(root, &target)?;
    let clock = target
        .output_frame_rate
        .as_ref()
        .context("composition clock missing")?
        .clock()?;
    let profile = compact_profile(&target)?;
    if !profile.retain_lossless_master {
        bail!("composition must retain immutable lossless sources");
    }
    let current = file_sha(&std::env::current_exe()?)?;
    let mut sources = BTreeMap::new();
    for source in c.sources {
        if source.id.is_empty() || sources.insert(source.id.clone(), source).is_some() {
            bail!("duplicate source identity");
        }
    }
    let mut used = BTreeSet::new();
    let mut prepared = Vec::new();
    let mut proof_refs = Vec::new();
    let mut provenance = CompositionProvenance {
        composition: scene_delivery::FileRef {
            path: path
                .canonicalize()?
                .strip_prefix(root.canonicalize()?)?
                .into(),
            sha256: file_sha(path)?,
            bytes: fs::metadata(path)?.len(),
        },
        sources: vec![],
        placements: vec![],
        finalizer_sha256: current.clone(),
    };
    let (mut frames, mut samples) = (0u64, 0u64);
    let mut signature = None;
    for (block, id) in c.order.iter().enumerate() {
        if !used.insert(id) {
            bail!("repeated source block");
        }
        let source = sources.get(id).context("unknown source block")?;
        scene_delivery::checked_file(root, &source.producer)?;
        if source.producer.sha256 != current
            && !HISTORICAL_PRODUCERS.contains(&source.producer.sha256.as_str())
        {
            bail!("unqualified original producer");
        }
        if source.set.bytes > 4 * 1024 * 1024 {
            bail!("source set too large");
        }
        let set: PartSet = serde_json::from_slice(&read(root, &source.set)?)?;
        if set.manifest.bytes > 4 * 1024 * 1024 {
            bail!("source manifest too large");
        }
        let source_root = scoped_import_root(root, Some(&source.asset_root))?;
        let original_bytes = read(&source_root, &set.manifest)?;
        let original: Manifest = serde_json::from_slice(&original_bytes)?;
        verify_controls(&source_root, &original)?;
        let original_value: serde_json::Value = serde_json::from_slice(&original_bytes)?;
        let p = compact_profile(&original)?;
        if original.language != target.language
            || original.output_sample_rate != target.output_sample_rate
            || original
                .output_frame_rate
                .as_ref()
                .context("source clock missing")?
                .clock()?
                != clock
            || p.crf != profile.crf
            || p.audio_bitrate_kbps != profile.audio_bitrate_kbps
            || (block == 0
                && (original.segments.len() != 1
                    || original.segments[0].kind != SegmentKind::EpisodePresentation))
        {
            bail!("incompatible complete source set");
        }
        let proofs =
            checked_source_proofs(&set, &source_root, &original_bytes, &source.producer.sha256)?;
        let originals = prepare_originals(&proofs)?;
        // Offsets must be integral, so original rounded local boundaries remain valid.
        if u128::from(frames) * u128::from(target.output_sample_rate) * u128::from(clock.1)
            % u128::from(clock.0)
            != 0
        {
            bail!("nonintegral source block offset");
        }
        for (source_index, mut part) in originals.into_iter().enumerate() {
            let index = prepared.len();
            if target.output_geometry.as_ref().is_some_and(|g| {
                u64::from(g.width) != part.facts.width || u64::from(g.height) != part.facts.height
            }) {
                bail!("target geometry differs from source parts");
            }
            if target_value["segments"].get(index) != original_value["segments"].get(source_index) {
                bail!("target source selection changed");
            }
            if signature
                .as_ref()
                .is_some_and(|s| *s != part.video_signature)
            {
                bail!("incompatible source video signatures");
            }
            signature = Some(part.video_signature.clone());
            provenance.placements.push(Placement {
                source_id: id.clone(),
                source_index,
                source_start_frame: part.native.start_frame,
                source_start_sample: part.native.start_sample,
                target_index: index,
                start_frame: frames,
                start_sample: samples,
            });
            // Finalizer-only references are scoped; immutable producer proofs stay byte exact.
            part.video.path = source.asset_root.join(&part.video.path);
            part.pcm.path = source.asset_root.join(&part.pcm.path);
            part.native.start_frame = frames;
            part.native.start_sample = samples;
            frames = frames
                .checked_add(part.native.frames)
                .context("composition frame overflow")?;
            samples = samples
                .checked_add(part.native.samples)
                .context("composition sample overflow")?;
            let boundary =
                (u128::from(frames) * u128::from(target.output_sample_rate) * u128::from(clock.1)
                    + u128::from(clock.0) / 2)
                    / u128::from(clock.0);
            if u128::from(samples) != boundary {
                bail!("composition sample/frame mismatch");
            }
            prepared.push(part);
        }
        proof_refs.extend(set.proofs.iter().map(|f| scene_delivery::FileRef {
            path: source.asset_root.join(&f.path),
            sha256: f.sha256.clone(),
            bytes: f.bytes,
        }));
        provenance.sources.push(SourceIdentity {
            source: source.clone(),
            manifest: set.manifest,
            proofs: set.proofs,
            episode_id: original.episode_id,
            language: original.language,
        });
    }
    if prepared.len() != target.segments.len() {
        bail!("incomplete target coverage");
    }
    finalize_checked(
        root,
        output,
        &bytes,
        &prepared,
        &proof_refs,
        Some(&provenance),
    )
}
fn packet_hashes(path: &Path) -> Result<Vec<String>> {
    let out = command("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_packets",
            "-show_data_hash",
            "sha256",
            "-show_entries",
            "packet=data_hash",
            "-of",
            "json",
        ])
        .arg(path)
        .output()?;
    if !out.status.success() {
        bail!("coded packet probe failed");
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    v["packets"]
        .as_array()
        .context("missing coded packets")?
        .iter()
        .map(|p| {
            p["data_hash"]
                .as_str()
                .map(str::to_owned)
                .context("packet hash missing")
        })
        .collect()
}
pub(super) fn verify_packet_copy(root: &Path, parts: &[PreparedPart], target: &Path) -> Result<()> {
    let mut expected = Vec::new();
    for p in parts {
        expected.extend(packet_hashes(&scene_delivery::checked_file(
            root, &p.video,
        )?)?);
    }
    if packet_hashes(target)? != expected {
        bail!("coded H264 packet payloads changed");
    }
    Ok(())
}
