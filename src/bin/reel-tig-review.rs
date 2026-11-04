//! Existing-byte frozen review entry; never renders or opens a viewer.
use clap::{Parser, Subcommand};
use std::{path::PathBuf, process::ExitCode};
#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    operation: Operation,
}
#[derive(Subcommand)]
enum Operation {
    Aggregate {
        profile: PathBuf,
        output: PathBuf,
    },
    Inspect {
        profile: PathBuf,
    },
    Reserve {
        profile: PathBuf,
        #[arg(long)]
        owner: String,
    },
    Resume {
        profile: PathBuf,
    },
    Submit {
        profile: PathBuf,
    },
}
fn main() -> ExitCode {
    use reel::tig_review::admission;
    let result = match Args::parse().operation {
        Operation::Aggregate { profile, output } => admission::aggregate(&profile, &output),
        Operation::Inspect { profile } => admission::inspect(&profile),
        Operation::Reserve { profile, owner } => admission::reserve(&profile, &owner),
        Operation::Resume { profile } => admission::resume(&profile),
        Operation::Submit { profile } => admission::submit(&profile),
    };
    match result {
        Ok(value) => {
            println!("{value}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::FAILURE
        }
    }
}
