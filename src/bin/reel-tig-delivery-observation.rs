use clap::Parser;
use std::{path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(
    about = "Reconcile pinned file metadata with retained Tig publication evidence; no media/provider reads"
)]
struct Args {
    profile: PathBuf,
    /// Private, new report file. Contains observed paths and asset IDs.
    output: PathBuf,
}
fn main() -> ExitCode {
    let args = Args::parse();
    match reel::tig_delivery_observation::run(&args.profile, &args.output) {
        Ok(summary) => {
            println!("{summary}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("delivery observation refused: {error}");
            ExitCode::FAILURE
        }
    }
}
