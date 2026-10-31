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
    /// Replay an already enrolled generic job; no setup, source adoption or media claim.
    RunFrozen { profile: PathBuf },
    /// Inspect a declared generation basis; no model execution or primary reads.
    InspectGeneration { profile: PathBuf, output: PathBuf },
    /// Inspect a declared generator set and order blocked/catch-up advice.
    InspectGenerationSet { profile: PathBuf, output: PathBuf },
    /// Complete an already-recorded result adoption; never render or remove scratch.
    AdoptPrepared { profile: PathBuf },
    /// Verify a complete published packet, then record prepare/adopt; preserve scratch.
    RetainPublished { profile: PathBuf },
    /// Retain declared interrupted-job candidates; dry-run unless --apply.
    SalvageWorkspace {
        profile: PathBuf,
        #[arg(long)]
        apply: bool,
    },
    /// Verify owned scratch retirement; remove it only with explicit --apply.
    RetireWorkspace {
        profile: PathBuf,
        #[arg(long)]
        apply: bool,
    },
    /// Verify retained components and assemble an episode in admitted scratch.
    RunEpisode { profile: PathBuf },
    /// Resume a completed episode job's verified metadata/owner handoff only.
    FinishEpisode { profile: PathBuf },
    #[command(hide = true)]
    EpisodeWorker {
        ticket: PathBuf,
        #[arg(long)]
        sha256: String,
        #[arg(long)]
        bytes: u64,
    },
    /// Verify exact PCM-WAV reconstruction; publish only with explicit admission.
    RecoverPcmWave {
        profile: PathBuf,
        #[arg(long)]
        publish: bool,
    },
    /// Compile a committed presentation source without media reads or writes.
    InspectPresentation { profile: PathBuf },
    /// Independently compare a selected presentation master with its primaries.
    /// This read-only observation does not adopt a managed job or publish media.
    ComparePresentation { profile: PathBuf },
    /// Publish one exact caller-owned file through an explicitly enrolled cache.
    PublishFile { profile: PathBuf, output: PathBuf },
    /// Recheck retained native outputs and compare their master with a baseline.
    CompareRetained { profile: PathBuf, output: PathBuf },
    /// Recompile original controls and verify retained bytes; no production claim.
    RevalidateRetained { profile: PathBuf, output: PathBuf },
    /// Explicit one-time shared metadata/journal setup; never invoked by Run.
    Setup { profile: PathBuf, output: PathBuf },
    /// Check existing named stores against private trusted-minimum bindings.
    CheckStores { bindings: PathBuf },
    /// Freeze and read back a committed scene/episode into a private fragment.
    CaptureContext { profile: PathBuf, output: PathBuf },
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
    let operation = Args::parse().operation;
    let comparison = matches!(
        &operation,
        Operation::CompareRetained { .. } | Operation::RevalidateRetained { .. }
    );
    let episode_job = matches!(
        &operation,
        Operation::RunEpisode { .. } | Operation::RunFrozen { .. }
    );
    let result = match operation {
        Operation::RunFrozen { profile } => reel::tig_frozen_job_cli::run(&profile),
        Operation::InspectGeneration { profile, output } => {
            reel::tig_generation::inspect(&profile, &output)
        }
        Operation::InspectGenerationSet { profile, output } => {
            reel::tig_generation::inspect_set(&profile, &output)
        }
        Operation::AdoptPrepared { profile } => reel::tig_job_runner::adopt_prepared(&profile),
        Operation::RetainPublished { profile } => reel::tig_job_runner::retain_published(&profile),
        Operation::SalvageWorkspace { profile, apply } => {
            reel::tig_job_runner::salvage_workspace(&profile, apply)
        }
        Operation::RetireWorkspace { profile, apply } => {
            reel::tig_job_runner::retire_workspace(&profile, apply)
        }
        Operation::RunEpisode { profile } => reel::tig_episode_cli::run(&profile),
        Operation::FinishEpisode { profile } => reel::tig_episode_cli::finish_episode(&profile),
        Operation::EpisodeWorker {
            ticket,
            sha256,
            bytes,
        } => (|| -> anyhow::Result<_> {
            let expected = tig_core::Object {
                sha256: tig_core::Digest::parse(&sha256)?,
                bytes,
            };
            reel::tig_episode_cli::worker(&ticket, &expected)
        })(),
        Operation::RecoverPcmWave { profile, publish } => {
            reel::tig_pcm_recovery::run(&profile, publish)
        }
        Operation::InspectPresentation { profile } => reel::tig_presentation_cli::inspect(&profile),
        Operation::ComparePresentation { profile } => reel::tig_presentation_cli::compare(&profile),
        Operation::PublishFile { profile, output } => {
            reel::tig_native_job_cli::publish_file(&profile, &output)
        }
        Operation::CompareRetained { profile, output } => {
            reel::tig_native_job_cli::compare_retained(&profile, &output)
        }
        Operation::RevalidateRetained { profile, output } => {
            reel::tig_native_job_cli::revalidate_retained(&profile, &output)
        }
        Operation::Setup { profile, output } => {
            reel::tig_native_job_cli::setup_stores(&profile, &output)
        }
        Operation::CheckStores { bindings } => reel::tig_native_job_cli::check_stores(&bindings),
        Operation::CaptureContext { profile, output } => {
            reel::tig_native_job_cli::capture_context(&profile, &output)
        }
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
            if (comparison && value["equal"] != true) || (episode_job && value["succeeded"] != true)
            {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::FAILURE
        }
    }
}
