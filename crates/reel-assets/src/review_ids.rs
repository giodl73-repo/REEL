//! Durable, project-scoped review labels; no asset selection or cache authority.
use crate::Result;
use fs2::FileExt;
use serde::{Deserialize, Deserializer, Serialize, de};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
    thread,
    time::{Duration, Instant},
};

const BASE_LIMIT: u64 = 8 * 1024 * 1024;
const JOURNAL_LIMIT: u64 = 64 * 1024 * 1024;
const BATCH_LIMIT: usize = 512;

fn unique_map<'de, D, T>(deserializer: D) -> std::result::Result<BTreeMap<String, T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Unique<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> de::Visitor<'de> for Unique<T> {
        type Value = BTreeMap<String, T>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("unique map keys")
        }
        fn visit_map<M: de::MapAccess<'de>>(
            self,
            mut access: M,
        ) -> std::result::Result<Self::Value, M::Error> {
            let mut map = BTreeMap::new();
            while let Some((key, value)) = access.next_entry::<String, T>()? {
                if map.insert(key, value).is_some() {
                    return Err(de::Error::custom("duplicate map key"));
                }
            }
            Ok(map)
        }
    }
    deserializer.deserialize_map(Unique(std::marker::PhantomData))
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub schema: String,
    #[serde(deserialize_with = "unique_map")]
    pub by_source: BTreeMap<String, String>,
    #[serde(deserialize_with = "unique_map")]
    pub next: BTreeMap<String, u64>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    sha256: String,
    bytes: u64,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub kind: String,
    pub sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub sources: Vec<Source>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Assignment {
    pub source: Source,
    pub id: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Transaction {
    sequence: u64,
    previous: String,
    assignments: Vec<Assignment>,
}

#[derive(Serialize)]
pub struct Report {
    pub schema: &'static str,
    pub checkpoint: String,
    pub transactions: u64,
    pub registry: Registry,
    pub assignments: Vec<Assignment>,
    pub new_assignments: usize,
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn kind_valid(kind: &str) -> bool {
    !kind.is_empty() && kind.len() <= 8 && kind.bytes().all(|b| b.is_ascii_uppercase())
}

fn source_key(source: &Source) -> Result<String> {
    if !kind_valid(&source.kind)
        || source.sha256.len() != 64
        || !source
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("Invalid source identity".into());
    }
    Ok(format!("{}:{}", source.kind, source.sha256))
}

fn validate(registry: &Registry) -> Result<()> {
    if !matches!(
        registry.schema.as_str(),
        "caminos.private-review-short-ids.v1" | "reel.review-short-ids.v1"
    ) || registry.next.is_empty()
        || registry.next.len() > 32
    {
        return Err("Unsupported registry".into());
    }
    for (kind, next) in &registry.next {
        if !kind_valid(kind) || *next == 0 || *next > 999_999_999_999 {
            return Err("Invalid registry counter".into());
        }
    }
    let mut labels = BTreeSet::new();
    for (key, id) in &registry.by_source {
        let (kind, digest) = key.split_once(':').ok_or("Invalid source key")?;
        source_key(&Source {
            kind: kind.into(),
            sha256: digest.into(),
        })?;
        let next = registry.next.get(kind).ok_or("Undeclared source kind")?;
        let number = id.strip_prefix(kind).ok_or("Wrong label kind")?;
        if number.len() < 4 || number.len() > 12 || !number.bytes().all(|b| b.is_ascii_digit()) {
            return Err("Invalid label".into());
        }
        let index: u64 = number.parse()?;
        if index == 0 || index >= *next || format!("{kind}{index:04}") != *id || !labels.insert(id)
        {
            return Err("Duplicate label or regressed counter".into());
        }
    }
    Ok(())
}

fn read(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > limit {
        return Err("Invalid or oversized state file".into());
    }
    let mut bytes = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err("State read limit exceeded".into());
    }
    Ok(bytes)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn lock(state: &Path) -> Result<File> {
    if !state.is_absolute() || fs::symlink_metadata(state)?.file_type().is_symlink() {
        return Err("Explicit existing state directory required".into());
    }
    let path = state.join("registry.lock");
    if !fs::symlink_metadata(&path)?.is_file()
        || fs::symlink_metadata(&path)?.file_type().is_symlink()
    {
        return Err("Invalid registry lock".into());
    }
    let file = OpenOptions::new().read(true).write(true).open(path)?;
    let start = Instant::now();
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(file),
            Err(e)
                if (e.kind() == std::io::ErrorKind::WouldBlock
                    || e.raw_os_error() == fs2::lock_contended_error().raw_os_error())
                    && start.elapsed() < Duration::from_secs(2) =>
            {
                thread::sleep(Duration::from_millis(10))
            }
            Err(_) => return Err("Registry busy or unavailable; retry unchanged request".into()),
        }
    }
}

fn recover(state: &Path) -> Result<Report> {
    let manifest: Manifest = serde_json::from_slice(&read(&state.join("manifest.json"), 4096)?)?;
    if manifest.schema != "reel.review-id-state.v1" {
        return Err("Unsupported state manifest".into());
    }
    let baseline = read(&state.join("baseline.json"), BASE_LIMIT)?;
    if baseline.len() as u64 != manifest.bytes || hash(&baseline) != manifest.sha256 {
        return Err("Baseline integrity failure".into());
    }
    let mut registry: Registry = serde_json::from_slice(&baseline)?;
    validate(&registry)?;
    let journal = read(&state.join("assignments.ndjson"), JOURNAL_LIMIT)?;
    if !journal.is_empty() && journal.last() != Some(&b'\n') {
        return Err("Incomplete journal; explicit recovery required".into());
    }
    let mut checkpoint = manifest.sha256;
    let mut transactions = 0;
    for terminated in journal.split_inclusive(|b| *b == b'\n') {
        let line = terminated.strip_suffix(b"\n").ok_or("Incomplete journal")?;
        if line.is_empty() || line.len() > 131072 {
            return Err("Invalid journal record size".into());
        }
        let transaction: Transaction = serde_json::from_slice(line)?;
        if transaction.previous != checkpoint
            || transaction.sequence != transactions + 1
            || transaction.assignments.is_empty()
            || transaction.assignments.len() > BATCH_LIMIT
        {
            return Err("Journal chain mismatch".into());
        }
        for entry in transaction.assignments {
            let key = source_key(&entry.source)?;
            let next = registry
                .next
                .get_mut(&entry.source.kind)
                .ok_or("Undeclared source kind")?;
            if entry.id != format!("{}{next:04}", entry.source.kind)
                || registry.by_source.insert(key, entry.id).is_some()
            {
                return Err("Journal assignment conflict".into());
            }
            *next = next.checked_add(1).ok_or("Counter overflow")?;
        }
        checkpoint = hash(line);
        transactions += 1;
    }
    validate(&registry)?;
    Ok(Report {
        schema: "reel.review-id-report.v1",
        checkpoint,
        transactions,
        registry,
        assignments: Vec::new(),
        new_assignments: 0,
    })
}

pub fn adopt(
    source: &Path,
    expected_sha256: &str,
    expected_bytes: u64,
    state: &Path,
) -> Result<Report> {
    let raw = read(source, BASE_LIMIT)?;
    if raw.len() as u64 != expected_bytes || hash(&raw) != expected_sha256 || !state.is_absolute() {
        return Err("Snapshot pin mismatch or relative state path".into());
    }
    let registry: Registry = serde_json::from_slice(&raw)?;
    validate(&registry)?;
    fs::create_dir(state)?;
    write_new(&state.join("registry.lock"), b"")?;
    write_new(&state.join("baseline.json"), &raw)?;
    write_new(&state.join("assignments.ndjson"), b"")?;
    let manifest = Manifest {
        schema: "reel.review-id-state.v1".into(),
        sha256: expected_sha256.into(),
        bytes: expected_bytes,
    };
    write_new(
        &state.join("manifest.json"),
        &serde_json::to_vec(&manifest)?,
    )?;
    #[cfg(unix)]
    {
        File::open(state)?.sync_all()?;
        File::open(state.parent().ok_or("Missing state parent")?)?.sync_all()?;
    }
    inspect(state)
}

pub fn inspect(state: &Path) -> Result<Report> {
    let _guard = lock(state)?;
    recover(state)
}

pub fn allocate(state: &Path, request: &Request) -> Result<Report> {
    if request.schema != "reel.review-id-request.v1"
        || request.sources.is_empty()
        || request.sources.len() > BATCH_LIMIT
    {
        return Err("Unsupported or oversized allocation request".into());
    }
    let _guard = lock(state)?;
    let mut report = recover(state)?;
    let mut fresh = Vec::new();
    for source in &request.sources {
        let key = source_key(source)?;
        let id = if let Some(id) = report.registry.by_source.get(&key) {
            id.clone()
        } else {
            let next = report
                .registry
                .next
                .get_mut(&source.kind)
                .ok_or("Undeclared source kind")?;
            if *next >= 999_999_999_999 {
                return Err("Counter exhausted".into());
            }
            let id = format!("{}{next:04}", source.kind);
            *next += 1;
            report.registry.by_source.insert(key, id.clone());
            fresh.push(Assignment {
                source: source.clone(),
                id: id.clone(),
            });
            id
        };
        report.assignments.push(Assignment {
            source: source.clone(),
            id,
        });
    }
    if !fresh.is_empty() {
        let transaction = Transaction {
            sequence: report.transactions + 1,
            previous: report.checkpoint.clone(),
            assignments: fresh,
        };
        let raw = serde_json::to_vec(&transaction)?;
        let path = state.join("assignments.ndjson");
        if fs::metadata(&path)?.len() + raw.len() as u64 + 1 > JOURNAL_LIMIT {
            return Err("Journal capacity exceeded".into());
        }
        let mut file = OpenOptions::new().append(true).open(path)?;
        file.write_all(&raw)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        let reopened = recover(state)?;
        if reopened.registry.by_source != report.registry.by_source
            || reopened.registry.next != report.registry.next
        {
            return Err("Journal readback mismatch".into());
        }
        report.checkpoint = reopened.checkpoint;
        report.transactions = reopened.transactions;
        report.new_assignments = transaction.assignments.len();
    }
    Ok(report)
}
