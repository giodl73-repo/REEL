//! Explicit-profile experimental Tig consumer; no configuration installation.
use anyhow::{Result, anyhow, bail};
use clap::{Parser, Subcommand};
use reel::{
    scene_delivery,
    tig_inputs::{Profile, Source, read_job, read_reference},
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
    CompileNative {
        request: PathBuf,
        output: PathBuf,
    },
    Plan {
        job: PathBuf,
    },
    Render {
        job: PathBuf,
        output: PathBuf,
    },
    RenderAudio {
        job: PathBuf,
        output: PathBuf,
    },
    Check {
        job: PathBuf,
        output: PathBuf,
    },
    CheckAudio {
        job: PathBuf,
        output: PathBuf,
    },
    Compare {
        job: PathBuf,
        output: PathBuf,
        reference: PathBuf,
    },
}
fn run() -> Result<()> {
    let args = Args::parse();
    if let Operation::CompileNative { request, output } = &args.command {
        let mut source = Source::open(Profile::read(&args.profile)?)?;
        let result = reel::tig_scene::compile(
            &mut source,
            &reel::tig_scene::Request::read(request)?,
            output,
        )?;
        println!(
            "{}",
            serde_json::json!({"schema":"reel.tig-scene-result.experimental-1","source_checkpoint":source.checkpoint(),"complete_semantic_coverage":false,"result":result})
        );
        return Ok(());
    }
    let job_path = match &args.command {
        Operation::Plan { job }
        | Operation::Render { job, .. }
        | Operation::RenderAudio { job, .. }
        | Operation::Check { job, .. }
        | Operation::CheckAudio { job, .. }
        | Operation::Compare { job, .. } => job,
        Operation::CompileNative { .. } => unreachable!(),
    };
    let job_path = job_path
        .canonicalize()
        .map_err(|_| anyhow!("cannot resolve job file"))?;
    let job = read_job(&job_path)?;
    let mut source = Source::open(Profile::read(&args.profile)?)?;
    let inputs = source.prepare(&job)?;
    let comparison = matches!(&args.command, Operation::Compare { .. });
    let result = match args.command {
        Operation::CompileNative { .. } => unreachable!(),
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
        Operation::Compare {
            output, reference, ..
        } => {
            let reference = read_reference(&reference)?;
            let reference_inputs = source.prepare_files(std::slice::from_ref(&reference))?;
            let receipt = scene_delivery::check_with_inputs(&job_path, &inputs, &output)?;
            let master = receipt
                .outputs
                .get("master.mkv")
                .ok_or_else(|| anyhow!("candidate master missing"))?;
            let plan = &receipt.plan;
            let shape = reel::media_parity::Shape {
                width: job.width,
                height: job.height,
                fps_numerator: plan.fps_numerator,
                fps_denominator: plan.fps_denominator,
                frames: plan.frame_count,
                sample_rate: plan.sample_rate,
                samples: plan.duration_samples,
            };
            serde_json::to_value(reel::media_parity::compare_with_inputs(
                &reference,
                &reference_inputs,
                master,
                output.as_path(),
                &shape,
            )?)?
        }
    };
    println!(
        "{}",
        serde_json::json!({"schema":"reel.tig-scene-result.experimental-1","source_checkpoint":inputs.checkpoint(),"input_names":inputs.names(),"verified_objects":inputs.objects(),"complete_semantic_coverage":false,"result":result})
    );
    if comparison && result["equal"] != true {
        bail!("decoded media comparison differs");
    }
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
