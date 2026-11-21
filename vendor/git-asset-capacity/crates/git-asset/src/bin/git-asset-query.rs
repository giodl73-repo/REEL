//! Explicit machine-readable agent queries. No discovery, daemon or cache mutation.
use git_asset::catalogue::{
    Config,
    query::{self, Request},
};
use git_asset_core::Digest;
use serde_json::{Value, json};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};
const LIMIT: usize = 16 * 1024 * 1024;
struct Bounded(Vec<u8>);
impl Write for Bounded {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        if b.len() > LIMIT.saturating_sub(self.0.len()) {
            return Err(std::io::Error::other("report budget"));
        }
        self.0.extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn publish(raw: &[u8], output: &Path) -> Result<(), &'static str> {
    if !output.is_absolute() || output.symlink_metadata().is_ok() {
        return Err("output_refused");
    }
    let parent = output
        .parent()
        .filter(|p| p.is_dir())
        .ok_or("output_refused")?;
    let mut nonce = [0; 32];
    getrandom::fill(&mut nonce).map_err(|_| "output_refused")?;
    let temporary = parent.join(format!(
        ".git-asset-query-{}.tmp",
        Digest::blob(&nonce).as_str()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| "output_refused")?;
    let owned = Temporary(temporary);
    file.write_all(raw)
        .and_then(|_| file.sync_all())
        .map_err(|_| "output_refused")?;
    drop(file);
    std::fs::hard_link(&owned.0, output).map_err(|_| "output_refused")?;
    drop(owned);
    Ok(())
}
fn run(args: &[String]) -> Result<(Value, u8), &'static str> {
    if args == ["describe"] {
        return Ok((
            json!({"schema":"tig.query-capabilities.experimental-1","read_only":true,
            "commands":["git-asset-query describe","git-asset-query run ABSOLUTE_REQUEST [--out ABSOLUTE_NEW_REPORT]"],
            "request_schema":serde_json::from_str::<Value>(include_str!("../../../../specs/AGENT-QUERY.schema.json")).map_err(|_|"internal_error")?,
            "response_schema":serde_json::from_slice::<Value>(query::RESULT_SCHEMA).map_err(|_|"internal_error")?,"statuses":["complete","partial","error"],
            "exit_codes":{"0":"complete within declared scope","3":"partial declared coverage; inspect gaps","2":"refused or invalid request; no successful report"},
            "limits":{"request_bytes":65536,"report_bytes":LIMIT,"page_rows":100,"impact_nodes":10000,"impact_edges":40000,"impact_depth":64,
            "manifest_roots":256,"manifest_documents":256,"manifest_document_bytes":4194304,"manifest_total_bytes":16777216,"manifest_references":10000,"manifest_depth":8},
            "manifest_reads":"Explicit admitted metadata objects only; source basis required, content token required for offset pages; physical opens unknown, media leaves never opened",
            "complete_project_scope":false,"payloads_opened":0,"authorizes_eviction":false}),
            0,
        ));
    }
    if !(args.len() == 2 || args.len() == 4)
        || args[0] != "run"
        || (args.len() == 4 && args[2] != "--out")
    {
        return Err("invalid_arguments");
    }
    let path = Path::new(&args[1]);
    if !path.is_absolute() {
        return Err("invalid_request_file");
    }
    let mut raw = Vec::new();
    File::open(path)
        .map_err(|_| "invalid_request_file")?
        .take(65537)
        .read_to_end(&mut raw)
        .map_err(|_| "invalid_request_file")?;
    if raw.len() > 65536 {
        return Err("invalid_request");
    }
    let request = Request::parse(&raw).map_err(|_| "invalid_request")?;
    let config = Config::load(&request.enrollment).map_err(|_| "invalid_enrollment")?;
    let report = query::execute(config, &request).map_err(|e| match e.as_str() {
        "invalid_request" => "invalid_request",
        "stale_snapshot" => "stale_snapshot",
        "stale_basis" => "stale_basis",
        "unknown_project" => "unknown_project",
        "unknown_asset" => "unknown_asset",
        "source_unavailable" => "source_unavailable",
        "conflicting_identity" => "conflicting_identity",
        "invalid_proposal" => "invalid_proposal",
        "unknown_collection" => "unknown_collection",
        "unknown_work" => "unknown_work",
        "manifest_scope_refused" => "manifest_scope_refused",
        "manifest_budget_exceeded" => "manifest_budget_exceeded",
        "manifest_read_refused" => "manifest_read_refused",
        "stale_content" => "stale_content",
        "graph_budget_exceeded" => "graph_budget_exceeded",
        _ => "query_refused",
    })?;
    let code = if report["status"] == "partial" { 3 } else { 0 };
    if args.len() == 4 {
        let mut writer = Bounded(Vec::new());
        serde_json::to_writer(&mut writer, &report).map_err(|_| "report_budget_exceeded")?;
        publish(&writer.0, Path::new(&args[3]))?;
        return Ok((
            json!({"schema":report["schema"],"operation":report["operation"],"status":report["status"],
            "request":report["request"],"basis":report["basis"],"basis_sha256":report["basis_sha256"],"coverage":report["coverage"],"paging":report["paging"],"report_written":true,"authorizes_eviction":false}),
            code,
        ));
    }
    Ok((report, code))
}
fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let (report, code) = match run(&args) {
        Ok(value) => value,
        Err(code) => (
            json!({"schema":"tig.query-result.experimental-1","status":"error","error":{"code":code},"authorizes_eviction":false}),
            2,
        ),
    };
    let mut writer = Bounded(Vec::new());
    if serde_json::to_writer(&mut writer, &report).is_err() {
        writer.0=b"{\"schema\":\"tig.query-result.experimental-1\",\"status\":\"error\",\"error\":{\"code\":\"report_budget_exceeded\"},\"authorizes_eviction\":false}".to_vec();
        return if std::io::stdout().write_all(&writer.0).is_ok() {
            ExitCode::from(2)
        } else {
            ExitCode::FAILURE
        };
    }
    writer.0.push(b'\n');
    if std::io::stdout().write_all(&writer.0).is_err() {
        return ExitCode::FAILURE;
    }
    ExitCode::from(code)
}
