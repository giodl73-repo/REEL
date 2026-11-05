//! Explicit private presentation over a retained complete album.
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
        Operation::Plan { request } => reel::music_album::video::plan(&request)?,
        Operation::Render { request, output } => {
            reel::music_album::video::render(&request, &output)?
        }
        Operation::Check { request, output } => reel::music_album::video::check(&request, &output)?,
    };
    println!("{}", serde_json::to_string(&value)?);
    Ok(())
}
