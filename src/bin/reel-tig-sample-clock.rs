//! Explicit source-bound draft rendering; no automatic setup or publication.
use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    profile: PathBuf,
    request: PathBuf,
    #[arg(long)]
    output_dir: PathBuf,
    #[arg(long)]
    max_output_bytes: u64,
}
fn main() -> Result<()> {
    let args = Args::parse();
    let report = reel::tig_sample_clock::render(
        reel::tig_inputs::Profile::read(&args.profile)?,
        &args.request,
        &args.output_dir,
        args.max_output_bytes,
    )?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}
