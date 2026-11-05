//! Explicit sample-preserving editions from retained source data, never a model.
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
    Plan { request: PathBuf },
    Render { request: PathBuf, output: PathBuf },
    Check { request: PathBuf, output: PathBuf },
}
fn main() -> Result<()> {
    let value = match Cli::parse().command {
        Operation::Plan { request } => reel::song_edition::packet::plan(&request)?,
        Operation::Render { request, output } => {
            reel::song_edition::packet::render(&request, &output)?
        }
        Operation::Check { request, output } => {
            reel::song_edition::packet::check(&request, &output)?
        }
    };
    println!("{}", serde_json::to_string(&value)?);
    Ok(())
}
