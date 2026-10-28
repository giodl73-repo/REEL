//! Explicit existing-root coordinator and contained native worker.
use clap::{Parser, Subcommand};
use std::{path::PathBuf, process::ExitCode};
#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    operation: Operation,
}
#[derive(Subcommand)]
enum Operation {
    /// Adopt planned resources, render in bounded scratch and retain exact results.
    Run { profile: PathBuf },
    /// Run a bounded planning job and retain its output contract; no media render.
    PlanOutputs { profile: PathBuf },
    /// Inspect preparation allowances from existing roots; no writes or execution.
    PreparationBudget { profile: PathBuf },
    /// Internal trusted child route. Use Run for admission/containment/retention.
    #[command(hide = true)]
    Worker {
        ticket: PathBuf,
        #[arg(long)]
        sha256: String,
        #[arg(long)]
        bytes: u64,
    },
}
fn main() -> ExitCode {
    let result = match Args::parse().operation {
        Operation::Run { profile } => reel::tig_native_job_cli::run(&profile),
        Operation::PlanOutputs { profile } => reel::tig_native_job_cli::plan_outputs(&profile),
        Operation::PreparationBudget { profile } => {
            reel::tig_native_job_cli::preparation_budget(&profile)
        }
        Operation::Worker {
            ticket,
            sha256,
            bytes,
        } => (|| -> anyhow::Result<_> {
            let expected = tig_core::Object {
                sha256: tig_core::Digest::parse(&sha256)?,
                bytes,
            };
            reel::tig_native_job_cli::worker(&ticket, &expected)
        })(),
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
