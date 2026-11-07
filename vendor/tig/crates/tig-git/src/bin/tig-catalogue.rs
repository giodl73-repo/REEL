//! Explicit local application launch; no setup, hydration or automatic deletion.
use std::{path::Path, process::ExitCode};
use tig_git::catalogue::{Catalogue, Config};
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 2 || !matches!(args[0].as_str(), "check" | "serve") {
        return Err("usage: tig-catalogue check ENROLLMENT | serve ENROLLMENT [PORT]".into());
    }
    let config = Config::load(Path::new(&args[1]))?;
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
    tig_git::catalogue::http::serve(catalogue, port)
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
