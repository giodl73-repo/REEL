//! Strict C2PA verification via the official `c2patool` executable.
//!
//! REEL does not author Content Credentials, install or bundle `c2patool`, fetch
//! trust lists, or discover the tool on `PATH`. It requires an absolute executable
//! path pinned by SHA-256, hashes both the executable and an immutable private
//! snapshot of the exact target bytes, invokes the tool directly with fixed
//! arguments and no shell, bounds the captured output, and parses the official
//! report strictly. V1 evaluates manifest integrity only: the official
//! `c2patool trust` flow and trust resources are deliberately not invoked, so
//! certificate trust is always reported as `not-evaluated`. Remote manifests,
//! OCSP, identity decoding, and all other network access are disabled by a
//! fixed private settings file.
//!
//! A valid Content Credential never infers identity, rights, publication, or
//! release.

use std::{
    collections::HashSet,
    fs::{self, File},
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::{
    Deserialize, Serialize,
    de::{self, DeserializeOwned, Deserializer, MapAccess, SeqAccess, Visitor},
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::Builder;

pub const C2PA_INPUT_SCHEMA: &str = "reel.c2pa-verification-input.v0.1";
pub const C2PA_REPORT_SCHEMA: &str = "reel.c2pa-verification-report.v0.1";

const MAX_CONTRACT_BYTES: u64 = 1024 * 1024;
const MAX_TOOL_OUTPUT_BYTES: usize = 1024 * 1024;
const MAX_VERSION_OUTPUT_BYTES: usize = 4096;
const MAX_VALIDATION_CODES: usize = 256;
const MAX_CODE_LEN: usize = 200;
const MAX_ACTIVE_MANIFEST_LABEL_LEN: usize = 200;
const MAX_MANIFEST_COUNT: usize = 256;
const MAX_TOOL_RUNTIME_SECONDS: u64 = 30;
const MAX_TOOL_RUNTIME: Duration = Duration::from_secs(MAX_TOOL_RUNTIME_SECONDS);
const VERIFIER_SETTINGS: &[u8] = br#"{"core":{"decode_identity_assertions":false,"allowed_network_hosts":[],"allow_redirects":false},"verify":{"verify_trust":false,"verify_timestamp_trust":false,"ocsp_fetch":false,"remote_manifest_fetch":false}}"#;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalFileHash {
    path: PathBuf,
    sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct C2paVerificationInput {
    schema: String,
    c2patool_path: PathBuf,
    expected_c2patool_sha256: String,
    target: LocalFileHash,
    #[serde(default)]
    expected_tool_version: Option<String>,
}

/// Path-free portable C2PA verification report. Integrity and trust are distinct
/// results; the report never flattens them into a single "approved" flag.
#[derive(Clone, Debug, Serialize)]
pub struct C2paVerificationReport {
    pub schema: String,
    pub manifest_integrity: String,
    pub certificate_trust: String,
    pub trust_evaluated: bool,
    pub validation_state: String,
    pub active_manifest_label: String,
    pub manifest_count: usize,
    pub validation_status_codes: Vec<String>,
    pub tool_version: String,
    pub c2patool_sha256: String,
    pub verifier_settings_sha256: String,
    pub target_sha256: String,
    pub report_sha256: String,
    pub grants_identity: bool,
    pub grants_rights: bool,
    pub grants_publication: bool,
    pub grants_release: bool,
    pub human_review_required: bool,
}

#[derive(Debug)]
struct ParsedReport {
    validation_state: String,
    active_manifest_label: String,
    manifest_count: usize,
    validation_status_codes: Vec<String>,
}

struct BoundedOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    truncated: bool,
    timed_out: bool,
}

struct ReaderControl {
    output_truncated: AtomicBool,
    stop_readers: AtomicBool,
    completed_readers: AtomicUsize,
}

/// Verify Content Credentials on a target asset through the official `c2patool`.
pub fn verify_c2pa(
    input_path: impl AsRef<Path>,
    output_path: Option<&Path>,
) -> Result<C2paVerificationReport> {
    let input_path = input_path.as_ref();
    let input_bytes = read_contract_bytes(input_path, "c2pa verification input")?;
    let input: C2paVerificationInput = parse_json_strict(&input_bytes, "c2pa verification input")?;
    require_schema(&input.schema, C2PA_INPUT_SCHEMA)?;
    require_hash(&input.expected_c2patool_sha256)?;
    require_hash(&input.target.sha256)?;
    if !input.c2patool_path.is_absolute() {
        bail!("c2patool path must be an absolute executable path");
    }
    if let Some(expected) = input.expected_tool_version.as_deref() {
        require_version(expected)?;
    }

    let snapshot_dir = Builder::new().prefix(".reel-c2pa-").tempdir()?;
    let executable_snapshot = snapshot_executable(&input.c2patool_path, snapshot_dir.path())?;
    let c2patool_sha256 = hash_file_streaming(&executable_snapshot)?;
    if c2patool_sha256 != input.expected_c2patool_sha256 {
        bail!("c2patool executable hash does not match the expected digest");
    }

    // Address target TOCTOU: copy the exact bytes into a private snapshot, hash
    // those bytes, and only ever invoke the tool against the snapshot. The
    // generated basename avoids leaking the source name, while the extension is
    // retained so c2patool can identify the media format.
    let target_path = resolve(input_path, &input.target.path);
    let snapshot_path = target_snapshot_path(snapshot_dir.path(), &target_path);
    fs::copy(&target_path, &snapshot_path)
        .with_context(|| format!("failed to snapshot c2pa target {}", target_path.display()))?;
    let target_sha256 = hash_file_streaming(&snapshot_path)?;
    if target_sha256 != input.target.sha256 {
        bail!("c2pa target hash does not match the pinned digest");
    }

    // Prevent c2patool from loading caller-local settings, trust resources, or
    // attacker-selected network resources such as remote manifests.
    let settings_path = snapshot_dir.path().join("c2patool-settings.json");
    fs::write(&settings_path, VERIFIER_SETTINGS)?;
    let verifier_settings_sha256 = hash_bytes(VERIFIER_SETTINGS);
    let tool_version = capture_tool_version(&executable_snapshot)?;
    if let Some(expected) = input.expected_tool_version.as_deref() {
        if tool_version != expected {
            bail!("c2patool version {tool_version} does not match the expected {expected}");
        }
    }

    let raw_report = run_report(&executable_snapshot, &snapshot_path, &settings_path)?;
    let report_sha256 = hash_bytes(&raw_report);
    let parsed = parse_report(&raw_report)?;
    drop(snapshot_dir);

    let report = C2paVerificationReport {
        schema: C2PA_REPORT_SCHEMA.to_string(),
        manifest_integrity: "valid".to_string(),
        certificate_trust: "not-evaluated".to_string(),
        trust_evaluated: false,
        validation_state: parsed.validation_state,
        active_manifest_label: parsed.active_manifest_label,
        manifest_count: parsed.manifest_count,
        validation_status_codes: parsed.validation_status_codes,
        tool_version,
        c2patool_sha256,
        verifier_settings_sha256,
        target_sha256,
        report_sha256,
        grants_identity: false,
        grants_rights: false,
        grants_publication: false,
        grants_release: false,
        human_review_required: true,
    };
    if let Some(output_path) = output_path {
        write_json_new(&report, output_path)?;
    }
    Ok(report)
}

fn snapshot_executable(executable: &Path, snapshot_dir: &Path) -> Result<PathBuf> {
    let source_metadata = fs::metadata(executable)
        .with_context(|| format!("c2patool executable not found at {}", executable.display()))?;
    if !source_metadata.is_file() {
        bail!("c2patool path is not a regular file");
    }
    let snapshot = executable_snapshot_path(snapshot_dir);
    fs::copy(executable, &snapshot)
        .with_context(|| format!("failed to snapshot c2patool {}", executable.display()))?;
    #[cfg(unix)]
    fs::set_permissions(&snapshot, source_metadata.permissions()).with_context(|| {
        format!(
            "failed to preserve c2patool permissions at {}",
            snapshot.display()
        )
    })?;
    if !fs::metadata(&snapshot)
        .with_context(|| format!("failed to inspect c2patool snapshot {}", snapshot.display()))?
        .is_file()
    {
        bail!("c2patool snapshot is not a regular file");
    }
    Ok(snapshot)
}

#[cfg(windows)]
fn executable_snapshot_path(snapshot_dir: &Path) -> PathBuf {
    snapshot_dir.join("c2patool-snapshot.exe")
}

#[cfg(not(windows))]
fn executable_snapshot_path(snapshot_dir: &Path) -> PathBuf {
    snapshot_dir.join("c2patool-snapshot")
}

fn target_snapshot_path(snapshot_dir: &Path, target: &Path) -> PathBuf {
    let mut snapshot = snapshot_dir.join("target-snapshot");
    if let Some(extension) = target.extension() {
        snapshot.set_extension(extension);
    }
    snapshot
}

fn capture_tool_version(executable: &Path) -> Result<String> {
    let output = run_bounded(executable, &["--version"], MAX_VERSION_OUTPUT_BYTES)?;
    if output.timed_out {
        bail!("c2patool --version exceeded the {MAX_TOOL_RUNTIME_SECONDS} second timeout");
    }
    if output.truncated {
        bail!("c2patool --version output exceeded the {MAX_VERSION_OUTPUT_BYTES} byte bound");
    }
    if !output.status.success() {
        bail!(
            "c2patool --version exited with a failure status {}",
            describe_status(&output.status)
        );
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let first_line = text.lines().next().unwrap_or("").trim();
    let token = first_line
        .split_whitespace()
        .find(|candidate| looks_like_version(candidate))
        .unwrap_or("");
    if token.is_empty() || token.len() > 64 {
        bail!("could not determine a strict c2patool version");
    }
    Ok(token.to_string())
}

fn run_report(executable: &Path, snapshot: &Path, settings: &Path) -> Result<Vec<u8>> {
    let snapshot = snapshot
        .to_str()
        .ok_or_else(|| anyhow!("snapshot path is not valid UTF-8"))?;
    let settings = settings
        .to_str()
        .ok_or_else(|| anyhow!("settings path is not valid UTF-8"))?;
    let output = run_bounded(
        executable,
        &[snapshot, "--settings", settings],
        MAX_TOOL_OUTPUT_BYTES,
    )?;
    if output.timed_out {
        bail!("c2patool exceeded the {MAX_TOOL_RUNTIME_SECONDS} second timeout");
    }
    if output.truncated {
        bail!("c2patool output exceeded the {MAX_TOOL_OUTPUT_BYTES} byte bound");
    }
    if !output.status.success() {
        bail!(
            "c2patool exited with a failure status {}",
            describe_status(&output.status)
        );
    }
    Ok(output.stdout)
}

fn run_bounded(executable: &Path, args: &[&str], limit: usize) -> Result<BoundedOutput> {
    let mut command = Command::new(executable);
    command.args(args);
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    command.env_clear();
    #[cfg(windows)]
    {
        for key in ["SystemRoot", "windir", "SystemDrive", "TEMP", "TMP"] {
            if let Ok(value) = std::env::var(key) {
                command.env(key, value);
            }
        }
    }
    let mut child = command
        .spawn()
        .with_context(|| format!("failed to launch c2patool {}", executable.display()))?;
    let stdout = child.stdout.take().expect("configured piped stdout");
    let stderr = child.stderr.take().expect("configured piped stderr");
    let reader_control = Arc::new(ReaderControl {
        output_truncated: AtomicBool::new(false),
        stop_readers: AtomicBool::new(false),
        completed_readers: AtomicUsize::new(0),
    });
    let stdout_reader = {
        let reader_control = Arc::clone(&reader_control);
        thread::spawn(move || {
            let result = read_bounded(stdout, limit, Arc::clone(&reader_control));
            reader_control
                .completed_readers
                .fetch_add(1, Ordering::Release);
            result
        })
    };
    let stderr_reader = {
        let reader_control = Arc::clone(&reader_control);
        thread::spawn(move || {
            let result = read_bounded(stderr, limit, Arc::clone(&reader_control));
            reader_control
                .completed_readers
                .fetch_add(1, Ordering::Release);
            result
        })
    };

    let started = Instant::now();
    let mut timed_out = false;
    let mut status = None;
    let status = loop {
        if reader_control.output_truncated.load(Ordering::Acquire) {
            reader_control.stop_readers.store(true, Ordering::Release);
            if status.is_none() {
                if let Err(error) = child.kill() {
                    if error.kind() != ErrorKind::InvalidInput {
                        return Err(error)
                            .context("failed to stop c2patool after output truncation");
                    }
                }
                status = Some(child.wait().context("failed to wait for c2patool")?);
            }
            break status.expect("a stopped c2patool has an exit status");
        }

        if status.is_none() {
            status = child.try_wait().context("failed to poll c2patool")?;
        }
        if let Some(status) = status {
            if reader_control.completed_readers.load(Ordering::Acquire) == 2 {
                break status;
            }
        }
        if started.elapsed() >= MAX_TOOL_RUNTIME {
            timed_out = true;
            reader_control.stop_readers.store(true, Ordering::Release);
            if status.is_none() {
                if let Err(error) = child.kill() {
                    if error.kind() != ErrorKind::InvalidInput {
                        return Err(error).context("failed to stop timed-out c2patool");
                    }
                }
                status = Some(child.wait().context("failed to wait for c2patool")?);
            }
            break status.expect("an exited or stopped c2patool has an exit status");
        }
        thread::sleep(Duration::from_millis(10));
    };

    // A descendant may inherit the pipes after c2patool exits. Never let such a
    // process bypass the deadline by blocking these joins indefinitely.
    if (timed_out || reader_control.output_truncated.load(Ordering::Acquire))
        && reader_control.completed_readers.load(Ordering::Acquire) != 2
    {
        drop(stdout_reader);
        drop(stderr_reader);
        return Ok(BoundedOutput {
            status,
            stdout: Vec::new(),
            truncated: reader_control.output_truncated.load(Ordering::Acquire),
            timed_out,
        });
    }

    let stdout_bytes = stdout_reader
        .join()
        .map_err(|_| anyhow!("c2patool stdout reader panicked"))??;
    let _stderr_bytes = stderr_reader
        .join()
        .map_err(|_| anyhow!("c2patool stderr reader panicked"))??;
    Ok(BoundedOutput {
        status,
        stdout: stdout_bytes,
        truncated: reader_control.output_truncated.load(Ordering::Acquire),
        timed_out,
    })
}

fn read_bounded(
    mut reader: impl Read,
    limit: usize,
    reader_control: Arc<ReaderControl>,
) -> Result<Vec<u8>> {
    let read_limit = limit
        .checked_add(1)
        .ok_or_else(|| anyhow!("c2patool output limit is too large"))?;
    let mut retained = Vec::with_capacity(limit);
    let mut chunk = [0u8; 8192];
    while retained.len() < read_limit && !reader_control.stop_readers.load(Ordering::Acquire) {
        let remaining = read_limit - retained.len();
        let read_len = remaining.min(chunk.len());
        let read = reader.read(&mut chunk[..read_len])?;
        if read == 0 {
            break;
        }
        retained.extend_from_slice(&chunk[..read]);
    }
    if retained.len() > limit {
        reader_control
            .output_truncated
            .store(true, Ordering::Release);
        retained.truncate(limit);
        while !reader_control.stop_readers.load(Ordering::Acquire) {
            thread::sleep(Duration::from_millis(1));
        }
    }
    Ok(retained)
}

fn parse_report(raw: &[u8]) -> Result<ParsedReport> {
    let value: serde_json::Value =
        serde_json::from_slice(raw).context("c2patool report is not valid JSON")?;
    ensure_no_duplicate_keys(raw).context("c2patool report contains duplicate JSON fields")?;
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("c2patool report is not a JSON object"))?;
    let active_manifest_label = match object.get("active_manifest").and_then(|v| v.as_str()) {
        Some(label) if !label.is_empty() && label.len() <= MAX_ACTIVE_MANIFEST_LABEL_LEN => {
            label.to_string()
        }
        Some(label) if label.len() > MAX_ACTIVE_MANIFEST_LABEL_LEN => {
            bail!("c2patool active manifest label exceeds the size bound")
        }
        _ => bail!("c2patool report has no active manifest"),
    };
    let manifests = object
        .get("manifests")
        .and_then(|value| value.as_object())
        .ok_or_else(|| anyhow!("c2patool report has no manifests map"))?;
    if !manifests.contains_key(&active_manifest_label) {
        bail!("c2patool active manifest is absent from the manifest store");
    }
    let manifest_count = manifests.len();
    if manifest_count > MAX_MANIFEST_COUNT {
        bail!("c2patool report has too many manifests");
    }

    let validation_state = match object.get("validation_state") {
        Some(Value::String(state)) => match state.as_str() {
            "Valid" => state.clone(),
            "Invalid" => bail!("c2patool reported validation_state Invalid"),
            "Trusted" => bail!(
                "c2patool reported validation_state Trusted as an uncontrolled trust configuration under the controlled no-trust run"
            ),
            _ => bail!("c2patool reported an unsupported validation_state"),
        },
        Some(_) => bail!("c2patool validation_state must be a string"),
        None => bail!("c2patool report is missing validation_state"),
    };
    let validation_results = object
        .get("validation_results")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("c2patool report has no validation_results object"))?;
    if let Some(ingredient_deltas) = validation_results.get("ingredientDeltas") {
        let count = match ingredient_deltas {
            Value::Array(entries) => entries.len(),
            Value::Object(entries) => entries.len(),
            Value::Null => 0,
            _ => bail!("c2patool validation_results.ingredientDeltas must be a list or map"),
        };
        if count > MAX_MANIFEST_COUNT {
            bail!("c2patool validation_results.ingredientDeltas has too many entries");
        }
    }
    let active_validation = validation_results
        .get("activeManifest")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("c2patool report has no validation_results.activeManifest"))?;
    let success_codes = collect_codes(active_validation, "success")?;
    let informational_codes = collect_codes(active_validation, "informational")?;
    let failure_codes = collect_codes(active_validation, "failure")?;
    let total_codes = success_codes
        .len()
        .checked_add(informational_codes.len())
        .and_then(|count| count.checked_add(failure_codes.len()))
        .ok_or_else(|| anyhow!("c2patool validation code count overflow"))?;
    if total_codes > MAX_VALIDATION_CODES {
        bail!("c2patool validation results have too many code entries");
    }
    if !failure_codes.is_empty() {
        bail!(
            "c2patool reported a manifest integrity validation failure: \
             validation_results.activeManifest.failure is non-empty"
        );
    }
    let validation_status_codes = success_codes
        .into_iter()
        .chain(informational_codes)
        .chain(failure_codes)
        .collect();

    Ok(ParsedReport {
        validation_state,
        active_manifest_label,
        manifest_count,
        validation_status_codes,
    })
}

fn collect_codes(
    active_manifest: &serde_json::Map<String, Value>,
    field: &str,
) -> Result<Vec<String>> {
    let entries = active_manifest
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| {
            anyhow!("c2patool validation_results.activeManifest.{field} must be an array")
        })?;
    if entries.len() > MAX_VALIDATION_CODES {
        bail!("c2patool validation_results.activeManifest.{field} has too many entries");
    }
    entries
        .iter()
        .map(|entry| {
            let code = entry
                .as_object()
                .and_then(|value| value.get("code"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    anyhow!("c2patool validation_results.activeManifest.{field} entry has no code")
                })?;
            if code.is_empty() {
                bail!("c2patool validation code must not be empty");
            }
            if code.len() > MAX_CODE_LEN {
                bail!("c2patool validation code exceeds the size bound");
            }
            Ok(code.to_string())
        })
        .collect()
}

fn looks_like_version(candidate: &str) -> bool {
    candidate.contains('.')
        && candidate
            .chars()
            .any(|character| character.is_ascii_digit())
        && candidate.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '+' | '_')
        })
}

fn require_version(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 64 {
        bail!("expected c2patool version must be 1..=64 characters");
    }
    if !value.chars().all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '+' | '_')
    }) {
        bail!("expected c2patool version contains unsupported characters");
    }
    Ok(())
}

fn describe_status(status: &ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("code {code}"),
        None => "signal".to_string(),
    }
}

fn read_contract_bytes(path: &Path, label: &str) -> Result<Vec<u8>> {
    let file =
        File::open(path).with_context(|| format!("failed to open {label} {}", path.display()))?;
    let mut reader = file.take(MAX_CONTRACT_BYTES + 1);
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read {label} {}", path.display()))?;
    if bytes.len() as u64 > MAX_CONTRACT_BYTES {
        bail!("{label} exceeds the {MAX_CONTRACT_BYTES} byte contract bound");
    }
    Ok(bytes)
}

fn parse_json_strict<T: DeserializeOwned>(bytes: &[u8], label: &str) -> Result<T> {
    ensure_no_duplicate_keys(bytes)
        .with_context(|| format!("{label} contains duplicate JSON fields"))?;
    serde_json::from_slice(bytes).with_context(|| format!("{label} is not valid strict JSON"))
}

fn resolve(contract_path: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        contract_path
            .parent()
            .filter(|value| !value.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .join(path)
    }
}

fn require_schema(actual: &str, expected: &str) -> Result<()> {
    if actual != expected {
        bail!("unsupported schema {actual}; expected {expected}");
    }
    Ok(())
}

fn require_hash(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        bail!("expected a lowercase SHA-256 digest");
    }
    Ok(())
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    finalize_hex(hasher)
}

fn hash_file_streaming(path: &Path) -> Result<String> {
    let mut file =
        File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(finalize_hex(hasher))
}

fn finalize_hex(hasher: Sha256) -> String {
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn write_json_new<T: Serialize>(value: &T, output: &Path) -> Result<()> {
    if output.exists() {
        bail!("refusing to overwrite {}", output.display());
    }
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = Builder::new()
        .prefix(".reel-c2pa-report-")
        .tempfile_in(parent)?;
    temporary.write_all(format!("{}\n", serde_json::to_string_pretty(value)?).as_bytes())?;
    temporary.flush()?;
    temporary
        .persist_noclobber(output)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to publish {}", output.display()))?;
    Ok(())
}

fn ensure_no_duplicate_keys(bytes: &[u8]) -> Result<()> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    StrictShape::deserialize(&mut deserializer).map_err(|error| anyhow!("{error}"))?;
    deserializer.end().map_err(|error| anyhow!("{error}"))?;
    Ok(())
}

struct StrictShape;

impl<'de> Deserialize<'de> for StrictShape {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(StrictShapeVisitor)
    }
}

struct StrictShapeVisitor;

impl<'de> Visitor<'de> for StrictShapeVisitor {
    type Value = StrictShape;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("strict JSON without duplicate object keys")
    }

    fn visit_bool<E>(self, _value: bool) -> Result<Self::Value, E> {
        Ok(StrictShape)
    }

    fn visit_i64<E>(self, _value: i64) -> Result<Self::Value, E> {
        Ok(StrictShape)
    }

    fn visit_u64<E>(self, _value: u64) -> Result<Self::Value, E> {
        Ok(StrictShape)
    }

    fn visit_i128<E>(self, _value: i128) -> Result<Self::Value, E> {
        Ok(StrictShape)
    }

    fn visit_u128<E>(self, _value: u128) -> Result<Self::Value, E> {
        Ok(StrictShape)
    }

    fn visit_f64<E>(self, _value: f64) -> Result<Self::Value, E> {
        Ok(StrictShape)
    }

    fn visit_str<E>(self, _value: &str) -> Result<Self::Value, E> {
        Ok(StrictShape)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(StrictShape)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(StrictShape)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_any(StrictShapeVisitor)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        while seq.next_element::<StrictShape>()?.is_some() {}
        Ok(StrictShape)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut keys = HashSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key.clone()) {
                return Err(de::Error::custom(format!("duplicate JSON field: {key}")));
            }
            map.next_value::<StrictShape>()?;
        }
        Ok(StrictShape)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_REPORT: &[u8] = br#"{"active_manifest":"urn:uuid:1","manifests":{"urn:uuid:1":{}},"validation_state":"Valid","validation_results":{"activeManifest":{"success":[{"code":"claimSignature.insideValidity"},{"code":"claimSignature.validated"},{"code":"assertion.dataHash.match"}],"informational":[{"code":"timeStamp.untrusted"}],"failure":[]},"ingredientDeltas":[]}}"#;

    #[test]
    fn parses_current_official_valid_shape() {
        let report = parse_report(VALID_REPORT).unwrap();
        assert_eq!(report.validation_state, "Valid");
        assert_eq!(report.active_manifest_label, "urn:uuid:1");
        assert_eq!(report.validation_status_codes.len(), 4);
        assert!(
            report
                .validation_status_codes
                .contains(&"timeStamp.untrusted".to_string())
        );
    }

    #[test]
    fn rejects_invalid_and_uncontrolled_trusted_states() {
        for state in ["Invalid", "Trusted"] {
            let report = format!(
                r#"{{"active_manifest":"urn:uuid:1","manifests":{{"urn:uuid:1":{{}}}},"validation_state":"{state}","validation_results":{{"activeManifest":{{"success":[],"informational":[],"failure":[]}}}}}}"#
            );
            let error = parse_report(report.as_bytes()).unwrap_err().to_string();
            assert!(error.contains("validation_state"));
        }
    }

    #[test]
    fn rejects_missing_state_results_and_legacy_status() {
        let missing_state = br#"{"active_manifest":"urn:uuid:1","manifests":{"urn:uuid:1":{}},"validation_results":{"activeManifest":{"success":[],"informational":[],"failure":[]}}}"#;
        assert!(
            parse_report(missing_state)
                .unwrap_err()
                .to_string()
                .contains("missing validation_state")
        );

        let missing_results = br#"{"active_manifest":"urn:uuid:1","manifests":{"urn:uuid:1":{}},"validation_state":"Valid"}"#;
        assert!(
            parse_report(missing_results)
                .unwrap_err()
                .to_string()
                .contains("no validation_results")
        );

        let legacy = br#"{"active_manifest":"urn:uuid:1","manifests":{"urn:uuid:1":{}},"validation_status":null}"#;
        assert!(
            parse_report(legacy)
                .unwrap_err()
                .to_string()
                .contains("missing validation_state")
        );
    }

    #[test]
    fn accepts_unknown_success_codes_but_rejects_any_failure_code() {
        let unknown = br#"{"active_manifest":"urn:uuid:1","manifests":{"urn:uuid:1":{}},"validation_state":"Valid","validation_results":{"activeManifest":{"success":[{"code":"future.validation.success"}],"informational":[],"failure":[]}}}"#;
        let report = parse_report(unknown).unwrap();
        assert_eq!(
            report.validation_status_codes,
            vec!["future.validation.success".to_string()]
        );

        let failure = br#"{"active_manifest":"urn:uuid:1","manifests":{"urn:uuid:1":{}},"validation_state":"Valid","validation_results":{"activeManifest":{"success":[],"informational":[],"failure":[{"code":"future.validation.failure"}]}}}"#;
        assert!(
            parse_report(failure)
                .unwrap_err()
                .to_string()
                .contains("integrity validation failure")
        );
    }
}
