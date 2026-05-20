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
        Command::Adapters { output } => {
            let catalog = reel::adapters::adapter_catalog();
            match output {
                OutputFormat::Text => {
                    for adapter in catalog {
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
                            "{} | {} | operations={} | {} | policy={}",
                            adapter.id,
                            adapter.status.as_str(),
                            operations,
                            adapter.boundary,
                            adapter.dependency_policy
                        );
                    }
                }
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&catalog)?),
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
                            "{} | {} | declared={} | operations={} | {} | policy={}",
                            adapter.id,
                            adapter.status.as_str(),
                            adapter.declared_by_manifest,
                            operations,
                            adapter.boundary,
                            adapter.dependency_policy
                        );
                    }
                }
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&adapter_plan)?),
            }
        }
        Command::ScenePlan {
            manifest,
            scene,
            platform,
            output,
        } => {
            let scene_plan = reel::scene_plan(&manifest, &scene, &platform)?;
            match output {
                OutputFormat::Text => {
                    println!(
                        "{} | {} | {}x{} | source={:.3}-{:.3}s | render={:.3}s | shots={}",
                        scene_plan.scene_id,
                        scene_plan.platform,
                        scene_plan.width,
                        scene_plan.height,
                        scene_plan.source_start_seconds,
                        scene_plan.source_start_seconds + scene_plan.source_duration_seconds,
                        scene_plan.render_duration_seconds,
                        scene_plan.shots.len()
                    );
                    for shot in scene_plan.shots {
                        println!(
                            "  {} | source={:.3}-{:.3}s | render={:.3}-{:.3}s",
                            shot.id,
                            shot.source_start_seconds,
                            shot.source_start_seconds + shot.source_duration_seconds,
                            shot.render_start_seconds,
                            shot.render_start_seconds + shot.render_duration_seconds
                        );
                    }
                }
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&scene_plan)?),
            }
        }
        Command::ScenePreview {
            manifest,
            scene,
            platform,
        } => {
            let video = reel::render_scene_preview(&manifest, &scene, &platform)?;
            println!("{}", video.display());
        }
        Command::ScenePreviews { manifest, platform } => {
            for video in reel::render_scene_previews(&manifest, &platform)? {
                println!("{}", video.display());
            }
        }
        Command::WorkPreview { manifest, platform } => {
            let video = reel::render_work_preview(&manifest, &platform)?;
            println!("{}", video.display());
        }
        Command::ArtifactManifest { manifest, output } => {
            let artifact_manifest = reel::render_artifact_manifest(&manifest)?;
            match output {
                OutputFormat::Text => println!("{}", artifact_manifest.display()),
                OutputFormat::Json => println!("{}", std::fs::read_to_string(&artifact_manifest)?),
            }
        }
        Command::ArtifactCheck {
            artifact_manifest,
            output,
        } => {
            let report = reel::check_artifact_manifest(&artifact_manifest)?;
            match output {
                OutputFormat::Text => {
                    println!(
                        "{} | schema={} | generated={} | checked={} | source={} | work={} | adapter={} | platforms={} | scenes={} | videos={} | images={} | files={} | bytes={} | duration={:.3}s",
                        report.artifact_manifest,
                        report.schema_version,
                        report.generated_unix,
                        report.checked_unix,
                        report.source_manifest,
                        report.work,
                        report.baseline_adapter,
                        report.platforms,
                        report.scene_previews,
                        report.video_files,
                        report.image_files,
                        report.files,
                        report.total_bytes,
                        report.total_video_duration_seconds
                    );
                }
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
            }
        }
        Command::ArtifactCheckAll { root, output } => {
            let report = reel::check_all_artifact_manifests(&root)?;
            match output {
                OutputFormat::Text => {
                    println!(
                        "{} | works={} | work_ids={} | titles={} | artifact_manifests={} | sources={} | schemas={} | adapters={} | platforms={} | scenes={} | videos={} | images={} | files={} | bytes={} | duration={:.3}s",
                        report.works_root,
                        report.works,
                        report.work_ids.join(","),
                        report.work_titles.join(";"),
                        report.artifact_manifests.len(),
                        report.source_manifests.len(),
                        report.schema_versions.join(","),
                        report.baseline_adapters.join(","),
                        report.platforms,
                        report.scene_previews,
                        report.video_files,
                        report.image_files,
                        report.files,
                        report.total_bytes,
                        report.total_video_duration_seconds
                    );
                    for item in report.reports {
                        println!(
                            "  {} | schema={} | generated={} | checked={} | source={} | work={} | adapter={} | platforms={} | scenes={} | videos={} | images={} | files={} | bytes={} | duration={:.3}s",
                            item.artifact_manifest,
                            item.schema_version,
                            item.generated_unix,
                            item.checked_unix,
                            item.source_manifest,
                            item.work,
                            item.baseline_adapter,
                            item.platforms,
                            item.scene_previews,
                            item.video_files,
                            item.image_files,
                            item.files,
                            item.total_bytes,
                            item.total_video_duration_seconds
                        );
                    }
                }
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
            }
        }
        Command::Corpus { root, output } => {
            let report = reel::summarize_work_corpus(&root)?;
            match output {
                OutputFormat::Text => {
                    println!(
                        "{} | works={} | manifests={} | manifest_versions={} | work_ids={} | sources={} | source_ids={} | source_paths={} | source_commits={} | audience_primaries={} | audience_contexts={} | audience_desired_effects={} | formats={} | styles={} | alternate_styles={} | platform_names={} | platforms={} | scenes={} | shots={} | exports={} | scene_duration={:.3}s | shot_duration={:.3}s",
                        report.works_root,
                        report.works,
                        report.manifests.join(";"),
                        report.manifest_versions.join(","),
                        report.work_ids.join(","),
                        report.source_repos.join(","),
                        report.source_ids.join(","),
                        report.source_paths.join(","),
                        report.source_commits.join(","),
                        report.audience_primaries.join(";"),
                        report.audience_contexts.join(";"),
                        report.audience_desired_effects.join(";"),
                        report.formats.join(","),
                        report.styles.join(","),
                        report.alternate_styles.join(","),
                        report.platform_names.join(","),
                        report.platforms,
                        report.scenes,
                        report.shots,
                        report.exports,
                        report.total_scene_duration_seconds,
                        report.total_shot_duration_seconds
                    );
                    for item in report.reports {
                        println!(
                            "  {} | version={} | work={} | title={} | source={} | source_path={} | source_commit={} | audience_primary={} | audience_context={} | audience_desired_effect={} | format={} | style={} | alternate_styles={} | platform_names={} | platforms={} | scenes={} | shots={} | exports={} | scene_duration={:.3}s | shot_duration={:.3}s",
                            item.manifest,
                            item.manifest_version,
                            item.work,
                            item.title,
                            item.source_repo,
                            item.source_path,
                            item.source_commit,
                            item.audience_primary,
                            item.audience_context,
                            item.audience_desired_effect,
                            item.format,
                            item.style,
                            item.alternate_styles.join(","),
                            item.platform_names.join(","),
                            item.platforms,
                            item.scenes,
                            item.shots,
                            item.exports,
                            item.scene_duration_seconds,
                            item.shot_duration_seconds
                        );
                    }
                }
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
            }
        }
        Command::ReviewQueue { root, output } => {
            let report = reel::summarize_review_queue(&root)?;
            match output {
                OutputFormat::Text => {
                    println!(
                        "{} | works={} | manifests={} | review_statuses={} | status_counts={} | status_roles={} | required_roles={} | role_counts={} | role_manifests={} | role_work_ids={} | role_work_titles={} | role_status_manifests={} | role_status_work_ids={} | role_status_work_titles={}",
                        report.works_root,
                        report.works,
                        report.manifests.join(";"),
                        report.review_statuses.join(","),
                        report
                            .review_status_counts
                            .iter()
                            .map(|(status, count)| format!("{status}={count}"))
                            .collect::<Vec<_>>()
                            .join(","),
                        report
                            .review_status_required_roles
                            .iter()
                            .map(|(status, roles)| format!("{status}:{}", roles.join(",")))
                            .collect::<Vec<_>>()
                            .join(";"),
                        report.required_roles.join(","),
                        report
                            .required_role_counts
                            .iter()
                            .map(|(role, count)| format!("{role}={count}"))
                            .collect::<Vec<_>>()
                            .join(","),
                        report
                            .required_role_manifests
                            .iter()
                            .map(|(role, manifests)| format!("{role}:{}", manifests.join(",")))
                            .collect::<Vec<_>>()
                            .join(";"),
                        report
                            .required_role_work_ids
                            .iter()
                            .map(|(role, work_ids)| format!("{role}:{}", work_ids.join(",")))
                            .collect::<Vec<_>>()
                            .join(";"),
                        report
                            .required_role_work_titles
                            .iter()
                            .map(|(role, work_titles)| format!("{role}:{}", work_titles.join("|")))
                            .collect::<Vec<_>>()
                            .join(";"),
                        report
                            .required_role_status_manifests
                            .iter()
                            .map(|(role, statuses)| {
                                let status_items = statuses
                                    .iter()
                                    .map(|(status, manifests)| {
                                        format!("{status}={}", manifests.join(","))
                                    })
                                    .collect::<Vec<_>>()
                                    .join(",");
                                format!("{role}:{status_items}")
                            })
                            .collect::<Vec<_>>()
                            .join(";"),
                        report
                            .required_role_status_work_ids
                            .iter()
                            .map(|(role, statuses)| {
                                let status_items = statuses
                                    .iter()
                                    .map(|(status, work_ids)| {
                                        format!("{status}={}", work_ids.join(","))
                                    })
                                    .collect::<Vec<_>>()
                                    .join(",");
                                format!("{role}:{status_items}")
                            })
                            .collect::<Vec<_>>()
                            .join(";"),
                        report
                            .required_role_status_work_titles
                            .iter()
                            .map(|(role, statuses)| {
                                let status_items = statuses
                                    .iter()
                                    .map(|(status, work_titles)| {
                                        format!("{status}={}", work_titles.join("|"))
                                    })
                                    .collect::<Vec<_>>()
                                    .join(",");
                                format!("{role}:{status_items}")
                            })
                            .collect::<Vec<_>>()
                            .join(";")
                    );
                    for item in report.reports {
                        println!(
                            "  {} | work={} | title={} | status={} | roles={}",
                            item.manifest,
                            item.work,
                            item.title,
                            item.review_status,
                            item.required_roles.join(",")
                        );
                    }
                }
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
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
        Command::Demo { manifest } => {
            let demo = reel::render_demo(&manifest)?;
            println!("{}", demo.display());
        }
        Command::RemotionPack {
            manifest,
            platform,
            scene,
        } => {
            let package = reel::render_remotion_package_for_scene(&manifest, &platform, &scene)?;
            println!("{}", package.display());
        }
        Command::ReviewAll { root, output } => {
            let report = reel::render_all_review_pack_report(&root)?;
            match output {
                OutputFormat::Text => println!("{}", report.index),
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
            }
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
    Adapters {
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Print manifest-aware adapter plan for a REEL manifest.
    AdapterPlan {
        #[arg(default_value = "works/0001-ash-vale-last-road-before-winter/manifest.yaml")]
        manifest: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Print a scene-level render plan for one manifest scene and platform.
    ScenePlan {
        #[arg(default_value = "works/0001-ash-vale-last-road-before-winter/manifest.yaml")]
        manifest: PathBuf,
        #[arg(default_value = "scene-01")]
        scene: String,
        #[arg(default_value = "youtube-demo")]
        platform: String,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Render one scene preview MP4 through the FFmpeg baseline adapter.
    ScenePreview {
        #[arg(default_value = "works/0001-ash-vale-last-road-before-winter/manifest.yaml")]
        manifest: PathBuf,
        #[arg(default_value = "scene-01")]
        scene: String,
        #[arg(default_value = "youtube-demo")]
        platform: String,
    },
    /// Render every scene preview MP4 for one platform through FFmpeg.
    ScenePreviews {
        #[arg(default_value = "works/0001-ash-vale-last-road-before-winter/manifest.yaml")]
        manifest: PathBuf,
        #[arg(default_value = "youtube-demo")]
        platform: String,
    },
    /// Render a full-work preview MP4 by concatenating all scene previews.
    WorkPreview {
        #[arg(default_value = "works/0001-ash-vale-last-road-before-winter/manifest.yaml")]
        manifest: PathBuf,
        #[arg(default_value = "youtube-demo")]
        platform: String,
    },
    /// Render baseline artifacts and write a machine-readable artifact manifest.
    ArtifactManifest {
        #[arg(default_value = "works/0001-ash-vale-last-road-before-winter/manifest.yaml")]
        manifest: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Verify a generated artifact manifest's files and byte sizes.
    ArtifactCheck {
        #[arg(
            default_value = "renders/artifacts/0001-ash-vale-last-road-before-winter-artifacts.json"
        )]
        artifact_manifest: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Generate and verify artifact manifests for every work under a root.
    ArtifactCheckAll {
        #[arg(default_value = "works")]
        root: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Validate and summarize all work manifests under a root without rendering media.
    Corpus {
        #[arg(default_value = "works")]
        root: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Summarize manifest-owned review queue metadata without rendering media.
    ReviewQueue {
        #[arg(default_value = "works")]
        root: PathBuf,
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
    /// Render a browser-openable HTML demo page for one manifest.
    Demo {
        #[arg(default_value = "works/0001-ash-vale-last-road-before-winter/manifest.yaml")]
        manifest: PathBuf,
    },
    /// Create a Remotion handoff package without installing or running Node.
    RemotionPack {
        #[arg(default_value = "works/0001-ash-vale-last-road-before-winter/manifest.yaml")]
        manifest: PathBuf,
        #[arg(default_value = "youtube-demo")]
        platform: String,
        #[arg(default_value = "scene-01")]
        scene: String,
    },
    /// Render review packs for every work manifest under a root directory.
    ReviewAll {
        #[arg(default_value = "works")]
        root: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}
