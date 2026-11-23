//! Bind affected film consumers to exact native Git inputs and retained origins.
//! Construction resolves metadata only. Admission still verifies live parents
//! and acquires child-owned claims through `upstream::Admission`.
use crate::{NativeProfile, SnapshotBudget, SnapshotInputs, error, upstream};
use reel_workflow::{
    Artifact, DependencyResult, ErrorKind, Packet, Result, canonical_digest, digest,
    film_rebuild as contract, make_packet,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const MAX_EXPORT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ExportFile {
    sha256: String,
    bytes: usize,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ExportManifest {
    schema: String,
    request_sha256: String,
    render_tasks: Vec<String>,
    files: BTreeMap<String, ExportFile>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Baseline {
    pub packet: Packet,
    pub source: NativeProfile,
    /// Explicit new closure bounds, not an implicit increase of the old limits.
    pub budget: SnapshotBudget,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub rebuild: contract::Request,
    pub baselines: BTreeMap<String, Baseline>,
    pub parents: upstream::Parents,
}
#[derive(Serialize)]
pub struct Bound {
    pub schema: &'static str,
    pub prepared: contract::Prepared,
    pub packets: BTreeMap<String, Packet>,
    pub sources: BTreeMap<String, NativeProfile>,
    pub native_metadata_verified: bool,
    pub live_parent_custody_verified: bool,
    pub child_admitted: bool,
}

pub fn bind(request: &Request) -> Result<Bound> {
    if request.schema != "reel.workflow-native-film-rebuild.v1" {
        return Err(error(ErrorKind::Invalid, "native-film-rebuild-schema"));
    }
    // The binder produces the new pins; accepting caller pins here would hide
    // a mismatch between the portable plan and its actual native closure.
    if request
        .rebuild
        .consumers
        .iter()
        .any(|c| c.native_inputs.is_some())
    {
        return Err(error(
            ErrorKind::Conflict,
            "native-film-rebuild-supplied-pin",
        ));
    }
    let mut prepared = contract::prepare(&request.rebuild)?;
    let ids: BTreeSet<_> = prepared.render_tasks.iter().cloned().collect();
    if request.baselines.keys().cloned().collect::<BTreeSet<_>>() != ids {
        return Err(error(ErrorKind::Conflict, "native-film-rebuild-baselines"));
    }
    let parents = request.parents.borrowed()?;
    let declared: BTreeMap<_, _> = std::iter::once(&request.rebuild.image)
        .chain(request.rebuild.controls.values())
        .map(|p| (p.packet.task.id.clone(), p))
        .collect();
    if parents.keys().collect::<BTreeSet<_>>() != declared.keys().collect::<BTreeSet<_>>() {
        return Err(error(ErrorKind::Conflict, "native-film-rebuild-parent-set"));
    }
    for (id, parent) in &parents {
        let expected = declared[id];
        if canonical_digest(&(parent.packet, parent.result))?
            != canonical_digest(&(&expected.packet, &expected.result))?
        {
            return Err(error(
                ErrorKind::Conflict,
                "native-film-rebuild-parent-subject",
            ));
        }
    }
    let mut sources = BTreeMap::new();
    let mut packets = BTreeMap::new();
    for id in &prepared.render_tasks {
        let baseline = &request.baselines[id];
        let original = make_packet(
            &request.rebuild.baseline,
            id,
            baseline.packet.dependencies.clone(),
        )?;
        if canonical_digest(&original)? != canonical_digest(&baseline.packet)? {
            return Err(error(
                ErrorKind::Conflict,
                "native-film-rebuild-original-packet",
            ));
        }
        // Exact old declaration, logical record, annex leaf, identity and
        // snapshot must resolve before constructing the modified closure.
        drop(SnapshotInputs::open(&original, &baseline.source.snapshot)?);
        let binding = &prepared.bindings[id];
        let needed: BTreeSet<_> = binding
            .inputs
            .values()
            .map(|r| r.task_id.as_str())
            .collect();
        let selected = parents
            .iter()
            .filter(|(id, _)| needed.contains(id.as_str()))
            .map(|(id, p)| {
                (
                    id.clone(),
                    upstream::Parent {
                        packet: p.packet,
                        result: p.result,
                        source: p.source,
                        admission: p.admission,
                        receipt: p.receipt,
                        acknowledgement: p.acknowledgement,
                    },
                )
            })
            .collect();
        let mut snapshot = baseline.source.snapshot.clone();
        snapshot.schema = "reel.workflow-native-snapshot.v3".into();
        snapshot.budget = Some(baseline.budget.clone());
        snapshot.upstream = Some(upstream::Frozen::capture(binding, &selected));
        let raw = snapshot.declaration_bytes()?;
        let task = prepared
            .plan
            .tasks
            .iter_mut()
            .find(|t| &t.id == id)
            .unwrap();
        task.native_inputs = Some(Artifact {
            logical_id: format!("workflow/rebuild/{id}/native-inputs"),
            sha256: digest(&raw),
            bytes: raw.len() as u64,
            media_type: "application/json".into(),
        });
        sources.insert(
            id.clone(),
            NativeProfile {
                snapshot,
                annex: baseline.source.annex.clone(),
                source: baseline.source.source.clone(),
            },
        );
    }
    for id in &prepared.render_tasks {
        let mut dependencies = request.baselines[id].packet.dependencies.clone();
        for producer in prepared.bindings[id].inputs.values().map(|r| &r.task_id) {
            let parent = &parents[producer];
            dependencies.insert(
                producer.clone(),
                DependencyResult {
                    result_sha256: canonical_digest(parent.result)?,
                    artifacts: parent.result.artifacts.clone(),
                },
            );
        }
        let packet = make_packet(&prepared.plan, id, dependencies)?;
        let reader_limit = sources[id].source.maximum_bytes;
        if reader_limit == 0
            || reader_limit > 64 * 1024 * 1024 * 1024
            || packet.task.inputs.iter().any(|a| a.bytes > reader_limit)
        {
            return Err(error(
                ErrorKind::Budget,
                "native-film-rebuild-reader-envelope",
            ));
        }
        drop(SnapshotInputs::open(&packet, &sources[id].snapshot)?);
        packets.insert(id.clone(), packet);
    }
    prepared.native_binding_required.clear();
    Ok(Bound {
        schema: "reel.workflow-native-film-rebuild-bound.v1",
        prepared,
        packets,
        sources,
        native_metadata_verified: true,
        live_parent_custody_verified: false,
        child_admitted: false,
    })
}

/// Private, create-only metadata package. Every file is prepared before the
/// destination is created; interrupted writes remain preserved for recovery.
pub fn export(path: &Path, pin: &str, directory: &Path) -> Result<String> {
    use std::{
        fs::{self, OpenOptions},
        io::Write,
    };
    let (raw, _held) = crate::staging::read_held(path, reel_workflow::MAX_CONTROL_BYTES as u64)?;
    if digest(&raw) != pin {
        return Err(error(
            ErrorKind::Integrity,
            "native-film-rebuild-request-pin",
        ));
    }
    let request: Request = reel_workflow::decode(&raw)?;
    let bound = bind(&request)?;
    let mut files = BTreeMap::from([("bound.json".into(), crate::admission::encode(&bound)?)]);
    for (id, packet) in &bound.packets {
        files.insert(
            format!("{id}-packet.json"),
            crate::admission::encode(packet)?,
        );
        let source = &bound.sources[id];
        files.insert(
            format!("{id}-source.json"),
            crate::admission::encode(source)?,
        );
        let needed = &source.snapshot.upstream.as_ref().unwrap().parents;
        let parents: BTreeMap<_, _> = request
            .parents
            .parents
            .iter()
            .filter(|(id, _)| needed.contains_key(*id))
            .collect();
        files.insert(
            format!("{id}-parents.json"),
            crate::admission::encode(
                &serde_json::json!({"schema":"reel.workflow-native-parents.v1","parents":parents}),
            )?,
        );
    }
    let inventory = files
        .iter()
        .map(|(name, raw)| {
            (
                name.clone(),
                ExportFile {
                    sha256: digest(raw),
                    bytes: raw.len(),
                },
            )
        })
        .collect();
    let manifest = crate::admission::encode(&ExportManifest {
        schema: "reel.workflow-native-film-rebuild-export.v1".into(),
        request_sha256: pin.into(),
        render_tasks: bound.prepared.render_tasks,
        files: inventory,
    })?;
    if manifest.len() > reel_workflow::MAX_CONTROL_BYTES
        || files
            .values()
            .any(|v| v.len() > reel_workflow::MAX_CONTROL_BYTES)
        || files.values().map(Vec::len).sum::<usize>() + manifest.len() > MAX_EXPORT_BYTES
    {
        return Err(error(
            ErrorKind::Budget,
            "native-film-rebuild-package-bytes",
        ));
    }
    fs::create_dir(directory).map_err(crate::io_error)?;
    let manifest_pin = digest(&manifest);
    for (name, raw) in files.into_iter().chain([("export.json".into(), manifest)]) {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(name))
            .map_err(crate::io_error)?;
        file.write_all(&raw)
            .and_then(|_| file.sync_all())
            .map_err(crate::io_error)?;
    }
    Ok(manifest_pin)
}

/// Check the complete export before dispatch. The returned task order is
/// bound to both the original request and the published manifest.
pub fn verify_export(directory: &Path, request_pin: &str, export_pin: &str) -> Result<Vec<String>> {
    let limit = reel_workflow::MAX_CONTROL_BYTES;
    let (raw, _held) = crate::staging::read_held(&directory.join("export.json"), limit as u64)?;
    if digest(&raw) != export_pin {
        return Err(error(
            ErrorKind::Integrity,
            "native-film-rebuild-export-pin",
        ));
    }
    let manifest: ExportManifest = reel_workflow::decode(&raw)?;
    if manifest.schema != "reel.workflow-native-film-rebuild-export.v1"
        || manifest.request_sha256 != request_pin
        || manifest.render_tasks.is_empty()
        || manifest.render_tasks.len() > 128
    {
        return Err(error(
            ErrorKind::Invalid,
            "native-film-rebuild-export-manifest",
        ));
    }
    let mut names = BTreeSet::from(["bound.json".to_owned()]);
    for id in &manifest.render_tasks {
        if !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
            || id.is_empty()
            || id.len() > 128
            || matches!(id.as_str(), "." | "..")
            || !names.insert(format!("{id}-packet.json"))
        {
            return Err(error(ErrorKind::Invalid, "native-film-rebuild-export-task"));
        }
        names.insert(format!("{id}-source.json"));
        names.insert(format!("{id}-parents.json"));
    }
    if names.iter().ne(manifest.files.keys()) {
        return Err(error(
            ErrorKind::Conflict,
            "native-film-rebuild-export-files",
        ));
    }
    let mut total = raw.len();
    for (name, expected) in manifest.files {
        if expected.bytes > limit || expected.bytes > MAX_EXPORT_BYTES - total {
            return Err(error(
                ErrorKind::Budget,
                "native-film-rebuild-package-bytes",
            ));
        }
        let (bytes, _held) =
            crate::staging::read_held(&directory.join(name), expected.bytes as u64)?;
        if bytes.len() != expected.bytes || digest(&bytes) != expected.sha256 {
            return Err(error(
                ErrorKind::Integrity,
                "native-film-rebuild-export-file",
            ));
        }
        total += bytes.len();
    }
    Ok(manifest.render_tasks)
}
