//! Explicit-profile experimental Tig consumer; no configuration installation.
use anyhow::{Result, anyhow};
use clap::{Parser, Subcommand};
use reel::{
    scene_delivery,
    tig_inputs::{Profile, Source, read_job},
};
use std::{path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(about = "Render/check native scenes using an explicit committed Tig source")]
struct Args {
    #[arg(long)]
    profile: PathBuf,
    #[command(subcommand)]
    command: Operation,
}
#[derive(Subcommand)]
enum Operation {
    Plan { job: PathBuf },
    Render { job: PathBuf, output: PathBuf },
    RenderAudio { job: PathBuf, output: PathBuf },
    Check { job: PathBuf, output: PathBuf },
    CheckAudio { job: PathBuf, output: PathBuf },
}
fn run() -> Result<()> {
    let args = Args::parse();
    let job_path = match &args.command {
        Operation::Plan { job }
        | Operation::Render { job, .. }
        | Operation::RenderAudio { job, .. }
        | Operation::Check { job, .. }
        | Operation::CheckAudio { job, .. } => job,
    };
    let job_path = job_path
        .canonicalize()
        .map_err(|_| anyhow!("cannot resolve job file"))?;
    let job = read_job(&job_path)?;
    let mut source = Source::open(Profile::read(&args.profile)?)?;
    let inputs = source.prepare(&job)?;
    let result = match args.command {
        Operation::Plan { .. } => {
            serde_json::to_value(scene_delivery::plan_with_inputs(&job_path, &inputs)?.1)?
        }
        Operation::Render { output, .. } => serde_json::to_value(
            scene_delivery::render_with_inputs(&job_path, &inputs, &output)?,
        )?,
        Operation::RenderAudio { output, .. } => serde_json::to_value(
            scene_delivery::render_audio_with_inputs(&job_path, &inputs, &output)?,
        )?,
        Operation::Check { output, .. } => serde_json::to_value(
            scene_delivery::check_with_inputs(&job_path, &inputs, &output)?,
        )?,
        Operation::CheckAudio { output, .. } => serde_json::to_value(
            scene_delivery::check_audio_with_inputs(&job_path, &inputs, &output)?,
        )?,
    };
    println!(
        "{}",
        serde_json::json!({"schema":"reel.tig-scene-result.experimental-1","source_checkpoint":inputs.checkpoint(),"input_names":inputs.names(),"verified_objects":inputs.objects(),"complete_semantic_coverage":false,"result":result})
    );
    Ok(())
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
