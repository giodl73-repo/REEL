//! Shared explicit history service; compiled hosts supply admitted providers.
#![forbid(unsafe_code)]
use crate::source::{
    GitId, Limits as GitLimits, Repository, Scope,
    native::{Limits as CaptureLimits, metadata_reader::CacheMetadataReader, updates::publication},
    proposal::{Identity, Replacement},
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};
use tig_assets::revisions::Reader;
use tig_assets::{
    edits::Change,
    native::Pin,
    revisions::{Coverage, Limits},
    schema::Dialect,
};
use tig_core::{LogicalPath, Object, identity::parse_json};
use tig_store::{
    AckLevel, Key, PublicationBackend,
    existing::{Layout, Profile as ByteProfile},
};

fn flat() -> Layout {
    Layout::Flat
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Publication {
    pub root: PathBuf,
    #[serde(default = "flat")]
    pub layout: Layout,
    #[serde(default)]
    pub settings: Value,
    pub operation: Key,
    pub owner: Key,
    pub required: AckLevel,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Proposal {
    path: String,
    identity: Identity,
    message: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    git_executable: PathBuf,
    repository: PathBuf,
    base: String,
    records: Vec<String>,
    definitions: Vec<String>,
    asset: String,
    expected: Object,
    dialect: String,
    #[serde(default)]
    limits: CaptureLimits,
    byte_source: Option<ByteProfile>,
    publication: Option<Publication>,
    proposal: Option<Proposal>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    path: PathBuf,
    expected: Object,
}
#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "kebab-case", deny_unknown_fields)]
enum Request {
    ConfigureDelivery {
        document: Document,
    },
    RegisterRevision {
        document: Document,
        coverage: Coverage,
    },
    RecordDelivery {
        document: Document,
        coverage: Coverage,
    },
    SelectRevision {
        revision: Object,
        representation: String,
        subject: Pin,
        status: Map<String, Value>,
        streams: Vec<String>,
    },
}
fn bytes(path: &Path) -> Result<Vec<u8>, String> {
    let mut raw = Vec::new();
    File::open(path)
        .map_err(|_| "cannot open explicit input")?
        .take(65_537)
        .read_to_end(&mut raw)
        .map_err(|_| "cannot read explicit input")?;
    if raw.len() > 65_536 {
        return Err("explicit input exceeds budget".into());
    }
    Ok(raw)
}
fn input<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    serde_json::from_value(parse_json(&bytes(path)?).map_err(|_| "invalid input JSON")?)
        .map_err(|_| "invalid input fields".into())
}
fn scope(paths: Vec<String>) -> Result<Scope, String> {
    Scope::new(
        paths
            .iter()
            .map(|p| LogicalPath::parse(p))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "invalid scope")?,
    )
    .map_err(|e| e.to_string())
}
/// File inputs are explicit, bounded and private. A host never supplies policy authority.
pub fn execute<B: PublicationBackend>(
    action: &str,
    profile_path: &Path,
    request_path: &Path,
    open: impl FnOnce(&Publication) -> Result<B, publication::Error>,
) -> Result<Value, String> {
    if !matches!(action, "check" | "publish" | "propose") {
        return Err("usage: tig-history check|publish|propose PROFILE REQUEST".into());
    }
    let profile: Profile = input(profile_path)?;
    let request: Request = input(request_path)?;
    if profile.schema != "tig.history-command.experimental-1" {
        return Err("unsupported history profile".into());
    }
    if action != "check" && profile.publication.is_none() {
        return Err("explicit publication profile required".into());
    }
    if action == "propose" && profile.proposal.is_none() {
        return Err("explicit proposal profile required".into());
    }
    let dialect: Dialect = profile
        .dialect
        .parse()
        .map_err(|_| "invalid schema dialect")?;
    let document = match &request {
        Request::ConfigureDelivery { document }
        | Request::RegisterRevision { document, .. }
        | Request::RecordDelivery { document, .. } => Some(document),
        _ => None,
    };
    let raw = if let Some(document) = document {
        let path = if document.path.is_absolute() {
            document.path.clone()
        } else {
            request_path
                .parent()
                .unwrap_or(Path::new("."))
                .join(&document.path)
        };
        let raw = bytes(&path)?;
        if Object::from_bytes(&raw) != document.expected {
            return Err("change document pin differs".into());
        }
        raw
    } else {
        Vec::new()
    };
    let names = match &request {
        Request::SelectRevision { streams, .. } => {
            streams.iter().map(String::as_str).collect::<Vec<_>>()
        }
        _ => vec![],
    };
    let change = match &request {
        Request::ConfigureDelivery { .. } => Change::ConfigureDelivery { policy: &raw },
        Request::RegisterRevision { coverage, .. } => Change::RegisterRevision {
            manifest: &raw,
            coverage: *coverage,
        },
        Request::RecordDelivery { coverage, .. } => Change::RecordDelivery {
            event: &raw,
            coverage: *coverage,
        },
        Request::SelectRevision {
            revision,
            representation,
            subject,
            status,
            ..
        } => Change::SelectRevision {
            revision,
            representation,
            subject,
            status,
            streams: &names,
        },
    };
    let matching = profile.publication.as_ref().is_some_and(|p| {
        profile.byte_source.as_ref().is_some_and(|b| {
            b.places.iter().any(|place| {
                std::mem::discriminant(&place.layout) == std::mem::discriminant(&p.layout)
                    && place.root == p.root
            })
        })
    });
    if action != "check" && !matching {
        return Err("publication root/layout must be an explicit reader place".into());
    }
    let mut reader = CacheMetadataReader::new(profile.byte_source)
        .map_err(|e| format!("history byte profile: {e}"))?;
    let repository = Repository::new(
        profile.git_executable,
        profile.repository,
        GitLimits::default(),
    )
    .map_err(|e| e.to_string())?;
    let base = GitId::parse(&profile.base).map_err(|e| e.to_string())?;
    let records = scope(profile.records)?;
    let record_scope = if let Some(p) = &profile.proposal {
        let path = LogicalPath::parse(&p.path).map_err(|_| "invalid proposal path")?;
        if !records.prefixes().any(|prefix| {
            path.as_str() == prefix.as_str()
                || path
                    .as_str()
                    .strip_prefix(prefix.as_str())
                    .is_some_and(|s| s.starts_with('/'))
        }) {
            return Err("proposal path outside declared record scope".into());
        }
        Scope::new([path]).map_err(|e| e.to_string())?
    } else {
        records
    };
    let snapshot = repository
        .native(
            &repository.commit(base.clone()).map_err(|e| e.to_string())?,
            record_scope,
            scope(profile.definitions)?,
            profile.limits,
        )
        .map_err(|e| e.to_string())?;
    let prepared = snapshot
        .prepare_history(
            &profile.asset,
            &profile.expected,
            dialect,
            Limits::default(),
            change,
            &mut reader,
        )
        .map_err(|e| {
            reader
                .take_failure()
                .unwrap_or_else(|| format!("history preparation: {e}"))
        })?;
    let replacement = profile
        .proposal
        .as_ref()
        .map(|p| {
            LogicalPath::parse(&p.path).map(|path| Replacement {
                path,
                expected: prepared.expected().clone(),
                bytes: prepared.record().bytes().to_vec(),
            })
        })
        .transpose()
        .map_err(|_| "invalid proposal path")?;
    if let (Some(change), Some(p)) = (&replacement, &profile.proposal) {
        repository
            .check_proposal(
                base.clone(),
                std::slice::from_ref(change),
                &p.identity,
                &p.message,
            )
            .map_err(|e| format!("proposal preflight: {e}"))?;
    }
    let mut output = json!({"schema":"tig.history-command-result.experimental-1","action":action,
        "base":base.as_str(),"source_checkpoint":snapshot.checkpoint(),"expected":prepared.expected(),
        "record":prepared.record().object(),"metadata":prepared.metadata().iter().map(|b|b.object()).collect::<Vec<_>>(),
        "noop":prepared.is_noop(),"publication_admitted":false,"published":false,"intent":null,"proposal":null,"new_metadata_readback_verified":false,
        "current_selection":false,"approval":false,"old_protection_released":false,"independent_custody_verified":false,"complete_semantic_coverage":false});
    if action != "check" && !prepared.is_noop() {
        let publication = profile
            .publication
            .as_ref()
            .ok_or("publication profile required")?;
        let mut backend = open(publication).map_err(|e| format!("publication enrollment: {e}"))?;
        let published = publication::publish(
            &prepared,
            &publication.operation,
            &publication.owner,
            publication.required,
            &mut backend,
        )
        .map_err(|e| format!("history publication: {e}"))?;
        for blob in prepared.metadata().iter().chain([prepared.record()]) {
            let raw = reader.read(blob.object(), 65_536).map_err(|e| {
                reader
                    .take_failure()
                    .unwrap_or_else(|| format!("published metadata readback: {e}"))
            })?;
            if Object::from_bytes(&raw) != *blob.object() {
                return Err("published metadata readback differs".into());
            }
        }
        output["new_metadata_readback_verified"] = json!(true);
        output["publication_admitted"] = json!(true);
        output["published"] = json!(true);
        output["intent"] = json!(published.intent());
        if action == "propose" {
            let p = profile
                .proposal
                .as_ref()
                .ok_or("proposal profile required")?;
            let changes = [published
                .replacement(LogicalPath::parse(&p.path).map_err(|_| "invalid proposal path")?)];
            let proposal = repository
                .propose(base, &changes, &p.identity, &p.message)
                .map_err(|e| format!("proposal creation: {e}"))?;
            output["proposal"] =
                json!({"commit":proposal.commit().as_str(),"reference":proposal.reference()});
        }
    }
    Ok(output)
}
