//! Explicit installation metadata tools. Playback/operator session is a later
//! adapter component; neither argument parsing nor Validate opens a device.
use anyhow::{Result, ensure};
use clap::{Parser, Subcommand};
use reel::spoken_programme::installation::native::{self, EndpointBinding, ProbeRequest};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};
#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    command: Action,
}
#[derive(Subcommand)]
enum Action {
    /// Check a private endpoint binding's declared shape, without probing.
    ValidateBinding { binding: PathBuf },
    /// Explicitly observe one pinned WASAPI output in an owned quiet worker.
    Probe {
        request: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    #[command(name = "__probe-worker", hide = true)]
    Worker,
}
fn load<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let file = File::open(path)?;
    ensure!(
        file.metadata()?.len() <= 4096,
        "installation control byte profile"
    );
    let mut raw = Vec::new();
    file.take(4097).read_to_end(&mut raw)?;
    ensure!(raw.len() <= 4096, "installation control byte profile");
    Ok(serde_json::from_slice(&raw)?)
}
fn run() -> Result<()> {
    match Args::parse().command {
        Action::ValidateBinding { binding } => {
            let value: EndpointBinding = load(&binding)?;
            value.validate()?;
            println!(
                "{}",
                serde_json::json!({"binding_shape_checked":true,"endpoint_probed":false,"device_playback_checked":false})
            );
        }
        Action::Probe { request, output } => {
            ensure!(
                !output.exists(),
                "installation private output already exists"
            );
            let request: ProbeRequest = load(&request)?;
            let observed = native::probe(&request, || Ok(()))?;
            let parent = output
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let mut file = tempfile::NamedTempFile::new_in(parent)?;
            file.write_all(&serde_json::to_vec_pretty(&observed)?)?;
            file.as_file().sync_all()?;
            file.persist_noclobber(&output)?;
            println!(
                "{}",
                serde_json::json!({"endpoint_metadata_observed":true,"device_playback_checked":false})
            );
        }
        Action::Worker => native::probe_worker()?,
    }
    Ok(())
}
fn main() {
    if run().is_err() {
        eprintln!("installation operation refused; no playback qualification");
        std::process::exit(1)
    }
}
