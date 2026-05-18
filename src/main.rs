use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Validate { manifest } => {
            let manifest = reel::load_manifest(&manifest)?;
            let report = reel::validate_manifest(&manifest)?;
            println!(
                "manifest ok: {} scenes={:.3}s shots={:.3}s exports={}",
                manifest.path.display(),
                report.scene_total,
                report.shot_total,
                report.exports.len()
            );
        }
        Command::Plan { manifest } => {
            let manifest = reel::load_manifest(&manifest)?;
            let report = reel::validate_manifest(&manifest)?;
            for export in report.exports {
                println!(
                    "{} | {} | {}x{} | {:.3}s | scale={:.3} | {}",
                    export.id,
                    export.aspect_ratio,
                    export.width,
                    export.height,
                    export.duration_seconds,
                    export.duration_scale,
                    export.filename
                );
            }
        }
        Command::Adapters => {
            for adapter in reel::adapters::adapter_catalog() {
                let operations = if adapter.operations.is_empty() {
                    "none".to_string()
                } else {
                    adapter
                        .operations
                        .iter()
                        .map(|operation| operation.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                };
                println!(
                    "{} | {} | operations={} | {}",
                    adapter.id,
                    adapter.status.as_str(),
                    operations,
                    adapter.boundary
                );
            }
        }
        Command::AdapterPlan { manifest, output } => {
            let adapter_plan = reel::adapter_plan(&manifest)?;
            match output {
                OutputFormat::Text => {
                    for adapter in adapter_plan {
                        let operations = if adapter.operations.is_empty() {
                            "none".to_string()
                        } else {
                            adapter
                                .operations
                                .iter()
                                .map(|operation| operation.as_str())
                                .collect::<Vec<_>>()
                                .join(",")
                        };
                        println!(
                            "{} | {} | declared={} | operations={} | {}",
                            adapter.id,
                            adapter.status.as_str(),
                            adapter.declared_by_manifest,
                            operations,
                            adapter.boundary
                        );
                    }
                }
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&adapter_plan)?),
            }
        }
        Command::ContactSheet { manifest, platform } => {
            let sheet = reel::render_contact_sheet(&manifest, &platform)?;
            println!("{}", sheet.display());
        }
        Command::ShotCards { manifest, platform } => {
            let video = reel::render_shot_cards(&manifest, &platform)?;
            println!("{}", video.display());
        }
        Command::Smoke { manifest } => {
            let video = reel::render_smoke(&manifest)?;
            println!("{}", video.display());
        }
        Command::ReviewPack { manifest } => {
            let report = reel::render_review_pack(&manifest)?;
            println!("{}", report.display());
        }
        Command::ReviewAll { root } => {
            let index = reel::render_all_review_packs(&root)?;
            println!("{}", index.display());
        }
    }

    Ok(())
}

#[derive(Debug, Parser)]
#[command(version, about = "REEL manifest and review-pack orchestration")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Validate a REEL manifest contract.
    Validate {
        #[arg(default_value = "works/0001-ash-vale-last-road-before-winter/manifest.yaml")]
        manifest: PathBuf,
    },
    /// Print renderer-neutral export plans derived from a manifest.
    Plan {
        #[arg(default_value = "works/0001-ash-vale-last-road-before-winter/manifest.yaml")]
        manifest: PathBuf,
    },
    /// Print available and planned render adapters.
    Adapters,
    /// Print manifest-aware adapter plan for a REEL manifest.
    AdapterPlan {
        #[arg(default_value = "works/0001-ash-vale-last-road-before-winter/manifest.yaml")]
        manifest: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Render a contact-sheet PNG through FFmpeg.
    ContactSheet {
        #[arg(default_value = "works/0001-ash-vale-last-road-before-winter/manifest.yaml")]
        manifest: PathBuf,
        #[arg(default_value = "youtube-demo")]
        platform: String,
    },
    /// Render a shot-card MP4 through FFmpeg.
    ShotCards {
        #[arg(default_value = "works/0001-ash-vale-last-road-before-winter/manifest.yaml")]
        manifest: PathBuf,
        #[arg(default_value = "youtube-demo")]
        platform: String,
    },
    /// Render a small FFmpeg smoke MP4 from manifest metadata.
    Smoke {
        #[arg(default_value = "manifests/templates/scenario-video.yaml")]
        manifest: PathBuf,
    },
    /// Render one manifest's review pack through Rust orchestration and FFmpeg adapters.
    ReviewPack {
        #[arg(default_value = "works/0001-ash-vale-last-road-before-winter/manifest.yaml")]
        manifest: PathBuf,
    },
    /// Render review packs for every work manifest under a root directory.
    ReviewAll {
        #[arg(default_value = "works")]
        root: PathBuf,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}
