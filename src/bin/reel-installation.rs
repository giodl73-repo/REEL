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
    /// Open an explicit Windows console session for the pinned programme.
    #[cfg(windows)]
    Run { request: PathBuf },
    /// Check a private endpoint binding's declared shape, without probing.
    ValidateBinding { binding: PathBuf },
    /// Check private session shape, without reading originals or devices.
    ValidateSession { request: PathBuf },
    /// Independently check pinned original PCM/text. No endpoint or playback.
    CheckSession { request: PathBuf },
    /// Explicitly observe one pinned WASAPI output in an owned quiet worker.
    Probe {
        request: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    #[command(name = "__probe-worker", hide = true)]
    Worker,
    #[cfg(windows)]
    #[command(name = "__session-worker", hide = true)]
    SessionWorker {
        request: PathBuf,
        bootstrap: PathBuf,
    },
}
fn bytes(path: &Path, max: usize) -> Result<Vec<u8>> {
    let file = File::open(path)?;
    ensure!(
        file.metadata()?.len() <= max as u64,
        "installation control byte profile"
    );
    let mut raw = Vec::new();
    file.take(max as u64 + 1).read_to_end(&mut raw)?;
    ensure!(raw.len() <= max, "installation control byte profile");
    Ok(raw)
}
fn load<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    Ok(serde_json::from_slice(&bytes(path, 4096)?)?)
}
fn session(path: &Path) -> Result<native::SessionRequest> {
    native::SessionRequest::parse(&bytes(path, native::MAX_SESSION_REQUEST_BYTES)?)
}
fn run() -> Result<()> {
    match Args::parse().command {
        #[cfg(windows)]
        Action::Run { request } => {
            native::worker::run_foreground(&bytes(&request, native::MAX_SESSION_REQUEST_BYTES)?)?
        }
        Action::ValidateSession { request } => {
            session(&request)?;
            println!(
                "{}",
                serde_json::json!({"session_shape_checked":true,
                "originals_checked":false,"endpoint_probed":false,"device_playback_checked":false})
            );
        }
        Action::CheckSession { request } => {
            let summary = session(&request)?.check_originals(
                reel::spoken_programme::producer::ProductionControl {
                    timeout: std::time::Duration::from_secs(30),
                    cancellation: || Ok(()),
                },
            )?;
            println!("{}", serde_json::to_string(&summary)?);
        }
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
        #[cfg(windows)]
        Action::SessionWorker { request, bootstrap } => {
            native::worker::run_worker(
                &bytes(&request, native::MAX_SESSION_REQUEST_BYTES)?,
                load(&bootstrap)?,
            )?;
        }
    }
    Ok(())
}
fn main() {
    if run().is_err() {
        eprintln!("installation operation refused; no playback qualification");
        std::process::exit(1)
    }
}
