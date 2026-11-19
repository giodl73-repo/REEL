//! Explicit transport effects, never native admission or retained completion.
use reel_workflow::{Error, ErrorKind, Result};
use reel_workflow_http::Config;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
    process::ExitCode,
};
fn error(kind: ErrorKind, code: &str) -> Error {
    Error {
        kind,
        code: code.into(),
    }
}
fn read(path: &str, limit: u64) -> Result<Vec<u8>> {
    let file = File::open(path).map_err(|_| error(ErrorKind::Missing, "http-input-open"))?;
    if !file
        .metadata()
        .map_err(|_| error(ErrorKind::Invalid, "http-input-metadata"))?
        .is_file()
    {
        return Err(error(ErrorKind::Invalid, "http-input-file"));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| error(ErrorKind::Integrity, "http-input-read"))?;
    if bytes.len() as u64 > limit {
        return Err(error(ErrorKind::Budget, "http-input-bytes"));
    }
    Ok(bytes)
}
fn create(path: &str) -> Result<File> {
    let path = Path::new(path);
    if !path.is_absolute()
        || path
            .components()
            .any(|p| matches!(p, std::path::Component::ParentDir))
        || path
            .file_name()
            .and_then(|n| n.to_str())
            .is_none_or(|n| n.contains(':'))
    {
        return Err(error(ErrorKind::Invalid, "http-output-path"));
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| {
            error(
                ErrorKind::NeedsRecovery,
                "http-existing-or-unwritable-output-preserve",
            )
        })
}
fn write(file: &mut File, bytes: &[u8]) -> Result<()> {
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| error(ErrorKind::NeedsRecovery, "http-response-write-preserve"))
}
fn run(args: &[String]) -> Result<serde_json::Value> {
    let [
        command,
        config,
        operation,
        body,
        intent,
        metadata,
        output,
        apply,
    ] = args
    else {
        return Err(error(ErrorKind::Invalid, "http-command-arguments"));
    };
    if command != "post" || apply != "--apply" {
        return Err(error(ErrorKind::Invalid, "http-command-arguments"));
    }
    // A previous operation intent always requires reconciliation, never replay.
    if Path::new(intent).exists() {
        return Err(error(
            ErrorKind::NeedsRecovery,
            "http-original-intent-exists",
        ));
    }
    let config: Config =
        reel_workflow::decode(&read(config, reel_workflow::MAX_CONTROL_BYTES as u64)?)?;
    let body = read(body, config.request_byte_limit.min(128 * 1024 * 1024))?;
    let prepared = config.prepare(operation, &body, |name| std::env::var(name).ok())?;
    let mut metadata_file = create(metadata)?;
    let mut response_file = create(output)?;
    let response = prepared.send_captured(
        |record| {
            let mut file = create(intent)?;
            let raw = serde_json::to_vec(record)
                .map_err(|_| error(ErrorKind::Invalid, "http-intent-json"))?;
            write(&mut file, &raw)
        },
        |chunk| write(&mut response_file, chunk),
    )?;
    let written = read(output, config.response_byte_limit)?;
    if written.len() != response.body.len()
        || reel_workflow::digest(&written) != response.body_sha256
    {
        return Err(error(ErrorKind::NeedsRecovery, "http-response-readback"));
    }
    let record = serde_json::json!({"schema":"reel.workflow-http-response.v1","operation_id":operation,"status":response.status,"content_type":response.content_type,
        "body_sha256":response.body_sha256,"body_bytes":response.body.len(),"task_completed":false,"retained":false,"selected":false});
    write(
        &mut metadata_file,
        &serde_json::to_vec(&record)
            .map_err(|_| error(ErrorKind::Invalid, "http-response-json"))?,
    )?;
    Ok(
        serde_json::json!({"ok":true,"scope":"http-transport-only","status":response.status,"task_completed":false,"retained":false,"selected":false}),
    )
}
fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--help"] {
        println!(
            "reel-workflow-http post CONFIG OPERATION_ID JSON_BODY NEW_INTENT NEW_METADATA NEW_RESPONSE --apply"
        );
        return ExitCode::SUCCESS;
    }
    match run(&args) {
        Ok(value) => {
            println!("{value}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            println!("{}", serde_json::json!({"ok":false,"error":error}));
            ExitCode::FAILURE
        }
    }
}
