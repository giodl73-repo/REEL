//! Explicit private album operations; no asset discovery or publication.
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
    /// Validate declared inputs/budgets without launching a media tool.
    Plan { request: PathBuf },
    /// Produce an unapproved private packet in a new owned directory.
    Render { request: PathBuf, output: PathBuf },
    /// Reopen a packet and independently decode its lossless/listening masters.
    Check { request: PathBuf, output: PathBuf },
}
fn main() -> Result<()> {
    let value = match Cli::parse().command {
        Operation::Plan { request } => reel::music_album::plan(&request)?,
        Operation::Render { request, output } => reel::music_album::render(&request, &output)?,
        Operation::Check { request, output } => reel::music_album::check(&request, &output)?,
    };
    println!("{}", serde_json::to_string(&value)?);
    Ok(())
}
