use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
#[derive(Parser)]
#[command(
    about = "Assemble SRT/VTT on a hash-bound verified native episode clock; no media execution"
)]
struct Args {
    manifest: PathBuf,
    #[arg(long)]
    input_root: PathBuf,
    #[arg(long)]
    output_dir: PathBuf,
}
fn main() -> Result<()> {
    let args = Args::parse();
    reel::episode_captions::assemble(&args.manifest, &args.input_root, &args.output_dir)
}
