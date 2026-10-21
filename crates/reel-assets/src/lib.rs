use serde::Deserialize;

pub mod accounting;
pub mod admission;
pub mod artifacts;
pub mod cli;
pub mod demotion;
pub mod goldens;
pub mod jobs;
pub mod registry;
pub mod scenes;
pub mod tier_review;
pub mod tiering;
pub mod versions;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Deserialize, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub cache_roots: Vec<PathBuf>,
    #[serde(default)]
    pub archive_roots: Vec<PathBuf>,
    #[serde(default)]
    pub capacity: admission::Policy,
    #[serde(default)]
    pub coordination_root: Option<PathBuf>,
    #[serde(default)]
    pub transfer_cache: Option<PathBuf>,
}

pub fn load(path: &Path) -> Result<Value> {
    let text = fs::read_to_string(path)?;
    Ok(serde_json::from_str(text.trim_start_matches('\u{feff}'))?)
}

pub fn valid_oid(oid: &str) -> bool {
    oid.len() == 64
        && oid
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn add_asset(map: &mut HashMap<String, u64>, asset: &Value) -> Result<()> {
    let oid = asset["sha256"].as_str().ok_or("Missing SHA-256")?;
    let size = asset["bytes"].as_u64().ok_or("Missing byte count")?;
    if !valid_oid(oid) {
        return Err("Invalid registered SHA-256".into());
    }
    if let Some(previous) = map.insert(oid.to_owned(), size)
        && previous != size
    {
        return Err("Conflicting registered byte counts".into());
    }
    Ok(())
}

pub fn selected(root: &Path) -> Result<HashMap<String, u64>> {
    let canonical = root.canonicalize()?;
    let root = canonical.as_path();
    let mut map = HashMap::new();
    let catalog = load(&root.join("catalog.json"))?;
    for row in catalog["videos"].as_array().ok_or("Invalid catalog")? {
        let record = root
            .join(
                row["bindings"]
                    .as_str()
                    .or_else(|| row["record"].as_str())
                    .ok_or("Missing record or scene bindings")?,
            )
            .canonicalize()?;
        if !record.starts_with(root) {
            return Err("Record escapes repository".into());
        }
        let data = load(&record)?;
        for a in data["assets"].as_object().ok_or("Missing assets")?.values() {
            add_asset(&mut map, a)?;
        }
        let cels = record
            .parent()
            .ok_or("Missing record parent")?
            .join("cels.jsonl");
        match fs::read_to_string(cels) {
            Ok(lines) => {
                for line in lines.lines().filter(|s| !s.trim().is_empty()) {
                    add_asset(&mut map, &serde_json::from_str::<Value>(line)?)?;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    for (oid, size) in generic_objects(root)? {
        if let Some(previous) = map.insert(oid, size)
            && previous != size
        {
            return Err("Conflicting generic registered byte counts".into());
        }
    }
    Ok(map)
}

pub fn generic_objects(root: &Path) -> Result<HashMap<String, u64>> {
    generic_objects_with(root, None)
}
pub(crate) fn generic_objects_with(
    root: &Path,
    storage: Option<&Config>,
) -> Result<HashMap<String, u64>> {
    let mut map = HashMap::new();
    let mut registries = Vec::new();
    let archive = root.join("registry/object-index.json");
    if archive.exists() {
        if fs::metadata(&archive)?.len() > 4096 {
            return Err("Object index control exceeds budget".into());
        }
        let descriptor = load(&archive)?;
        let asset = &descriptor["asset"];
        let oid = asset["sha256"]
            .as_str()
            .ok_or("Missing object index hash")?;
        let size = asset["bytes"].as_u64().ok_or("Missing object index size")?;
        if descriptor["schema"] != "caminos.object-index.v1"
            || !valid_oid(oid)
            || size > 4 * 1024 * 1024
            || asset["cache_uri"] != format!("cache://sha256/{oid}")
        {
            return Err("Invalid cache-backed object index".into());
        }
        let cfg = if let Some(c) = storage {
            c.clone()
        } else {
            config(root)?
        };
        let path = active_existing(&cfg, oid, size)?
            .ok_or("Object index absent from active caches; archive not probed")?;
        // The index authorizes its own LFS pointer without a circular lookup.
        add_asset(&mut map, asset)?;
        registries.push(path);
    }
    let legacy = root.join("registry/objects.tsv");
    if legacy.exists() {
        registries.push(legacy);
    }
    let shards = root.join("registry/objects");
    if shards.is_dir() {
        for entry in fs::read_dir(shards)? {
            let path = entry?.path();
            if path.extension().is_some_and(|s| s == "tsv") {
                registries.push(path);
            }
        }
    }
    for registry in registries {
        for line in fs::read_to_string(registry)?
            .lines()
            .filter(|l| !l.is_empty())
        {
            let (oid, size) = line
                .split_once('\t')
                .ok_or("Invalid generic object registry row")?;
            let size: u64 = size.parse()?;
            if !valid_oid(oid) {
                return Err("Invalid generic registered SHA-256".into());
            }
            if let Some(previous) = map.insert(oid.to_owned(), size)
                && previous != size
            {
                return Err("Conflicting generic registered byte counts".into());
            }
        }
    }
    Ok(map)
}

pub fn config(root: &Path) -> Result<Config> {
    let path = if root.join("asset-project.json").exists() {
        accounting::relative(root, &accounting::project(root)?.storage_config)?
    } else {
        root.join(".local/storage-rust.json")
    };
    let mut config: Config = serde_json::from_value(load(&path)?)?;
    if config.cache_roots.is_empty() {
        return Err("At least one active cache root required".into());
    }
    for (archive, roots) in [(false, &config.cache_roots), (true, &config.archive_roots)] {
        for path in roots {
            let text = path.to_string_lossy().replace('/', "\\").to_uppercase();
            let text = text.strip_prefix("\\\\?\\").unwrap_or(&text);
            if !path.is_absolute()
                || text.starts_with("UNC\\")
                || text.starts_with("\\\\")
                || text.starts_with("G:")
                || (!archive && text.starts_with("M:"))
            {
                return Err("Cache roots must be absolute local paths; G is forbidden and M is archive-only".into());
            }
        }
    }
    if let Some(transfer) = std::env::var_os("CAMINOS_TRANSFER_CACHE") {
        config.transfer_cache = Some(PathBuf::from(transfer));
    }
    if let Some(transfer) = &config.transfer_cache {
        let transfer = transfer.canonicalize()?;
        if !config
            .cache_roots
            .iter()
            .any(|r| r.canonicalize().ok().as_ref() == Some(&transfer))
        {
            return Err("Transfer destination must be a configured active cache".into());
        }
    }
    config.capacity.validate()?;
    // Archive volumes are not probed here.
    Ok(config)
}

pub fn object(root: &Path, oid: &str) -> PathBuf {
    root.join("objects/sha256").join(&oid[..2]).join(oid)
}

pub fn verify(path: &Path, oid: &str, size: u64) -> Result<()> {
    if !valid_oid(oid) {
        return Err("Invalid SHA-256".into());
    }
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err("Object path cannot be a symbolic link".into());
    }
    let file = File::open(path)?;
    if file.metadata()?.len() != size {
        return Err("Selected cache object has wrong size".into());
    }
    let mut hasher = Sha256::new();
    let mut file = file.take(size.saturating_add(1));
    let mut buffer = [0u8; 128 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    if format!("{:x}", hasher.finalize()) != oid {
        return Err("Selected cache object SHA-256 mismatch".into());
    }
    Ok(())
}

fn unique(parent: &Path, oid: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    parent.join(format!("{oid}-{}-{nanos}", std::process::id()))
}

struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub fn recover(root: &Path, source: &Path, cache: &Path, oid: &str, size: u64) -> Result<PathBuf> {
    if !valid_oid(oid) {
        return Err("Invalid SHA-256".into());
    }
    let destination = object(cache, oid);
    let parent = destination.parent().ok_or("Invalid cache path")?;
    fs::create_dir_all(parent)?;
    let temporary = Temporary(unique(parent, oid));
    {
        let mut input = File::open(source)?.take(size.saturating_add(1));
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary.0)?;
        io::copy(&mut input, &mut output)?;
        output.sync_all()?;
    }
    verify(&temporary.0, oid, size)?;
    // Atomic publication without overwriting another producer's object.
    match fs::hard_link(&temporary.0, &destination) {
        Ok(()) => (),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => verify(&destination, oid, size)?,
        Err(e) => return Err(e.into()),
    }
    let cfg = config(root)?;
    let control = admission::control(root, &cfg)?;
    let _lock = admission::lock(&control)?;
    let mut receipt = OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join(".local/cache-promotions.jsonl"))?;
    let mut line = serde_json::to_vec(
        &json!({"sha256": oid, "bytes": size, "source": source, "destination": destination, "sha256_verified": true, "creative_authority": "not-granted-by-this-receipt"}),
    )?;
    line.push(b'\n');
    receipt.write_all(&line)?;
    receipt.sync_all()?;
    Ok(destination)
}

fn recover_admitted(
    root: &Path,
    config: &Config,
    source: &Path,
    destination: &Path,
    oid: &str,
    size: u64,
) -> Result<PathBuf> {
    if object(destination, oid).exists() {
        verify(&object(destination, oid), oid, size)?;
        return Ok(object(destination, oid));
    }
    let _reservation = admission::reserve_on(root, config, destination, size)?;
    recover(root, source, destination, oid, size)
}

/// Verified reuse over active tiers only. Absence falls through; unreadable,
/// linked or corrupt objects fail rather than being hidden by another tier.
pub fn active_existing(config: &Config, oid: &str, size: u64) -> Result<Option<PathBuf>> {
    if !valid_oid(oid) || size > config.capacity.max_object_bytes {
        return Err("Invalid or oversized active-object lookup".into());
    }
    for cache in &config.cache_roots {
        let path = object(cache, oid);
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if !metadata.is_file() || metadata.file_type().is_symlink() {
                    return Err("Active cache object must be an immutable regular file".into());
                }
                verify(&path, oid, size)?;
                return Ok(Some(path));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(None)
}

pub fn ingest(root: &Path, source: &Path) -> Result<Value> {
    let name = source
        .file_name()
        .ok_or("Missing input filename")?
        .to_string_lossy()
        .to_lowercase();
    if name == ".env"
        || name == "auth.json"
        || name.contains("credential")
        || name.contains("secret")
        || name == "token"
    {
        return Err("Credential or secret inputs are not asset payloads".into());
    }
    let config = config(root)?;
    let mut file = File::open(source)?;
    let size = file.metadata()?.len();
    if size > config.capacity.max_object_bytes {
        return Err("Object exceeds configured operation budget".into());
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 128 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    let oid = format!("{:x}", hasher.finalize());
    if let Some(path) = active_existing(&config, &oid, size)? {
        return Ok(
            json!({"sha256":oid,"bytes":size,"cache_uri":format!("cache://sha256/{oid}"),"path":path,"deduplicated":true}),
        );
    }
    let reservation = admission::reserve(root, &config, size)?;
    let path = recover(root, source, &reservation.cache_root, &oid, size)?;
    Ok(
        json!({"sha256":oid,"bytes":size,"cache_uri":format!("cache://sha256/{oid}"),"path":path,"deduplicated":false,"admission_id":reservation.id,"volume_id":reservation.volume_id}),
    )
}

pub fn resolve(root: &Path, config: &Config, oid: &str, size: u64) -> Result<PathBuf> {
    let destination = config
        .transfer_cache
        .as_ref()
        .unwrap_or(&config.cache_roots[0]);
    for cache in &config.cache_roots {
        let path = object(cache, oid);
        match fs::metadata(&path) {
            Ok(_) => {
                verify(&path, oid, size)?;
                return if cache == destination {
                    Ok(path)
                } else {
                    recover_admitted(root, config, &path, destination, oid, size)
                };
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    for archive in &config.archive_roots {
        let path = object(archive, oid);
        match fs::metadata(&path) {
            Ok(_) => {
                verify(&path, oid, size)?;
                return recover_admitted(root, config, &path, destination, oid, size);
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Err("Selected cache object missing; no drive search or network fallback".into())
}

pub fn alias(source: &Path, oid: &str) -> Result<PathBuf> {
    let cache = source
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or("Invalid object path")?;
    let staging = cache.join("staging/caminos-lfs-rust");
    fs::create_dir_all(&staging)?;
    let alias = unique(&staging, oid);
    fs::hard_link(source, &alias)?;
    // LFS consumes this disposable link; the original is never handed to LFS.
    Ok(alias)
}

pub fn protocol(root: &Path, input: impl BufRead, mut output: impl Write) -> Result<()> {
    let mut state = None;
    for line in input.lines() {
        let request: Value = match serde_json::from_str(&line?) {
            Ok(value) => value,
            Err(e) => {
                writeln!(
                    output,
                    "{}",
                    json!({"error":{"code":2,"message":format!("Invalid JSON: {e}")}})
                )?;
                output.flush()?;
                return Ok(());
            }
        };
        let reply = match request["event"].as_str() {
            Some("init") => {
                if request["operation"] != "download" {
                    writeln!(
                        output,
                        "{}",
                        json!({"error":{"code":1,"message":"Uploads disabled. Media stays in the local cache."}})
                    )?;
                    output.flush()?;
                    return Ok(());
                }
                match config(root).and_then(|c| Ok((c, selected(root)?))) {
                    Ok(s) => {
                        state = Some(s);
                        json!({})
                    }
                    Err(e) => {
                        writeln!(
                            output,
                            "{}",
                            json!({"error":{"code":2,"message":e.to_string()}})
                        )?;
                        output.flush()?;
                        return Ok(());
                    }
                }
            }
            Some("terminate") => return Ok(()),
            Some("download") => {
                let oid = request["oid"].as_str().unwrap_or("");
                let result = (|| -> Result<PathBuf> {
                    let (config, selected) =
                        state.as_ref().ok_or("Download requires initialization")?;
                    if !valid_oid(oid) {
                        return Err("Invalid SHA-256".into());
                    }
                    let size = request["size"].as_u64().ok_or("Invalid byte count")?;
                    if selected.get(oid) != Some(&size) {
                        return Err(
                            "Object is not a registered selected asset at the requested size"
                                .into(),
                        );
                    }
                    alias(&resolve(root, config, oid, size)?, oid)
                })();
                match result {
                    Ok(path) => json!({"event":"complete","oid":oid,"path":path}),
                    Err(e) => {
                        json!({"event":"complete","oid":oid,"error":{"code":2,"message":e.to_string()}})
                    }
                }
            }
            _ => {
                json!({"event":"complete","oid":request["oid"],"error":{"code":1,"message":"Only local cache downloads are supported"}})
            }
        };
        writeln!(output, "{reply}")?;
        output.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
