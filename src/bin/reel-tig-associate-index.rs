use clap::Parser;
use std::{path::PathBuf, process::ExitCode};
#[derive(Parser)]
#[command(
    about = "Associate a pinned checksum index with saved target metadata; never a provider receipt"
)]
struct Args {
    profile: PathBuf,
    output: PathBuf,
}
fn main() -> ExitCode {
    let args = Args::parse();
    match reel::tig_index_association::run(&args.profile, &args.output) {
        Ok(report) => {
            println!("{report}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("index association refused: {error}");
            ExitCode::FAILURE
        }
    }
}
