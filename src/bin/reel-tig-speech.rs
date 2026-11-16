//! Explicit private speech attempts; no default network operation or selection.
use clap::{Parser, Subcommand};
use std::{path::PathBuf, process::ExitCode};

#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    operation: Operation,
}
#[derive(Subcommand)]
enum Operation {
    /// Verify exact request/declared permission inputs; never read credentials or call a provider.
    Inspect { profile: PathBuf },
    /// Make one authorized provider request and preserve a private, unselected attempt.
    Execute { profile: PathBuf, output: PathBuf },
    /// Capture an existing response; does not attest who generated it or select a voice.
    Capture {
        profile: PathBuf,
        response_sha256: String,
        response_bytes: u64,
        output: PathBuf,
    },
}
fn main() -> ExitCode {
    let result = match Args::parse().operation {
        Operation::Inspect { profile } => reel::speech_generation::inspect(&profile),
        Operation::Execute { profile, output } => {
            reel::speech_generation::execute(&profile, &output)
        }
        Operation::Capture {
            profile,
            response_sha256,
            response_bytes,
            output,
        } => reel::speech_generation::capture(&profile, &response_sha256, response_bytes, &output),
    };
    match result {
        Ok(summary) => {
            println!("{summary}");
            ExitCode::SUCCESS
        }
        Err(_) => {
            // Requests, provider errors, identities and credentials are private artifacts.
            eprintln!(
                "speech operation refused; inspect private attempt evidence and declared inputs"
            );
            ExitCode::FAILURE
        }
    }
}
