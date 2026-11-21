//! Explicit local application launch; no setup, hydration or automatic deletion.
use git_asset::catalogue::{Catalogue, Config};
use std::{path::Path, process::ExitCode};
fn save(report: &serde_json::Value, path: &str) -> Result<(), String> {
    use std::io::Write;
    if !Path::new(path).is_absolute() {
        return Err("Report path must be absolute".into());
    }
    let raw = serde_json::to_vec_pretty(report).map_err(|_| "Cannot encode report")?;
    if raw.len() > 16 * 1024 * 1024 {
        return Err("Report exceeds 16 MiB budget".into());
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "Report must be a new writable file")?;
    file.write_all(&raw)
        .and_then(|_| file.sync_all())
        .map_err(|_| "Cannot finish report; partial output is not evidence".into())
}
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 2
        || !matches!(
            args[0].as_str(),
            "check" | "serve" | "shared" | "references"
        )
    {
        return Err("usage: git-asset-catalogue check ENROLLMENT | serve ENROLLMENT [PORT] | shared ENROLLMENT LEFT RIGHT NEW_REPORT [CATEGORY] | references ENROLLMENT SHA256 NEW_REPORT [PROJECT] [OFFSET]".into());
    }
    let config = Config::load(Path::new(&args[1]))?;
    if args[0] == "shared" {
        if !(5..=6).contains(&args.len()) || !Path::new(&args[4]).is_absolute() {
            return Err("shared requires enrollment, two project IDs and an absolute new report path; category is optional".into());
        }
        let report = git_asset::catalogue::shared::compare(
            config,
            &args[2],
            &args[3],
            args.get(5).map_or("", String::as_str),
        )?;
        save(&report, &args[4])?;
        println!(
            "{}",
            serde_json::json!({"shared_objects":report["shared_objects"],"shared_unique_bytes":report["shared_unique_bytes"],"payloads_opened":0,"complete_project_comparison":false})
        );
        return Ok(());
    }
    if args[0] == "references" {
        if !(4..=6).contains(&args.len()) {
            return Err("references requires enrollment, SHA-256 and a new report; project and offset are optional".into());
        }
        let project = args.get(4).map_or("all", String::as_str);
        let offset = args
            .get(5)
            .map_or(Ok(0), |s| s.parse::<usize>().map_err(|_| "Invalid offset"))?;
        let mut config = config;
        if project != "all" {
            if !config.projects.iter().any(|p| p.id == project) {
                return Err("Unknown project".into());
            }
            config.projects.retain(|p| p.id == project);
            config.default_project = None;
            config.storage_domains.clear();
        }
        let report = Catalogue::load(config)?.object_references(&args[2], project, offset)?;
        save(&report, &args[3])?;
        println!(
            "{}",
            serde_json::json!({"total":report["total"],"offset":offset,"returned":report["items"].as_array().map_or(0,Vec::len),"payloads_opened":0,"complete_project_comparison":false})
        );
        return Ok(());
    }
    if args[0] == "serve" {
        eprintln!(
            "Loading {} enrolled project snapshots (metadata only). Large catalogs may take a minute; no media is being copied.",
            config.projects.len()
        );
    }
    let catalogue = Catalogue::load(config)?;
    if args[0] == "check" {
        if args.len() != 2 {
            return Err("check requires one enrollment".into());
        }
        println!(
            "{}",
            serde_json::json!({"projects":catalogue.projects.len(),"read_only":true,"payloads_opened":0,"automatic_eviction":false})
        );
        return Ok(());
    }
    if args.len() > 3 {
        return Err("Unexpected arguments".into());
    }
    let port = if let Some(p) = args.get(2) {
        p.parse::<u16>().map_err(|_| "Invalid port")?
    } else {
        8799
    };
    git_asset::catalogue::http::serve(catalogue, port)
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Catalogue: {e}");
            ExitCode::FAILURE
        }
    }
}
