//! Native fixed-duration masked materialization; never a provider or approval.
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Operation,
}
#[derive(Subcommand)]
enum Operation {
    Plan {
        admission: PathBuf,
        recipe: PathBuf,
    },
    Materialize {
        admission: PathBuf,
        recipe: PathBuf,
        candidates: PathBuf,
        output: PathBuf,
    },
    Check {
        admission: PathBuf,
        recipe: PathBuf,
        candidates: PathBuf,
        output: PathBuf,
    },
}
fn main() -> Result<()> {
    use reel::song_edition::repair;
    let report = match Cli::parse().command {
        Operation::Plan { admission, recipe } => repair::plan(&admission, &recipe)?,
        Operation::Materialize {
            admission,
            recipe,
            candidates,
            output,
        } => repair::materialize(&admission, &recipe, &candidates, &output)?,
        Operation::Check {
            admission,
            recipe,
            candidates,
            output,
        } => repair::check(&admission, &recipe, &candidates, &output)?,
    };
    println!("{report}");
    Ok(())
}
