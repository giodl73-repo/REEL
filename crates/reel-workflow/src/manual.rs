//! Private manual exchange. Byte checks are observations, not retained custody,
//! semantic validation, authenticated execution, selection or task completion.
use crate::*;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub type FileBindings = BTreeMap<String, PathBuf>;

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Contract {
    schema: String,
    task_id: String,
    task_key: String,
    attempt_id: String,
    packet_sha256: String,
    outputs: OutputContracts,
}
#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Candidate {
    schema: String,
    task_key: String,
    attempt_id: String,
    packet_sha256: String,
    result_sha256: String,
    bindings_sha256: String,
    native_retention_verified: bool,
    task_completed: bool,
    human_approval: bool,
}
fn io_error(e: std::io::Error) -> Error {
    Error {
        kind: match e.kind() {
            std::io::ErrorKind::NotFound => ErrorKind::Missing,
            std::io::ErrorKind::PermissionDenied => ErrorKind::Denied,
            std::io::ErrorKind::WouldBlock => ErrorKind::Busy,
            _ => ErrorKind::NeedsRecovery,
        },
        code: "manual-file-operation".into(),
    }
}
fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    let raw = serde_json::to_vec(value).map_err(|_| Error {
        kind: ErrorKind::Invalid,
        code: "manual-control-encoding".into(),
    })?;
    if raw.len() > MAX_CONTROL_BYTES {
        return fail(ErrorKind::Budget, "manual-control-bytes");
    }
    Ok(raw)
}
pub fn read_control<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    decode(&read_bytes(path)?)
}
pub(crate) fn read_bytes(path: &Path) -> Result<Vec<u8>> {
    let mut raw = Vec::new();
    File::open(path)
        .map_err(io_error)?
        .take(MAX_CONTROL_BYTES as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(io_error)?;
    if raw.len() > MAX_CONTROL_BYTES {
        return fail(ErrorKind::Budget, "manual-control-bytes");
    }
    Ok(raw)
}
pub(crate) fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(io_error)?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(io_error)
}
pub fn export_packet(
    plan: &Plan,
    task: &str,
    dependencies: BTreeMap<String, DependencyResult>,
    output: &Path,
) -> Result<()> {
    write_new(output, &encode(&make_packet(plan, task, dependencies)?)?)
}
pub fn verify_outputs(
    packet: &Packet,
    result: &ReturnedResult,
    bindings: &FileBindings,
) -> Result<()> {
    check_result(packet, result)?;
    output_files(packet, result, bindings).map(|_| ())
}
fn contract(packet: &Packet, attempt: &str, packet_raw: &[u8]) -> Result<Contract> {
    check_packet(packet)?;
    if !token(attempt) {
        return fail(ErrorKind::Invalid, "manual-attempt");
    }
    if packet.task.tool.is_some() {
        return fail(ErrorKind::Unsupported, "manual-intelligence-task-required");
    }
    Ok(Contract {
        schema: "reel.workflow-manual-contract.v1".into(),
        task_id: packet.task.id.clone(),
        task_key: packet.task_key.clone(),
        attempt_id: attempt.into(),
        packet_sha256: digest(packet_raw),
        outputs: packet.task.outputs.clone(),
    })
}
/// Export only authored prompt and bounded metadata. No input payloads are
/// acquired, providers called or native resources admitted by this operation.
pub fn export_work(packet: &Packet, attempt: &str, directory: &Path) -> Result<()> {
    let packet_raw = encode(packet)?;
    let contract = contract(packet, attempt, &packet_raw)?;
    let contract_raw = encode(&contract)?;
    fs::create_dir(directory).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            Error {
                kind: ErrorKind::Conflict,
                code: "manual-export-destination-exists".into(),
            }
        } else {
            io_error(e)
        }
    })?;
    write_new(&directory.join("packet.json"), &packet_raw)?;
    write_new(&directory.join("prompt.txt"), packet.task.prompt.as_bytes())?;
    write_new(&directory.join("response-contract.json"), &contract_raw)
}
fn read_work(directory: &Path, attempt: &str) -> Result<(Packet, Contract)> {
    let raw = read_bytes(&directory.join("packet.json"))?;
    let packet: Packet = decode(&raw)?;
    let expected = contract(&packet, attempt, &raw)?;
    let supplied: Contract = read_control(&directory.join("response-contract.json"))?;
    if supplied != expected
        || read_bytes(&directory.join("prompt.txt"))? != packet.task.prompt.as_bytes()
    {
        return fail(ErrorKind::Integrity, "manual-work-package-changed");
    }
    Ok((packet, expected))
}
/// Check exact returned bytes, holding ordinary file handles through receipt
/// writing. No portable path is interpreted as native record/annex provenance.
fn output_files(
    packet: &Packet,
    result: &ReturnedResult,
    bindings: &FileBindings,
) -> Result<Vec<File>> {
    if bindings.keys().ne(result.artifacts.keys()) {
        return fail(ErrorKind::Conflict, "manual-output-file-set");
    }
    let mut guards = Vec::new();
    for (name, path) in bindings {
        if !path.is_absolute() {
            return fail(ErrorKind::Invalid, "manual-output-absolute-path-required");
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1);
        }
        let mut file = options.open(path).map_err(io_error)?;
        let metadata = file.metadata().map_err(io_error)?;
        if !metadata.is_file() || metadata.len() != result.artifacts[name].bytes {
            return fail(ErrorKind::Integrity, "manual-output-file-length");
        }
        check_output_reader(
            packet,
            result,
            name,
            &mut file,
            packet.task.outputs[name].max_bytes,
        )?;
        guards.push(file);
    }
    Ok(guards)
}
fn candidate_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    #[cfg(windows)]
    let linked = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let linked = metadata.file_type().is_symlink();
    if !metadata.is_dir() || linked {
        return fail(ErrorKind::Denied, "manual-candidate-directory");
    }
    let names: BTreeSet<_> = fs::read_dir(path)
        .map_err(io_error)?
        .map(|entry| entry.map(|e| e.file_name()).map_err(io_error))
        .collect::<Result<_>>()?;
    if names
        != ["packet.json", "result.json", "files.json", "candidate.json"]
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect()
    {
        return fail(
            ErrorKind::NeedsRecovery,
            "manual-partial-or-foreign-candidate-preserved",
        );
    }
    Ok(())
}
/// Accept a byte-checked, unretained candidate into a new control directory.
/// Returns true only for an exact replay. Partial/foreign state is preserved.
/// Payload files stay at the caller's explicit locations and must be reverified
/// on later use; this operation neither completes a task nor creates a cache.
pub fn accept_work(
    directory: &Path,
    attempt: &str,
    result: &ReturnedResult,
    bindings: &FileBindings,
    candidate: &Path,
) -> Result<bool> {
    let (packet, contract) = read_work(directory, attempt)?;
    check_result(&packet, result)?;
    if result.attempt_id != attempt || result.provenance.mode != Mode::Handoff {
        return fail(ErrorKind::Conflict, "manual-result-attempt-or-mode");
    }
    let _files = output_files(&packet, result, bindings)?;
    save_candidate(&packet, &contract, result, bindings, candidate)
}

/// Capture exact local output bytes into the result envelope, so manual callers
/// need not manufacture hashes. This records no provider or execution identity.
pub fn capture_work(
    directory: &Path,
    attempt: &str,
    bindings: &FileBindings,
    candidate: &Path,
) -> Result<bool> {
    let (packet, contract) = read_work(directory, attempt)?;
    let mut artifacts = ArtifactSet::new();
    let mut guards = Vec::new();
    for (name, path) in bindings {
        let output = packet.task.outputs.get(name).ok_or_else(|| Error {
            kind: ErrorKind::Invalid,
            code: "undeclared-output".into(),
        })?;
        if !path.is_absolute() {
            return fail(ErrorKind::Invalid, "manual-output-absolute-path-required");
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1);
        }
        let mut file = options.open(path).map_err(io_error)?;
        let metadata = file.metadata().map_err(io_error)?;
        if !metadata.is_file() || metadata.len() > output.max_bytes {
            return fail(ErrorKind::Budget, "manual-output-capture-bound");
        }
        let mut hash = Sha256::new();
        let mut length = 0u64;
        let mut buffer = [0u8; 65536];
        loop {
            let count = match file.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(io_error(e)),
            };
            length = length
                .checked_add(count as u64)
                .filter(|n| *n <= output.max_bytes)
                .ok_or_else(|| Error {
                    kind: ErrorKind::Budget,
                    code: "manual-output-capture-bound".into(),
                })?;
            hash.update(&buffer[..count]);
        }
        if length != metadata.len() {
            return fail(ErrorKind::Integrity, "manual-output-changed-during-capture");
        }
        artifacts.insert(
            name.clone(),
            Artifact {
                logical_id: format!("workflow/{}/{name}", packet.task_key),
                sha256: hash.finalize().iter().map(|b| format!("{b:02x}")).collect(),
                bytes: length,
                media_type: output.media_type.clone(),
            },
        );
        guards.push(file);
    }
    let result = ReturnedResult {
        schema: RESULT_SCHEMA.into(),
        task_id: packet.task.id.clone(),
        task_key: packet.task_key.clone(),
        attempt_id: attempt.into(),
        artifacts,
        worker_receipt: None,
        provenance: Provenance {
            mode: Mode::Handoff,
            provider: None,
            model: None,
            executable: None,
        },
    };
    check_result(&packet, &result)?;
    save_candidate(&packet, &contract, &result, bindings, candidate)
}
fn save_candidate(
    packet: &Packet,
    contract: &Contract,
    result: &ReturnedResult,
    bindings: &FileBindings,
    candidate: &Path,
) -> Result<bool> {
    let record = Candidate {
        schema: "reel.workflow-byte-checked-candidate.v1".into(),
        task_key: packet.task_key.clone(),
        attempt_id: result.attempt_id.clone(),
        packet_sha256: contract.packet_sha256.clone(),
        result_sha256: canonical_digest(result)?,
        bindings_sha256: canonical_digest(bindings)?,
        native_retention_verified: false,
        task_completed: false,
        human_approval: false,
    };
    match fs::create_dir(candidate) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            candidate_directory(candidate)?;
            let prior: ReturnedResult = read_control(&candidate.join("result.json"))?;
            check_replay(&prior, result)?;
            if read_control::<Candidate>(&candidate.join("candidate.json"))? != record
                || read_control::<Packet>(&candidate.join("packet.json"))? != *packet
                || read_control::<FileBindings>(&candidate.join("files.json"))? != *bindings
            {
                return fail(ErrorKind::Conflict, "manual-candidate-replay-conflict");
            }
            return Ok(true);
        }
        Err(e) => return Err(io_error(e)),
    }
    write_new(&candidate.join("packet.json"), &encode(&packet)?)?;
    write_new(&candidate.join("result.json"), &encode(result)?)?;
    write_new(&candidate.join("files.json"), &encode(bindings)?)?;
    write_new(&candidate.join("candidate.json"), &encode(&record)?)?;
    Ok(false)
}
