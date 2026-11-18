//! Owned, bounded input exports. No provider dispatch, output ingestion,
//! retention, expiry, capacity release, workspace deletion or asset selection.
use crate::{
    NativeInputs, NativeProfile,
    admission::{self, Receipt},
    error, io_error,
};
use reel_assets::{
    admission as capacity,
    jobs::{WorkspaceDirectory, open_workspace_directory},
};
use reel_workflow::{Artifact, ErrorKind, Packet, Result, publication::InputReader};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tig_core::Object;
const MAX_BYTES: u64 = 512 * 1024 * 1024;
const MAX_FILES: usize = 256;
const MAX_ENTRIES: usize = 2048;
const ATTEMPTS: usize = 4;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub parent: PathBuf,
    pub maximum_bytes: u64,
    pub capacity_policy: capacity::Policy,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Owner {
    name: String,
    parent_identity: String,
    identity: String,
    manifest: Object,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Reservation {
    id: String,
    volume: String,
}
/// Private operational control. Its exact raw bytes must be pinned externally.
/// Deserialization alone grants no admission or workspace ownership.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Ticket {
    schema: String,
    profile: Profile,
    reservation: Reservation,
    admission: Receipt,
    owner: Owner,
}
impl Ticket {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        json(self)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    object: Object,
    input: Option<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    operation: String,
    files: BTreeMap<String, Entry>,
}
struct Plan {
    manifest: Manifest,
    controls: BTreeMap<String, Vec<u8>>,
}
#[derive(Serialize)]
pub struct Summary {
    pub staged_files: usize,
    pub logical_bytes: u64,
    pub already_complete: bool,
}
fn json(value: &impl Serialize) -> Result<Vec<u8>> {
    let raw =
        serde_json::to_vec(value).map_err(|_| error(ErrorKind::Invalid, "staging-control-json"))?;
    if raw.len() > reel_workflow::MAX_CONTROL_BYTES {
        return Err(error(ErrorKind::Budget, "staging-control-bytes"));
    }
    Ok(raw)
}
fn scoped_error(_: impl std::fmt::Display) -> reel_workflow::Error {
    error(ErrorKind::NeedsRecovery, "staging-preserve-original-state")
}
fn ordinary(metadata: &fs::Metadata, directory: bool) -> bool {
    #[cfg(windows)]
    let linked = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let linked = metadata.file_type().is_symlink();
    !linked
        && if directory {
            metadata.is_dir()
        } else {
            metadata.is_file()
        }
}
fn read_held(path: &Path, maximum: u64) -> Result<(Vec<u8>, File)> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1).custom_flags(0x0020_0000);
    }
    let mut file = options.open(path).map_err(io_error)?;
    let metadata = file.metadata().map_err(io_error)?;
    if !ordinary(&metadata, false) {
        return Err(error(ErrorKind::Denied, "staging-ordinary-file-required"));
    }
    if metadata.len() > maximum {
        return Err(error(ErrorKind::Budget, "staging-file-bytes"));
    }
    let mut raw = Vec::new();
    Read::by_ref(&mut file)
        .take(maximum + 1)
        .read_to_end(&mut raw)
        .map_err(io_error)?;
    if raw.len() as u64 > maximum {
        return Err(error(ErrorKind::Budget, "staging-file-growth"));
    }
    Ok((raw, file))
}
fn portable(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 256
        && path.split('/').count() <= 16
        && path.split('/').all(|part| {
            let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
            !part.is_empty()
                && !matches!(part, "." | "..")
                && !part.ends_with('.')
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
                && !matches!(
                    stem.as_str(),
                    "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
                )
                && !(stem.len() == 4
                    && (stem.starts_with("COM") || stem.starts_with("LPT"))
                    && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        })
}
fn plan(packet: &Packet, source: &NativeProfile, receipt: &Receipt) -> Result<Plan> {
    let mut controls: BTreeMap<String, Vec<u8>> = BTreeMap::from([
        ("packet.json".into(), json(packet)?),
        ("admission.json".into(), receipt.canonical_bytes()?),
        (
            "native-inputs.json".into(),
            source.snapshot.declaration_bytes()?,
        ),
    ]);
    if !packet.task.prompt.is_empty() {
        controls.insert("prompt.txt".into(), packet.task.prompt.as_bytes().to_vec());
    }
    let mut files = BTreeMap::new();
    let mut mapped = BTreeMap::<String, Vec<String>>::new();
    if let Some(tool) = &packet.task.tool {
        mapped
            .entry(tool.request.logical_id.clone())
            .or_default()
            .push("source/request.json".into());
        for (path, id) in &tool.source_bindings {
            mapped
                .entry(id.clone())
                .or_default()
                .push(format!("source/{path}"));
        }
    }
    let mut bindings = BTreeMap::new();
    for (index, asset) in packet.task.inputs.iter().enumerate() {
        let names = mapped
            .remove(&asset.logical_id)
            .unwrap_or_else(|| vec![format!("inputs/{index:03}.bin")]);
        bindings.insert(asset.logical_id.clone(), names[0].clone());
        for name in names {
            if files
                .insert(
                    name,
                    Entry {
                        object: Object {
                            sha256: tig_core::Digest::parse(&asset.sha256)
                                .map_err(|_| error(ErrorKind::Invalid, "staging-input-pin"))?,
                            bytes: asset.bytes,
                        },
                        input: Some(asset.logical_id.clone()),
                    },
                )
                .is_some()
            {
                return Err(error(ErrorKind::Conflict, "staging-path-alias"));
            }
        }
    }
    if !mapped.is_empty() {
        return Err(error(
            ErrorKind::Conflict,
            "staging-unused-producer-bindings",
        ));
    }
    controls.insert("input-bindings.json".into(), json(&bindings)?);
    for (path, raw) in &controls {
        if files
            .insert(
                path.clone(),
                Entry {
                    object: Object::from_bytes(raw),
                    input: None,
                },
            )
            .is_some()
        {
            return Err(error(ErrorKind::Conflict, "staging-control-alias"));
        }
    }
    if files.len() > MAX_FILES {
        return Err(error(ErrorKind::Budget, "staging-file-count"));
    }
    let mut folded = BTreeSet::new();
    for path in files.keys() {
        if !portable(path) || !folded.insert(path.to_ascii_lowercase()) {
            return Err(error(ErrorKind::Invalid, "staging-portable-path"));
        }
    }
    for path in &folded {
        for (at, _) in path.match_indices('/') {
            if folded.contains(&path[..at]) {
                return Err(error(ErrorKind::Conflict, "staging-file-directory-alias"));
            }
        }
    }
    Ok(Plan {
        manifest: Manifest {
            schema: "reel.workflow-staging-manifest.v1".into(),
            operation: receipt.operation_identity()?,
            files,
        },
        controls,
    })
}
fn configuration(profile: &Profile, coordination: &Path) -> Result<reel_assets::Config> {
    if profile.schema != "reel.workflow-staging.v1"
        || profile.maximum_bytes == 0
        || profile.maximum_bytes > MAX_BYTES
        || profile.maximum_bytes > profile.capacity_policy.max_object_bytes
    {
        return Err(error(ErrorKind::Budget, "staging-profile-bounds"));
    }
    profile.capacity_policy.validate().map_err(scoped_error)?;
    Ok(reel_assets::Config {
        cache_roots: vec![profile.parent.clone()],
        coordination_root: Some(coordination.into()),
        capacity: profile.capacity_policy.clone(),
        ..reel_assets::Config::default()
    })
}
fn check_parent(parent: &Path, source: &NativeProfile, profile: &admission::Profile) -> Result<()> {
    if !cfg!(windows) {
        return Err(error(
            ErrorKind::Unsupported,
            "staging-requires-qualified-windows-scope",
        ));
    }
    let canonical = parent.canonicalize().map_err(io_error)?;
    for root in [
        &profile.index_root,
        &profile.metadata_root,
        &profile.scope.coordination_root,
        &source.source.pool.repository,
    ] {
        let root = root.canonicalize().unwrap_or_else(|_| root.clone());
        if canonical.starts_with(root) {
            return Err(error(ErrorKind::Denied, "staging-parent-managed-root"));
        }
    }
    Ok(())
}
fn checked_budget(profile: &Profile, plan: &Plan) -> Result<()> {
    let bytes = plan
        .manifest
        .files
        .values()
        .try_fold(0u64, |n, e| n.checked_add(e.object.bytes))
        .and_then(|n| n.checked_mul(2))
        .and_then(|n| n.checked_add(2 * reel_workflow::MAX_CONTROL_BYTES as u64))
        .ok_or_else(|| error(ErrorKind::Budget, "staging-byte-overflow"))?;
    if bytes > profile.maximum_bytes {
        return Err(error(ErrorKind::Budget, "staging-copy-and-evidence-budget"));
    }
    Ok(())
}
fn create_owner(plan: &Plan, parent: &WorkspaceDirectory) -> Result<Owner> {
    let name = format!("handoff-{}", plan.manifest.operation);
    let path = parent.path().join(&name);
    fs::create_dir(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            error(
                ErrorKind::Conflict,
                "staging-existing-workspace-needs-ticket",
            )
        } else {
            io_error(e)
        }
    })?;
    let workspace = open_workspace_directory(&path).map_err(scoped_error)?;
    let raw = json(&plan.manifest)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path.join("intent.json"))
        .map_err(io_error)?;
    file.write_all(&raw)
        .and_then(|_| file.sync_all())
        .map_err(scoped_error)?;
    Ok(Owner {
        name,
        parent_identity: parent.identity().into(),
        identity: workspace.identity().into(),
        manifest: Object::from_bytes(&raw),
    })
}
/// First phase only: actual claims reopen, explicit capacity admission, fresh
/// owned directory/intent. Persist and pin the returned ticket before export.
/// Initial uncertainty preserves the directory and conservative capacity record.
pub fn prepare(
    packet: &Packet,
    source: &NativeProfile,
    admission: &admission::Profile,
    receipt: &Receipt,
    profile: &Profile,
) -> Result<Ticket> {
    let _claims = admission::Prepared::open(packet, source, admission, &receipt.attempt_id)?
        .reopen(receipt)?;
    check_parent(&profile.parent, source, admission)?;
    let parent = open_workspace_directory(&profile.parent).map_err(scoped_error)?;
    let mut profile = profile.clone();
    if let Some(minimum) = profile
        .capacity_policy
        .tier_minimum_free_bytes
        .get(&profile.parent)
    {
        profile.capacity_policy.minimum_free_bytes =
            profile.capacity_policy.minimum_free_bytes.max(*minimum);
    }
    profile.parent = parent.path().into();
    let config = configuration(&profile, &admission.scope.coordination_root)?;
    let plan = plan(packet, source, receipt)?;
    checked_budget(&profile, &plan)?;
    if fs::symlink_metadata(
        parent
            .path()
            .join(format!("handoff-{}", plan.manifest.operation)),
    )
    .is_ok()
    {
        return Err(error(
            ErrorKind::Conflict,
            "staging-existing-workspace-needs-ticket",
        ));
    }
    let reservation = capacity::reserve_on(
        &profile.parent,
        &config,
        &profile.parent,
        profile.maximum_bytes,
    )
    .map_err(scoped_error)?;
    let result = create_owner(&plan, &parent).map(|owner| Ticket {
        schema: "reel.workflow-staging-ticket.v1".into(),
        profile,
        reservation: Reservation {
            id: reservation.id.clone(),
            volume: reservation.volume_id.clone(),
        },
        admission: receipt.clone(),
        owner,
    });
    reservation.retain_for_recovery();
    result
}
struct View {
    _parent: WorkspaceDirectory,
    root: WorkspaceDirectory,
    _directories: Vec<WorkspaceDirectory>,
    files: Vec<File>,
    usage: u64,
}
fn open_owner(profile: &Profile, owner: &Owner, plan: &Plan) -> Result<View> {
    let parent = open_workspace_directory(&profile.parent).map_err(scoped_error)?;
    if parent.identity() != owner.parent_identity
        || owner.name != format!("handoff-{}", plan.manifest.operation)
    {
        return Err(error(
            ErrorKind::Conflict,
            "staging-parent-or-operation-identity",
        ));
    }
    let root = open_workspace_directory(&parent.path().join(&owner.name)).map_err(scoped_error)?;
    if root.identity() != owner.identity {
        return Err(error(ErrorKind::Conflict, "staging-workspace-replaced"));
    }
    let (raw, guard) = read_held(
        &root.path().join("intent.json"),
        reel_workflow::MAX_CONTROL_BYTES as u64,
    )?;
    if Object::from_bytes(&raw) != owner.manifest || raw != json(&plan.manifest)? {
        return Err(error(ErrorKind::Integrity, "staging-intent-binding"));
    }
    Ok(View {
        _parent: parent,
        root,
        _directories: Vec::new(),
        files: vec![guard],
        usage: raw.len() as u64,
    })
}
fn completion(plan: &Plan, ticket_hash: &str) -> Result<Vec<u8>> {
    json(
        &serde_json::json!({"schema":"reel.workflow-staged-inputs.v1","ticket_sha256":ticket_hash,"manifest":Object::from_bytes(&json(&plan.manifest)?),"files":plan.manifest.files.len(),"retained_owner_acknowledged":false,"task_completed":false,"selected":false}),
    )
}
fn scan(view: &mut View, profile: &Profile, plan: &Plan, complete: &[u8]) -> Result<bool> {
    let mut wanted = BTreeMap::from([(
        "intent.json".into(),
        view.root
            .path()
            .join("intent.json")
            .metadata()
            .map_err(io_error)?
            .len(),
    )]);
    for (path, entry) in &plan.manifest.files {
        wanted.insert(path.clone(), entry.object.bytes);
    }
    wanted.insert("complete.json".into(), complete.len() as u64);
    let mut directories = BTreeSet::from(["parts".to_owned()]);
    for path in wanted.keys() {
        for (at, _) in path.match_indices('/') {
            directories.insert(path[..at].to_owned());
        }
    }
    let mut stack = vec![(view.root.path().to_path_buf(), String::new())];
    let mut count = 0usize;
    let mut seen = BTreeSet::new();
    view.usage = 0;
    while let Some((directory, prefix)) = stack.pop() {
        for item in fs::read_dir(directory).map_err(io_error)? {
            let item = item.map_err(io_error)?;
            count += 1;
            if count > MAX_ENTRIES {
                return Err(error(ErrorKind::Budget, "staging-entry-count"));
            }
            let name = item
                .file_name()
                .into_string()
                .map_err(|_| error(ErrorKind::Invalid, "staging-unicode-path"))?;
            let relative = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let metadata = fs::symlink_metadata(item.path()).map_err(io_error)?;
            if ordinary(&metadata, true) {
                if !directories.contains(&relative) {
                    return Err(error(ErrorKind::Conflict, "staging-unexpected-directory"));
                }
                view._directories
                    .push(open_workspace_directory(&item.path()).map_err(scoped_error)?);
                stack.push((item.path(), relative));
                continue;
            }
            if !ordinary(&metadata, false) {
                return Err(error(ErrorKind::Denied, "staging-nonregular-entry"));
            }
            let maximum = if let Some(maximum) = wanted.get(&relative) {
                *maximum
            } else {
                let mut found = None;
                for (index, entry) in plan.manifest.files.values().enumerate() {
                    for attempt in 0..ATTEMPTS {
                        if relative == format!("parts/{index:03}-{attempt}.part") {
                            found = Some(entry.object.bytes);
                        }
                    }
                }
                for attempt in 0..ATTEMPTS {
                    if relative == format!("parts/{:03}-{attempt}.part", plan.manifest.files.len())
                    {
                        found = Some(complete.len() as u64);
                    }
                }
                found.ok_or_else(|| error(ErrorKind::Conflict, "staging-unexpected-file"))?
            };
            let (raw, file) = read_held(&item.path(), maximum)?;
            view.usage = view
                .usage
                .checked_add(raw.len() as u64)
                .ok_or_else(|| error(ErrorKind::Budget, "staging-byte-overflow"))?;
            if view.usage > profile.maximum_bytes {
                return Err(error(ErrorKind::Budget, "staging-workspace-bytes"));
            }
            if relative == "complete.json" && raw != complete {
                return Err(error(ErrorKind::Integrity, "staging-completion-content"));
            }
            if let Some(entry) = plan.manifest.files.get(&relative)
                && Object::from_bytes(&raw) != entry.object
            {
                return Err(error(
                    ErrorKind::Integrity,
                    "staging-existing-input-content",
                ));
            }
            // Part guards are deliberately dropped before hard-link publication.
            if !relative.starts_with("parts/") {
                view.files.push(file);
            }
            seen.insert(relative);
        }
    }
    let completed = seen.contains("complete.json");
    if completed && plan.manifest.files.keys().any(|name| !seen.contains(name)) {
        return Err(error(
            ErrorKind::Integrity,
            "staging-completed-input-missing",
        ));
    }
    Ok(completed)
}
fn space(profile: &Profile, view: &View, additional: u64) -> Result<()> {
    if view
        .usage
        .checked_add(additional)
        .is_none_or(|n| n > profile.maximum_bytes)
        || !profile.capacity_policy.fits(
            fs2::available_space(view.root.path()).map_err(io_error)?,
            fs2::total_space(view.root.path()).map_err(io_error)?,
            0,
            additional,
        )
    {
        return Err(error(
            ErrorKind::Budget,
            "staging-capacity-or-workspace-budget",
        ));
    }
    Ok(())
}
fn directory(view: &mut View, path: &Path) -> Result<()> {
    let relative = path
        .strip_prefix(view.root.path())
        .map_err(|_| error(ErrorKind::Denied, "staging-target-scope"))?;
    let mut current = view.root.path().to_path_buf();
    for part in relative.components() {
        current.push(part);
        match fs::create_dir(&current) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(io_error(e)),
        };
        view._directories
            .push(open_workspace_directory(&current).map_err(scoped_error)?);
    }
    Ok(())
}
fn publish(
    view: &mut View,
    profile: &Profile,
    path: &str,
    index: usize,
    raw: &[u8],
    hook: &mut impl FnMut(usize) -> Result<()>,
) -> Result<()> {
    let target = view.root.path().join(path);
    directory(
        view,
        target
            .parent()
            .ok_or_else(|| error(ErrorKind::Invalid, "staging-target-parent"))?,
    )?;
    if fs::symlink_metadata(&target).is_ok() {
        let (existing, guard) = read_held(&target, raw.len() as u64)?;
        if existing != raw {
            return Err(error(
                ErrorKind::Integrity,
                "staging-existing-target-content",
            ));
        }
        view.files.push(guard);
        return Ok(());
    }
    directory(view, &view.root.path().join("parts"))?;
    for attempt in 0..ATTEMPTS {
        let part = view
            .root
            .path()
            .join(format!("parts/{index:03}-{attempt}.part"));
        if fs::symlink_metadata(&part).is_ok() {
            let (existing, guard) = read_held(&part, raw.len() as u64)?;
            drop(guard);
            if existing != raw {
                continue;
            }
        } else {
            space(profile, view, raw.len() as u64)?;
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            let mut file = options.open(&part).map_err(io_error)?;
            file.write_all(raw)
                .and_then(|_| file.sync_all())
                .map_err(scoped_error)?;
            drop(file);
            view.usage += raw.len() as u64;
            hook(index)?;
        }
        space(profile, view, raw.len() as u64)?;
        fs::hard_link(&part, &target).map_err(io_error)?;
        let (actual, guard) = read_held(&target, raw.len() as u64)?;
        if actual != raw {
            return Err(error(
                ErrorKind::Integrity,
                "staging-published-input-content",
            ));
        }
        view.usage += raw.len() as u64;
        view.files.push(guard);
        return Ok(());
    }
    Err(error(
        ErrorKind::NeedsRecovery,
        "staging-part-attempt-bound-preserved",
    ))
}
struct Buffers(BTreeMap<String, (Artifact, Vec<u8>)>);
impl InputReader for Buffers {
    fn read(&mut self, asset: &Artifact, maximum: u64) -> Result<Vec<u8>> {
        let (expected, raw) = self
            .0
            .get(&asset.logical_id)
            .ok_or_else(|| error(ErrorKind::Missing, "staging-buffer-input"))?;
        if asset != expected || asset.bytes > maximum {
            return Err(error(ErrorKind::Conflict, "staging-buffer-binding"));
        }
        Ok(raw.clone())
    }
}
fn materialize(
    view: &mut View,
    profile: &Profile,
    plan: &Plan,
    buffers: &Buffers,
    ticket_hash: &str,
    hook: &mut impl FnMut(usize) -> Result<()>,
) -> Result<Summary> {
    let complete = completion(plan, ticket_hash)?;
    let resumed = scan(view, profile, plan, &complete)?;
    for (index, (path, entry)) in plan.manifest.files.iter().enumerate() {
        let raw = match &entry.input {
            Some(id) => {
                &buffers
                    .0
                    .get(id)
                    .ok_or_else(|| error(ErrorKind::Missing, "staging-payload-buffer"))?
                    .1
            }
            None => &plan.controls[path],
        };
        if Object::from_bytes(raw) != entry.object {
            return Err(error(ErrorKind::Integrity, "staging-buffer-content"));
        }
        publish(view, profile, path, index, raw, hook)?;
    }
    // Recheck all inputs before committing the completion marker.
    scan(view, profile, plan, &complete)?;
    publish(
        view,
        profile,
        "complete.json",
        plan.manifest.files.len(),
        &complete,
        hook,
    )?;
    scan(view, profile, plan, &complete)?;
    Ok(Summary {
        staged_files: plan.manifest.files.len(),
        logical_bytes: view.usage,
        already_complete: resumed,
    })
}
/// Second phase: externally pinned ticket, actual original claim guard, original
/// native reads, preserved capacity lease and exclusive immutable file writes.
/// All guards stay live through final completion verification.
pub fn export(
    packet: &Packet,
    source: &NativeProfile,
    admission: &admission::Profile,
    ticket_bytes: &[u8],
    expected_sha256: &str,
) -> Result<Summary> {
    if reel_workflow::digest(ticket_bytes) != expected_sha256 {
        return Err(error(ErrorKind::Integrity, "staging-ticket-pin"));
    }
    let ticket: Ticket = reel_workflow::decode(ticket_bytes)?;
    if ticket.schema != "reel.workflow-staging-ticket.v1" {
        return Err(error(ErrorKind::Invalid, "staging-ticket-schema"));
    }
    let _claims =
        admission::Prepared::open(packet, source, admission, &ticket.admission.attempt_id)?
            .reopen(&ticket.admission)?;
    check_parent(&ticket.profile.parent, source, admission)?;
    let config = configuration(&ticket.profile, &admission.scope.coordination_root)?;
    let plan = plan(packet, source, &ticket.admission)?;
    checked_budget(&ticket.profile, &plan)?;
    let mut view = open_owner(&ticket.profile, &ticket.owner, &plan)?;
    let _reservation = capacity::resume_preserved_on(
        &config,
        &ticket.profile.parent,
        &ticket.reservation.id,
        &ticket.reservation.volume,
        ticket.profile.maximum_bytes,
    )
    .map_err(scoped_error)?;
    scan(
        &mut view,
        &ticket.profile,
        &plan,
        &completion(&plan, expected_sha256)?,
    )?;
    let mut reader = NativeInputs::open(packet, source)?;
    let mut buffers = Buffers(BTreeMap::new());
    for asset in &packet.task.inputs {
        let raw = reader.read(asset, asset.bytes)?;
        buffers
            .0
            .insert(asset.logical_id.clone(), (asset.clone(), raw));
    }
    if packet.task.tool.is_some() {
        reel_workflow::publication::bind_comic(packet, &mut buffers)?;
    }
    materialize(
        &mut view,
        &ticket.profile,
        &plan,
        &buffers,
        expected_sha256,
        &mut |_| Ok(()),
    )
}

pub fn export_file(
    packet: &Packet,
    source: &NativeProfile,
    admission: &admission::Profile,
    path: &Path,
    expected_sha256: &str,
) -> Result<Summary> {
    let (raw, _guard) = read_held(path, reel_workflow::MAX_CONTROL_BYTES as u64)?;
    export(packet, source, admission, &raw, expected_sha256)
}

#[cfg(all(test, windows))]
#[path = "staging_tests.rs"]
mod tests;
