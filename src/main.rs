use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

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
