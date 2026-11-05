//! Explicit private source correspondence; no provider, selection or rewrite.
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
    Inspect { admission: PathBuf, output: PathBuf },
}
fn main() -> Result<()> {
    let Operation::Inspect { admission, output } = Cli::parse().command;
    let inspected = reel::song_edition::source::inspect(&admission)?;
    inspected.write(&output)?;
    println!("{}", inspected.summary());
    Ok(())
}
