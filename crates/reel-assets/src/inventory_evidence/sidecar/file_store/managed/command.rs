//! Explicit profile-driven operator boundary. No default profile, path guessing,
//! cache census, daemon, automatic retirement or fallback recovery.
use super::*;
use crate::Config;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceProfile {
    schema: String,
    project_id: String,
    authority_namespace: String,
    adapter_version: u32,
    encoding: String,
    encoded: Descriptor,
    decoded: Descriptor,
    cache: PathBuf,
}
impl SourceProfile {
    fn admitted(&self) -> Result<Source> {
        if self.schema != SOURCE_SCHEMA
            || self.adapter_version != ADAPTER_VERSION
            || !valid_oid(&self.encoded.sha256)
            || !valid_oid(&self.decoded.sha256)
            || self.project_id.trim().is_empty()
            || self.project_id.len() > 256
            || self.authority_namespace.trim().is_empty()
            || self.authority_namespace.len() > 256
        {
            return Err("Unsupported or invalid inventory profile source".into());
        }
        let encoding = match self.encoding.as_str() {
            "identity" => Encoding::Identity,
            "gzip" => Encoding::Gzip,
            _ => return Err("Unsupported explicit source encoding".into()),
        };
        Ok(Source {
            encoded: self.encoded.blob(),
            decoded: self.decoded.blob(),
            encoding,
            schema: self.schema.clone(),
            project_id: self.project_id.clone(),
            authority_namespace: self.authority_namespace.clone(),
            adapter_version: self.adapter_version,
        })
    }
}
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct BudgetProfile {
    encoded_bytes: u64,
    decoded_bytes: u64,
    objects: usize,
    copies: usize,
    node_bytes: u64,
    index_bytes: u64,
    index_nodes: usize,
    leaf_rows: usize,
    shadow_subjects: usize,
    shadow_bytes: u64,
    shadow_nodes: usize,
}
impl Default for BudgetProfile {
    fn default() -> Self {
        let b = Budgets::default();
        let s = ShadowLimits::default();
        Self {
            encoded_bytes: b.preparation.encoded_bytes,
            decoded_bytes: b.preparation.decoded_bytes,
            objects: b.preparation.objects,
            copies: b.preparation.total_copies,
            node_bytes: b.index.node_bytes,
            index_bytes: b.index.total_bytes,
            index_nodes: b.index.nodes,
            leaf_rows: b.index.leaf_rows,
            shadow_subjects: s.subjects,
            shadow_bytes: s.total_bytes,
            shadow_nodes: s.nodes,
        }
    }
}
impl BudgetProfile {
    fn budgets(&self) -> Budgets {
        Budgets {
            preparation: evidence::Limits {
                encoded_bytes: self.encoded_bytes,
                decoded_bytes: self.decoded_bytes,
                objects: self.objects,
                total_copies: self.copies,
            },
            index: BuildLimits {
                node_bytes: self.node_bytes,
                total_bytes: self.index_bytes,
                nodes: self.index_nodes,
                leaf_rows: self.leaf_rows,
            },
        }
    }
    fn shadow(&self) -> ShadowLimits {
        ShadowLimits {
            subjects: self.shadow_subjects,
            node_bytes: self.node_bytes,
            total_bytes: self.shadow_bytes,
            nodes: self.shadow_nodes,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    storage_config: PathBuf,
    cache: PathBuf,
    staging_parent: PathBuf,
    retained_parent: PathBuf,
    catalog_directory: PathBuf,
    catalog_id: String,
    key: String,
    expected_revision: u64,
    session_id: String,
    source: SourceProfile,
    #[serde(default)]
    budgets: BudgetProfile,
    #[serde(default)]
    staging_overhead_bytes: Option<u64>,
}
fn named(root: &Path, path: &Path) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        root.join(path)
    };
    checked_directory(
        path.parent()
            .ok_or("Missing explicit metadata input parent")?,
    )?;
    plain(&path, false)?;
    Ok(path)
}
fn json_input<T: serde::de::DeserializeOwned>(path: &Path, max: u64) -> Result<T> {
    plain(path, false)?;
    Ok(serde_json::from_slice(&read_limited(
        File::open(path)?,
        max,
    )?)?)
}
struct Loaded {
    profile: Profile,
    config: Config,
    source: Source,
    scope: catalog::Scope,
    overhead: u64,
}
impl Loaded {
    fn read(root: &Path, path: &Path) -> Result<Self> {
        let mut profile: Profile = json_input(&named(root, path)?, CONTROL_BYTES)?;
        if profile.schema != "reel.inventory-index-job.v1" {
            return Err("Unsupported inventory index job schema".into());
        }
        let source = profile.source.admitted()?;
        let mut config: Config = json_input(&named(root, &profile.storage_config)?, CONTROL_BYTES)?;
        config.capacity.validate()?;
        if !profile.source.cache.is_absolute()
            || !config.cache_roots.contains(&profile.cache)
            || !config
                .cache_roots
                .iter()
                .chain(&config.archive_roots)
                .any(|p| p == &profile.source.cache)
        {
            return Err("Profile cache/source locations must be explicit configured roots".into());
        }
        // Normalize ONLY the named output root; never probe unrelated active or
        // archive roots. Preserve its per-tier floor when rewriting the local copy.
        let declared = profile.cache.clone();
        let canonical = checked_directory(&declared)?;
        for cache in &mut config.cache_roots {
            if cache == &declared {
                *cache = canonical.clone();
            }
        }
        if let Some(floor) = config.capacity.tier_minimum_free_bytes.remove(&declared) {
            let previous = config
                .capacity
                .tier_minimum_free_bytes
                .entry(canonical.clone())
                .or_default();
            *previous = (*previous).max(floor);
        }
        profile.cache = canonical;
        let filesystem = crate::admission::volume_id(&profile.cache)?;
        for path in [
            &mut profile.staging_parent,
            &mut profile.retained_parent,
            &mut profile.catalog_directory,
        ] {
            *path = checked_directory(path)?;
            if !path.starts_with(&profile.cache)
                || **path == profile.cache
                || crate::admission::volume_id(path)? != filesystem
            {
                return Err("Index metadata directories must be existing children on the selected cache filesystem".into());
            }
        }
        let dirs = [
            &profile.staging_parent,
            &profile.retained_parent,
            &profile.catalog_directory,
        ];
        for (i, first) in dirs.iter().enumerate() {
            for second in dirs.iter().skip(i + 1) {
                if first.starts_with(second) || second.starts_with(first) {
                    return Err(
                        "Index staging/retention/catalog directories must be separate".into(),
                    );
                }
            }
        }
        profile.budgets.budgets().validate()?;
        let overhead = profile.staging_overhead_bytes.unwrap_or(
            (profile.budgets.index_nodes as u64)
                .checked_mul(8192)
                .ok_or("Index overhead overflow")?,
        );
        FileStore::capacity_budget(profile.budgets.budgets().store(), overhead)?;
        let scope = catalog::Scope {
            project_id: source.project_id.clone(),
            authority_namespace: source.authority_namespace.clone(),
            catalog_id: profile.catalog_id.clone(),
        };
        session_path(&profile.staging_parent, &profile.session_id)?;
        Ok(Self {
            profile,
            config,
            source,
            scope,
            overhead,
        })
    }
    fn request<'a>(&'a self, catalog: &'a Catalog) -> Request<'a> {
        let p = &self.profile;
        Request {
            catalog,
            cache: &p.cache,
            source_cache: &p.source.cache,
            staging_parent: &p.staging_parent,
            retained_parent: &p.retained_parent,
            key: &p.key,
            session_id: &p.session_id,
            expected_revision: p.expected_revision,
            source: &self.source,
            budgets: p.budgets.budgets(),
            staging_overhead_bytes: self.overhead,
        }
    }
}
/// Count-only JSON success output; private profiles, witnesses and root hashes
/// are never serialized here. Existing binary dispatch invokes this only by name.
pub fn run(root: &Path, operation: &str, profile: &Path, subjects: Option<&Path>) -> Result<Value> {
    let loaded = Loaded::read(root, profile)?;
    let p = &loaded.profile;
    let context = MutationContext {
        project_root: root,
        storage: &loaded.config,
        overhead_bytes: 64 * 1024,
    };
    if operation == "inventory-index-init" {
        let existing = match fs::symlink_metadata(p.catalog_directory.join("catalog.json")) {
            Ok(_) => true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(e.into()),
        };
        let catalog = if existing {
            Catalog::open(&p.cache, &p.catalog_directory, &loaded.scope)?
        } else {
            Catalog::initialize(&context, &p.cache, &p.catalog_directory, &loaded.scope)?
        };
        let selected = catalog.create_key(&context, &p.key)?;
        return Ok(
            json!({"schema":"reel.inventory-index-init-summary.v1","catalog_revision":selected.revision,
            "key_initialized":true,"media_payloads_copied":0,"removal_authorized":false}),
        );
    }
    let catalog = Catalog::open(&p.cache, &p.catalog_directory, &loaded.scope)?;
    match operation {
        "inventory-index-build" => Ok(serde_json::to_value(
            build(&context, &loaded.request(&catalog))?.summary(),
        )?),
        "inventory-index-shadow" => {
            let path =
                subjects.ok_or("Shadow command requires an explicit exact-subject JSON array")?;
            let subjects: Vec<String> = json_input(&named(root, path)?, 4 * 1024 * 1024)?;
            Ok(serde_json::to_value(shadow(
                &loaded.request(&catalog),
                &subjects,
                p.budgets.shadow(),
            )?)?)
        }
        "inventory-index-recover" => {
            let selected = catalog.recover(&context, &p.key)?;
            Ok(
                json!({"schema":"reel.inventory-index-recovery-summary.v1","catalog_revision":selected.revision,
                "index_selected":selected.root().is_some(),"media_payloads_copied":0,"removal_authorized":false}),
            )
        }
        _ => Err("Unsupported explicit inventory index operation".into()),
    }
}
