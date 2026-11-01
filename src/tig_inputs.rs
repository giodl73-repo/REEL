//! Explicit committed Tig sources for native REEL consumers.
//! Byte identity/admission is not full schema, producer or release approval.
use crate::scene_delivery::{FileRef, InputFiles, Job, TimedOverlayDerivation};
use anyhow::{Result, anyhow, bail};
use reel_assembly::scene_authoring::AssetRef;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};
use tig_assets::{graph, resolve::AssetResolver};
use tig_core::{
    Digest, LogicalPath, Object,
    tree::{NodeStore, TreeLimits},
};
use tig_git::source::{
    GitId, Repository, Scope,
    native::{AssetSnapshot, Limits, context::ProductionContext},
};
use tig_store::existing::{
    ByteSource, Profile as ByteProfile, ReadCache, RequiredProtection, VerifiedRead,
};
use tig_store::source_claims::Selection;

pub const PROFILE_SCHEMA: &str = "reel.tig-input-profile.experimental-1";
pub const ARCHIVE_PROFILE_SCHEMA: &str = "reel.tig-archive-input-profile.experimental-1";
const PROFILE_BYTES_MAX: usize = 65_536;
pub const JOB_BYTES_MAX: usize = 4 * 1024 * 1024;
const INPUT_NAMES_MAX: usize = 4096;
const METADATA_TOTAL_MAX: u64 = 16 * 1024 * 1024;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub git_executable: PathBuf,
    pub repository: PathBuf,
    pub commit: String,
    pub records: Vec<String>,
    pub definitions: Vec<String>,
    #[serde(default)]
    pub limits: Limits,
    pub admitted: Vec<Admission>,
    pub byte_source: ByteProfile,
}
/// Caller-admitted source archive plus explicit existing-byte enrollment.
/// Loading does not use Git, install a store or grant job/custody authority.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveProfile {
    pub schema: String,
    pub archive: Object,
    pub checkpoint: Digest,
    #[serde(default)]
    pub limits: Limits,
    pub admitted: Vec<Admission>,
    pub byte_source: ByteProfile,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Admission {
    pub id: String,
    pub object: Object,
    pub payload: Payload,
}
#[derive(Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Payload {
    Selected {},
    Stream { slot: String },
}

fn bounded_file(path: &Path, max: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| anyhow!("cannot open explicit input"))?
        .take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| anyhow!("cannot read explicit input"))?;
    if bytes.len() > max {
        bail!("input exceeds byte budget");
    }
    Ok(bytes)
}
impl Profile {
    pub fn read(path: &Path) -> Result<Self> {
        let value = tig_core::identity::parse_json(&bounded_file(path, PROFILE_BYTES_MAX)?)
            .map_err(|_| anyhow!("invalid Tig profile JSON"))?;
        serde_json::from_value(value).map_err(|_| anyhow!("invalid Tig profile fields"))
    }
}
pub fn read_job(path: &Path) -> Result<Job> {
    serde_yaml::from_slice(&bounded_file(path, JOB_BYTES_MAX)?)
        .map_err(|_| anyhow!("invalid scene job"))
}
/// Explicit exact-pin reference for comparison; never an implicit latest lookup.
pub fn read_reference(path: &Path) -> Result<FileRef> {
    let value = tig_core::identity::parse_json(&bounded_file(path, PROFILE_BYTES_MAX)?)
        .map_err(|_| anyhow!("invalid Tig reference JSON"))?;
    serde_json::from_value(value).map_err(|_| anyhow!("invalid Tig reference fields"))
}
fn scope(paths: Vec<String>) -> Result<Scope> {
    if paths.is_empty() || paths.len() > 64 {
        bail!("invalid namespace scope");
    }
    let paths = paths
        .iter()
        .map(|s| LogicalPath::parse(s))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Scope::new(paths)?)
}
fn logical(path: &Path) -> Result<String> {
    let value = path
        .to_str()
        .ok_or_else(|| anyhow!("non-Unicode asset ID"))?;
    token(value, 256)?;
    Ok(value.to_owned())
}
fn token(value: &str, max: usize) -> Result<()> {
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        bail!("invalid bounded asset token");
    }
    Ok(())
}

/// One bounded immutable metadata view and an explicit existing-byte provider.
/// Opening captures Git metadata only; primary byte IO is explicit below.
pub struct Source {
    snapshot: AssetSnapshot,
    admitted: BTreeMap<String, Admission>,
    cache: ReadCache,
    required: RequiredProtection,
    native_context_descriptor: Option<Object>,
}
impl Source {
    pub fn open(profile: Profile) -> Result<Self> {
        if profile.schema != PROFILE_SCHEMA {
            bail!("unsupported or unbounded Tig profile");
        }
        let admitted = Self::admit(profile.admitted)?;
        let required = profile.byte_source.required;
        let cache = ReadCache::new(profile.byte_source)?;
        let repository = Repository::new(
            profile.git_executable,
            profile.repository,
            Default::default(),
        )?;
        let view = repository.commit(GitId::parse(&profile.commit)?)?;
        let snapshot = repository.native(
            &view,
            scope(profile.records)?,
            scope(profile.definitions)?,
            profile.limits,
        )?;
        Ok(Self {
            snapshot,
            admitted,
            cache,
            required,
            native_context_descriptor: None,
        })
    }
    fn admit(entries: Vec<Admission>) -> Result<BTreeMap<String, Admission>> {
        if entries.is_empty() || entries.len() > 256 {
            bail!("unbounded definition admission");
        }
        let mut admitted = BTreeMap::new();
        for entry in entries {
            token(&entry.id, 256)?;
            if let Payload::Stream { slot } = &entry.payload
                && (slot.is_empty() || slot.len() > 128 || slot.chars().any(char::is_control))
            {
                bail!("invalid stream selector");
            }
            if admitted.insert(entry.id.clone(), entry).is_some() {
                bail!("duplicate definition admission");
            }
        }
        Ok(admitted)
    }
    /// The caller owns provider finish/readback and protection of this root.
    /// Source document archiving does not archive selected payloads or controls.
    pub fn archive_metadata(
        &self,
        store: &mut impl NodeStore,
        limits: TreeLimits,
    ) -> Result<Object> {
        Ok(self.snapshot.archive_metadata(store, limits)?)
    }
    /// Reopen an exact externally admitted checkpoint without a repository path.
    /// This does not grant production eligibility or claim retained payload bytes.
    pub fn from_archive(
        profile: ArchiveProfile,
        store: &mut impl NodeStore,
        limits: TreeLimits,
    ) -> Result<Self> {
        if profile.schema != ARCHIVE_PROFILE_SCHEMA {
            bail!("unsupported archived Tig profile");
        }
        let admitted = Self::admit(profile.admitted)?;
        let required = profile.byte_source.required;
        let cache = ReadCache::new(profile.byte_source)?;
        let snapshot =
            AssetSnapshot::from_metadata_archive(store, &profile.archive, profile.limits, limits)?;
        if snapshot.checkpoint() != &profile.checkpoint {
            bail!("archived source checkpoint differs from admission");
        }
        Ok(Self {
            snapshot,
            admitted,
            cache,
            required,
            native_context_descriptor: None,
        })
    }
    pub(crate) fn admission(&self, id: &str) -> Result<&Admission> {
        self.admitted
            .get(id)
            .ok_or_else(|| anyhow!("definition not admitted"))
    }
    pub(crate) fn bind_native_context(&mut self, descriptor: &[u8]) {
        self.native_context_descriptor = Some(Object::from_bytes(descriptor));
    }
    pub(crate) fn check_native_context(&self, descriptor: &[u8]) -> Result<()> {
        if self
            .native_context_descriptor
            .as_ref()
            .is_some_and(|expected| expected != &Object::from_bytes(descriptor))
        {
            bail!("request differs from frozen native context");
        }
        Ok(())
    }
    pub(crate) fn production_requirements(
        &mut self,
        root: &str,
        profiles: &[graph::Profile],
        limits: graph::Limits,
    ) -> Result<graph::ProductionRequirements> {
        let query = self.production_query(root, profiles, limits)?;
        Ok(graph::resolve(&mut self.snapshot, &query, profiles)?.production_requirements()?)
    }
    /// Explicit native-consumer roots at this exact captured source. No byte IO.
    pub(crate) fn production_requirements_for_ids(
        &mut self,
        roots: &std::collections::BTreeSet<String>,
        profiles: &[graph::Profile],
        limits: graph::Limits,
    ) -> Result<graph::ProductionRequirements> {
        let query = self.production_query_for_ids(roots, profiles, limits)?;
        Ok(graph::resolve(&mut self.snapshot, &query, profiles)?.production_requirements()?)
    }
    pub(crate) fn production_recipe(
        &mut self,
        root: &str,
        profiles: &[graph::Profile],
        consumption: &[graph::recipe::Consumption],
        operation: &graph::recipe::Operation,
        limits: graph::Limits,
    ) -> Result<graph::recipe::Plan> {
        let query = self.production_query(root, profiles, limits)?;
        let resolved = graph::resolve(&mut self.snapshot, &query, profiles)?;
        Ok(graph::recipe::plan(
            &resolved,
            &mut self.snapshot,
            profiles,
            consumption,
            operation,
            graph::recipe::Limits {
                nodes: limits.nodes,
                edges: limits.edges,
                bytes: limits.bytes,
            },
        )?)
    }
    pub(crate) fn projected_recipe_for_ids(
        &mut self,
        roots: &std::collections::BTreeSet<String>,
        profiles: &[graph::Profile],
        consumption: &[graph::recipe::Consumption],
        payloads: &[graph::recipe::PayloadConsumption],
        operation: &graph::recipe::Operation,
        limits: graph::Limits,
    ) -> Result<graph::recipe::Plan> {
        let query = self.production_query_for_ids(roots, profiles, limits)?;
        let resolved = graph::resolve(&mut self.snapshot, &query, profiles)?;
        Ok(graph::recipe::plan_with_payloads(
            &resolved,
            &mut self.snapshot,
            profiles,
            consumption,
            payloads,
            operation,
            graph::recipe::Limits {
                nodes: limits.nodes,
                edges: limits.edges,
                bytes: limits.bytes,
            },
        )?)
    }
    fn production_query(
        &self,
        root: &str,
        profiles: &[graph::Profile],
        limits: graph::Limits,
    ) -> Result<graph::Query> {
        for profile in profiles {
            if self.admission(&profile.definition_id)?.object != profile.definition {
                bail!("graph definition pin differs from source admission");
            }
        }
        let reference = self.record(root)?.reference().clone();
        Ok(graph::Query {
            checkpoint: self.checkpoint().clone(),
            roots: std::collections::BTreeSet::from([reference]),
            kinds: std::collections::BTreeSet::from([graph::EdgeKind::Production]),
            limits,
        })
    }
    fn production_query_for_ids(
        &self,
        roots: &std::collections::BTreeSet<String>,
        profiles: &[graph::Profile],
        limits: graph::Limits,
    ) -> Result<graph::Query> {
        if roots.is_empty() || roots.len() > limits.nodes {
            bail!("native root scope exceeds graph budget");
        }
        let first = roots
            .first()
            .ok_or_else(|| anyhow!("native root scope empty"))?;
        let mut query = self.production_query(first, profiles, limits)?;
        query.roots = roots
            .iter()
            .map(|id| self.record(id).map(|r| r.reference().clone()))
            .collect::<Result<_>>()?;
        Ok(query)
    }
    pub(crate) fn freeze_context_for_ids(
        &mut self,
        roots: &std::collections::BTreeSet<String>,
        profiles: &[graph::Profile],
        descriptor: &[u8],
        store: &mut impl NodeStore,
        limits: TreeLimits,
        query_limits: graph::Limits,
    ) -> Result<ProductionContext> {
        let query = self.production_query_for_ids(roots, profiles, query_limits)?;
        Ok(self
            .snapshot
            .freeze_production_context(&query, profiles, descriptor, store, limits)?)
    }
    pub(crate) fn freeze_context(
        &mut self,
        root: &str,
        profiles: &[graph::Profile],
        descriptor: &[u8],
        store: &mut impl NodeStore,
        limits: TreeLimits,
        query_limits: graph::Limits,
    ) -> Result<ProductionContext> {
        let query = self.production_query(root, profiles, query_limits)?;
        Ok(self
            .snapshot
            .freeze_production_context(&query, profiles, descriptor, store, limits)?)
    }
    pub(crate) fn replay_context(
        &mut self,
        selection: &Selection,
        profiles: &[graph::Profile],
        store: &mut impl NodeStore,
        native_limits: Limits,
        tree_limits: TreeLimits,
        query_limits: graph::Limits,
    ) -> Result<ProductionContext> {
        let (snapshot, context) = AssetSnapshot::from_production_context(
            selection,
            profiles,
            store,
            native_limits,
            tree_limits,
            query_limits,
        )?;
        if snapshot.checkpoint() != self.checkpoint() {
            bail!("context source checkpoint differs from admission");
        }
        self.snapshot = snapshot;
        Ok(context)
    }
    pub fn checkpoint(&self) -> &Digest {
        self.snapshot.checkpoint()
    }
    pub(crate) fn replay_produced<'a>(
        &mut self,
        claim: &'a graph::recipe::produced::Claim,
        admission: &tig_git::source::native::produced::Admission<'_>,
        store: &mut impl NodeStore,
    ) -> Result<tig_git::source::native::produced::ReplayedClaim<'a>> {
        let (snapshot, replay) =
            tig_git::source::native::produced::replay(claim, admission, store)?;
        if snapshot.checkpoint() != self.checkpoint() {
            bail!("produced claim source checkpoint differs from admission");
        }
        self.snapshot = snapshot;
        Ok(replay)
    }
    pub(crate) fn replay_produced_with_payloads<'a>(
        &mut self,
        claim: &'a graph::recipe::produced::Claim,
        admission: &tig_git::source::native::produced::Admission<'_>,
        payloads: &[graph::recipe::PayloadConsumption],
        store: &mut impl NodeStore,
    ) -> Result<tig_git::source::native::produced::ReplayedClaim<'a>> {
        let (snapshot, replay) = tig_git::source::native::produced::replay_with_payloads(
            claim, admission, payloads, store,
        )?;
        if snapshot.checkpoint() != self.checkpoint() {
            bail!("projected claim source checkpoint differs from admission");
        }
        self.snapshot = snapshot;
        Ok(replay)
    }
    pub fn byte_requests(&self) -> u32 {
        self.cache.requests()
    }
    pub fn active_readers(&self) -> u32 {
        self.cache.active_readers()
    }
    pub(crate) fn record(&self, id: &str) -> Result<tig_assets::resolve::Resolved> {
        token(id, 256)?;
        let asset = self.snapshot.resolve_id(id)?;
        let admission = self
            .admitted
            .get(&asset.definition().id)
            .ok_or_else(|| anyhow!("definition not admitted"))?;
        if admission.object != asset.definition().object {
            bail!("definition pin changed");
        }
        Ok(asset)
    }
    /// Resolve current binding metadata in this captured view, without media IO.
    /// Selection status is project data, not a grant of production authority.
    pub fn resolve_asset(&self, id: &str) -> Result<AssetRef> {
        let asset = self.record(id)?;
        let admission = &self.admitted[&asset.definition().id];
        let object = match &admission.payload {
            Payload::Selected {} => asset.selected()?,
            Payload::Stream { slot } => asset.stream(slot)?,
        };
        let state = asset
            .status()
            .get("selection_state")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow!("asset has no native selection state"))?;
        token(state, 128)?;
        Ok(AssetRef {
            logical_id: id.into(),
            sha256: object.sha256.as_str().into(),
            bytes: object.bytes,
            cache_uri: format!("cache://sha256/{}", object.sha256.as_str()),
            selection_state: state.into(),
        })
    }
    /// Admit a catalog record without requiring an unbuilt payload. This is not
    /// consumption authority: strict resolution still applies when a key is used.
    pub(crate) fn catalog_asset(&self, id: &str) -> Result<Option<AssetRef>> {
        let asset = self.record(id)?;
        let state = asset
            .status()
            .get("selection_state")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow!("asset has no native selection state"))?;
        token(state, 128)?;
        let unbuilt = matches!(asset.selected(), Err(tig_assets::resolve::Error::Planned));
        let missing = match &self.admitted[&asset.definition().id].payload {
            Payload::Selected {} => unbuilt,
            Payload::Stream { slot } => {
                unbuilt
                    && matches!(
                        asset.stream(slot),
                        Err(tig_assets::resolve::Error::MissingStream)
                    )
            }
        };
        if missing {
            Ok(None)
        } else {
            self.resolve_asset(id).map(Some)
        }
    }
    pub(crate) fn read_metadata(&mut self, object: &Object, max_bytes: usize) -> Result<Vec<u8>> {
        if max_bytes > JOB_BYTES_MAX || object.bytes > max_bytes as u64 {
            bail!("metadata exceeds byte budget");
        }
        let mut held = self.cache.open_verified(object)?;
        read_held(&mut held, max_bytes)
    }
    fn expected(&self, file: &FileRef) -> Result<(String, Object)> {
        let id = logical(&file.path)?;
        let asset = self.snapshot.resolve_id(&id)?;
        let admitted = self
            .admitted
            .get(&asset.definition().id)
            .ok_or_else(|| anyhow!("definition not admitted"))?;
        if admitted.object != asset.definition().object {
            bail!("definition pin changed");
        }
        let object = match &admitted.payload {
            Payload::Selected {} => asset.selected()?,
            Payload::Stream { slot } => asset.stream(slot)?,
        };
        if object.sha256.as_str() != file.sha256 || object.bytes != file.bytes {
            bail!("asset selection differs from expected pin");
        }
        Ok((id, object.clone()))
    }
    /// Adapter for read_verified_alignments_from_assets. Read/seek protection
    /// remains explicit and does not imply path-based renderer support.
    pub fn read_alignment(&mut self, expected: &AssetRef, max_bytes: usize) -> Result<Vec<u8>> {
        if max_bytes > crate::scene_authoring_inputs::ALIGNMENT_BYTES_MAX
            || expected.bytes > max_bytes as u64
        {
            bail!("alignment exceeds byte budget");
        }
        let file = FileRef {
            path: expected.logical_id.clone().into(),
            sha256: expected.sha256.clone(),
            bytes: expected.bytes,
        };
        let (_, object) = self.expected(&file)?;
        let mut held = self.cache.open_verified(&object)?;
        read_held(&mut held, max_bytes)
    }
    /// Hold an explicit subset of exact input pins, for compiler evidence.
    /// This is not a full job admission; render callers must use prepare(job).
    pub fn prepare_files(&mut self, files: &[FileRef]) -> Result<Inputs> {
        if !cfg!(windows) || self.required != RequiredProtection::PreventMutation {
            bail!("mandatory path protection unavailable");
        }
        let mut inputs = Inputs {
            checkpoint: self.checkpoint().clone(),
            names: BTreeMap::new(),
            sizes: BTreeMap::new(),
            held: BTreeMap::new(),
        };
        for file in files {
            self.register(&mut inputs, file)?;
        }
        let names = inputs.names.keys().cloned().collect::<Vec<_>>();
        for name in names {
            self.ensure(&mut inputs, &name)?;
        }
        Ok(inputs)
    }
    fn register(&self, inputs: &mut Inputs, file: &FileRef) -> Result<()> {
        let (name, object) = self.expected(file)?;
        if let Some(previous) = inputs.sizes.get(&object.sha256)
            && *previous != object.bytes
        {
            bail!("conflicting object sizes");
        }
        if let Some(previous) = inputs.names.get(&name) {
            if previous != &object {
                bail!("conflicting input bindings");
            }
            return Ok(());
        }
        if inputs.names.len() >= INPUT_NAMES_MAX {
            bail!("too many input names");
        }
        inputs.sizes.insert(object.sha256.clone(), object.bytes);
        inputs.names.insert(name, object);
        Ok(())
    }
    fn ensure(&mut self, inputs: &mut Inputs, name: &str) -> Result<()> {
        let object = inputs
            .names
            .get(name)
            .ok_or_else(|| anyhow!("unbound input"))?;
        if !inputs.held.contains_key(&object.sha256) {
            inputs
                .held
                .insert(object.sha256.clone(), self.cache.open_verified(object)?);
        }
        Ok(())
    }
    /// Own all media/evidence guards before rendering. No primary byte copies.
    /// Failure drops every partial guard; byte/request budgets remain charged.
    pub fn prepare(&mut self, job: &Job) -> Result<Inputs> {
        crate::scene_delivery::png_sequence::validate_counts(job)?;
        if !cfg!(windows) || self.required != RequiredProtection::PreventMutation {
            bail!("mandatory path protection unavailable");
        }
        let mut inputs = Inputs {
            checkpoint: self.checkpoint().clone(),
            names: BTreeMap::new(),
            sizes: BTreeMap::new(),
            held: BTreeMap::new(),
        };
        for picture in &job.pictures {
            self.register(&mut inputs, &picture.source)?;
        }
        for audio in &job.audio {
            self.register(&mut inputs, &audio.source)?;
            if let Some(offset) = &audio.placement_offset {
                self.register(&mut inputs, &offset.evidence)?;
            }
        }
        for layer in &job.external_layers {
            self.register(&mut inputs, &layer.evidence)?;
            for frame in &layer.frames {
                self.register(&mut inputs, frame)?;
            }
            for file in [&layer.font, &layer.render_source, &layer.derivation_receipt]
                .into_iter()
                .flatten()
            {
                self.register(&mut inputs, file)?;
            }
        }
        if let Some(camera) = &job.post_compose_camera {
            self.register(&mut inputs, &camera.evidence)?;
        }
        let mut metadata_bytes = 0u64;
        for layer in &job.external_layers {
            if let Some(receipt) = &layer.derivation_receipt {
                metadata_bytes = metadata_bytes
                    .checked_add(receipt.bytes)
                    .ok_or_else(|| anyhow!("metadata byte overflow"))?;
                if receipt.bytes > JOB_BYTES_MAX as u64 || metadata_bytes > METADATA_TOTAL_MAX {
                    bail!("derivation metadata exceeds budget");
                }
                let name = logical(&receipt.path)?;
                self.ensure(&mut inputs, &name)?;
                let object = &inputs.names[&name];
                let held = inputs
                    .held
                    .get_mut(&object.sha256)
                    .ok_or_else(|| anyhow!("missing byte guard"))?;
                let derivation: TimedOverlayDerivation =
                    serde_json::from_slice(&read_held(held, JOB_BYTES_MAX)?)
                        .map_err(|_| anyhow!("invalid selected derivation"))?;
                if derivation.schema != "reel.timed-overlay-derivation.v1"
                    || derivation.selected_evidence != layer.evidence
                    || layer.render_source.as_ref() != Some(&derivation.output)
                    || derivation.inputs.is_empty()
                    || !derivation.recipe.is_object()
                {
                    bail!("derivation differs from selected inputs");
                }
                for file in &derivation.inputs {
                    self.register(&mut inputs, file)?;
                }
            }
        }
        let names = inputs.names.keys().cloned().collect::<Vec<_>>();
        for name in names {
            self.ensure(&mut inputs, &name)?;
        }
        Ok(inputs)
    }
}
fn read_held(held: &mut VerifiedRead, max: usize) -> Result<Vec<u8>> {
    if held.object().bytes > max as u64 {
        bail!("metadata exceeds byte budget");
    }
    held.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    held.take(max as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != held.object().bytes {
        bail!("metadata size changed");
    }
    held.seek(SeekFrom::Start(0))?;
    Ok(bytes)
}

/// Immutable, deduplicated protected inputs. Keep this owner alive through the
/// complete synchronous operation; the source/catalog may be dropped earlier.
pub struct Inputs {
    checkpoint: Digest,
    names: BTreeMap<String, Object>,
    sizes: BTreeMap<Digest, u64>,
    held: BTreeMap<Digest, VerifiedRead>,
}
impl Inputs {
    pub fn checkpoint(&self) -> &Digest {
        &self.checkpoint
    }
    pub fn names(&self) -> usize {
        self.names.len()
    }
    pub fn objects(&self) -> usize {
        self.held.len()
    }
}
impl InputFiles for Inputs {
    fn verified_path(&self, expected: &FileRef) -> Result<PathBuf> {
        let name = logical(&expected.path)?;
        let object = self
            .names
            .get(&name)
            .ok_or_else(|| anyhow!("unprepared input"))?;
        if object.sha256.as_str() != expected.sha256 || object.bytes != expected.bytes {
            bail!("prepared input pin changed");
        }
        let held = self
            .held
            .get(&object.sha256)
            .ok_or_else(|| anyhow!("missing prepared guard"))?;
        Ok(held.with_path(Path::to_path_buf)?)
    }
}
