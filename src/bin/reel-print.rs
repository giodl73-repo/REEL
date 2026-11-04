//! Explicit immutable request; no providers, Python, browser or cache mutation.
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    operation: Operation,
}
#[derive(Subcommand)]
enum Operation {
    /// Compute the exact packet in memory and report its size without writes.
    Plan { request: PathBuf },
    /// Produce a new private PDF packet after complete input and output preflight.
    Render { request: PathBuf, output: PathBuf },
    /// Rebuild in memory and compare every packet byte; no output writes.
    Check { request: PathBuf, output: PathBuf },
}
fn main() -> Result<()> {
    let report = match Args::parse().operation {
        Operation::Plan { request } => reel_print::plan(&request)?,
        Operation::Render { request, output } => reel_print::render(&request, &output)?,
        Operation::Check { request, output } => reel_print::check(&request, &output)?,
    };
    println!(
        "{}",
        serde_json::json!({"pages":report.pages.len(),"source_files":report.inputs.len(),
            "output_files":report.outputs.len()+1,"output_bytes":report.outputs.values().map(|p|p.bytes).sum::<u64>()+serde_json::to_vec_pretty(&report)?.len() as u64,
        "human_approval":false,"published":false,"authorizes_eviction":false})
    );
    Ok(())
}
