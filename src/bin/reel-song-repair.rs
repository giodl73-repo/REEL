//! Native masked materialization and explicitly enabled bounded provider execution.
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
    #[cfg(feature = "song-provider")]
    CheckGeneration {
        admission: PathBuf,
        recipe: PathBuf,
        execution: PathBuf,
        output: PathBuf,
    },
    #[cfg(feature = "song-provider")]
    PlanGeneration {
        admission: PathBuf,
        recipe: PathBuf,
        execution: PathBuf,
    },
    #[cfg(feature = "song-provider")]
    Generate {
        admission: PathBuf,
        recipe: PathBuf,
        execution: PathBuf,
        output: PathBuf,
    },
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
        #[cfg(feature = "song-provider")]
        Operation::CheckGeneration {
            admission,
            recipe,
            execution,
            output,
        } => repair::provider::check_generation(&admission, &recipe, &execution, &output)?,
        #[cfg(feature = "song-provider")]
        Operation::PlanGeneration {
            admission,
            recipe,
            execution,
        } => repair::provider::plan(&admission, &recipe, &execution)?,
        #[cfg(feature = "song-provider")]
        Operation::Generate {
            admission,
            recipe,
            execution,
            output,
        } => {
            let output = if output.is_absolute() {
                output
            } else {
                if !output
                    .components()
                    .all(|c| matches!(c, std::path::Component::Normal(_)))
                {
                    anyhow::bail!("managed provider output must stay in its named relative view")
                };
                std::env::current_dir()?.join(output)
            };
            repair::provider::generate(&admission, &recipe, &execution, &output)?
        }
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
