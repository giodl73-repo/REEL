use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;

fn print_report(report: &impl Serialize, output: OutputFormat) -> Result<()> {
    match output {
        OutputFormat::Text | OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(report)?)
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Validate { manifest, output } => {
            if reel::production::is_production_manifest(&manifest)? {
                let loaded = reel::production::load(&manifest)?;
                let report = reel::production::validate(&loaded)?;
                match output {
                    OutputFormat::Text => println!(
                        "manifest ok: {} version={} profile={} timing={} scenes={} shots={} speakers={} cues={} duration={} preview_ready={} delivery_ready={} gated={}",
                        report.manifest,
                        report.version,
                        report.profile,
                        report.timing_status,
                        report.scenes,
                        report.shots,
                        report.speakers,
                        report.narration_cues,
                        report
                            .duration_ms
                            .map(|value| format!("{value}ms"))
                            .unwrap_or_else(|| "untimed".to_string()),
                        report.preview_ready,
                        report.delivery_ready,
                        report.gated_commands.join(",")
                    ),
                    OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
                }
            } else {
                let loaded = reel::load_manifest(&manifest)?;
                let report = reel::validate_manifest(&loaded)?;
                match output {
                    OutputFormat::Text => println!(
                        "manifest ok: {} scenes={:.3}s shots={:.3}s exports={}",
                        loaded.path.display(),
                        report.scene_total,
                        report.shot_total,
                        report.exports.len()
                    ),
                    OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
                }
            }
        }
        Command::Plan { manifest, output } => {
            if reel::production::is_production_manifest(&manifest)? {
                let loaded = reel::production::load(&manifest)?;
                let plan = reel::production::plan(&loaded)?;
                match output {
                    OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&plan)?),
                    OutputFormat::Text => {
                        println!(
                            "{} | timing={} | gated={}",
                            plan.work,
                            plan.timing_status,
                            plan.gated_commands.join(",")
                        );
                        for scene in plan.scenes {
                            println!(
                                "{} | duration={}",
                                scene.id,
                                scene
                                    .duration_ms
                                    .map(|value| format!("{value}ms"))
                                    .unwrap_or_else(|| "untimed".to_string())
                            );
                            for shot in scene.shots {
                                println!(
                                    "  {:03} | {} | start={} | duration={} | speakers={} | sources={} | {}",
                                    shot.order,
                                    shot.id,
                                    shot.start_ms
                                        .map(|v| format!("{v}ms"))
                                        .unwrap_or_else(|| "untimed".to_string()),
                                    shot.duration_ms
                                        .map(|v| format!("{v}ms"))
                                        .unwrap_or_else(|| "untimed".to_string()),
                                    shot.speaker_ids.join(","),
                                    shot.source_refs.join(","),
                                    shot.action
                                );
                            }
                        }
                    }
                }
            } else {
                let loaded = reel::load_manifest(&manifest)?;
                let report = reel::validate_manifest(&loaded)?;
                match output {
                    OutputFormat::Json => {
                        println!("{}", serde_json::to_string_pretty(&report.exports)?)
                    }
                    OutputFormat::Text => {
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
                }
            }
        }
        Command::SeriesValidate { manifest, output } => {
            let report = reel::series::validate(&manifest)?;
            print_report(&report, output)?;
        }
        Command::SeriesPlan { manifest, output } => {
            let report = reel::series::plan(&manifest)?;
            print_report(&report, output)?;
        }
        Command::SeriesCoverage { manifest, output } => {
            let report = reel::series::coverage(&manifest)?;
            print_report(&report, output)?;
        }
        Command::SeriesReviewQueue { manifest, output } => {
            let report = reel::series::review_queue(&manifest)?;
            print_report(&report, output)?;
        }
        Command::EpisodeCompose {
            manifest,
            episode,
            output_dir,
            output,
        } => {
            let report = reel::series::compose_episode(&manifest, &episode, &output_dir)?;
            print_report(&report, output)?;
        }
        Command::CueImportSrt {
            manifest,
            captions,
            speaker,
            source_refs,
            mapping,
            output_path,
            output,
        } => {
            let report = reel::cue_import::import_srt(
                &manifest,
                &captions,
                speaker.as_deref(),
                &source_refs,
                mapping.as_deref(),
                &output_path,
            )?;
            print_report(&report, output)?;
        }
        Command::ContinuityValidate { registry, output } => {
            let report = reel::continuity::validate(&registry)?;
            print_report(&report, output)?;
        }
        Command::Conform {
            manifest,
            cues,
            output_dir,
            speaker_tempo,
            output,
        } => {
            let tempos = reel::production::parse_tempos(&speaker_tempo)?;
            let report = reel::production::conform(&manifest, &cues, &output_dir, &tempos)?;
            match output {
                OutputFormat::Text => println!(
                    "{} | work={} | duration={}ms | manifest={} | captions={} | lineage={}",
                    report.packet,
                    report.work,
                    report.duration_ms,
                    report.manifest,
                    report.captions,
                    report.lineage
                ),
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
            }
        }
        Command::CaptionExport {
            manifest,
            output_path,
        } => {
            println!(
                "{}",
                reel::production::caption_export(&manifest, &output_path)?.display()
            );
        }
        Command::Migrate {
            manifest,
            output_path,
            normalize_timing,
        } => {
            println!(
                "{}",
                reel::production::migrate(&manifest, &output_path, normalize_timing)?.display()
            );
        }
        Command::SourceCoverage { manifest, output } => {
            let report = reel::production::source_coverage(&manifest)?;
            match output {
                OutputFormat::Text => println!(
                    "{} | spoken={} | attributed={} | invented={} | unattributed={} | uncovered={} | complete={}",
                    report.work,
                    report.spoken_cues,
                    report.attributed_cues,
                    report.invented_cues.join(","),
                    report.unattributed_cues.join(","),
                    report
                        .uncovered_units
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(","),
                    report.complete
                ),
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
            }
        }
        Command::ProviderPackage {
            manifest,
            output_path,
            output,
        } => {
            let report = reel::production::write_provider_package(&manifest, &output_path)?;
            match output {
                OutputFormat::Text => println!(
                    "{} | work={} | assets={} | text_fields={} | blocked={}",
                    output_path.display(),
                    report.work,
                    report.requested_assets.len(),
                    report.outbound_text_fields.len(),
                    report.blocked
                ),
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
            }
        }
        Command::ReviewSelect { root, output } => {
            let report = reel::production::review_select(&root)?;
            match output {
                OutputFormat::Text => {
                    for (group, selection) in report.groups {
                        println!(
                            "{} | latest_candidate={} | candidates={} | approved={}",
                            group,
                            selection
                                .latest_review_candidate
                                .unwrap_or_else(|| "none".to_string()),
                            selection.candidates.len(),
                            selection.principal_approved.join(",")
                        );
                    }
                }
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
            }
        }
        Command::QualityCheck { manifest, output } => {
            let report = reel::production::quality_check(&manifest)?;
            match output {
                OutputFormat::Text => {
                    println!(
                        "{} | passed={} | warnings={} | narration_only={} | effects_music={}",
                        report.work,
                        report.passed,
                        report.warnings.len(),
                        report.narration_only_output,
                        report.effects_music_output
                    );
                    for warning in report.warnings {
                        println!(
                            "  {} | {} | {}",
                            warning.shot_id, warning.code, warning.message
                        );
                    }
                }
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
            }
        }
        Command::AnimaticRender {
            manifest,
            asset_root,
            audio,
            silent,
            narration_only_audio,
            effects_music_audio,
            captions,
            output_path,
            width,
            height,
            fps,
            transition_seconds,
            disclosure,
            motion_quality,
            motion_curve,
            dry_run,
            output,
        } => {
            let base_options = reel::adapters::still_animatic::AnimaticRenderOptions {
                manifest,
                asset_root,
                audio,
                silent,
                captions,
                output: output_path,
                width,
                height,
                fps,
                transition_seconds,
                disclosure,
                motion_quality,
                motion_curve,
                dry_run,
            };
            let requested = reel::production::load(&base_options.manifest)?
                .manifest
                .quality_controls
                .ab_outputs;
            if silent && !requested.is_empty() {
                anyhow::bail!(
                    "silent rendering cannot satisfy requested A/B audio outputs: {}",
                    requested.join(", ")
                );
            }
            for (label, selected_audio) in [
                ("narration-only", &narration_only_audio),
                ("effects-music", &effects_music_audio),
            ] {
                if requested.iter().any(|item| item == label) && selected_audio.is_none() {
                    anyhow::bail!("manifest requests {label} A/B output; provide --{label}-audio");
                }
            }
            let mut reports = vec![reel::adapters::still_animatic::render(&base_options)?];
            for (label, selected_audio) in [
                ("narration-only", narration_only_audio),
                ("effects-music", effects_music_audio),
            ] {
                if requested.iter().any(|item| item == label) {
                    let selected_audio = selected_audio.expect("A/B audio preflighted");
                    let mut variant = base_options.clone();
                    variant.audio = Some(selected_audio);
                    variant.silent = false;
                    variant.output =
                        reel::adapters::still_animatic::variant_output(&base_options.output, label);
                    reports.push(reel::adapters::still_animatic::render(&variant)?);
                }
            }
            match output {
                OutputFormat::Text => {
                    for report in reports {
                        println!(
                            "{} | work={} | duration={}ms | {}x{}@{} | dry_run={} | artifacts={}",
                            report.output,
                            report.work,
                            report.duration_ms,
                            report.width,
                            report.height,
                            report.fps,
                            report.dry_run,
                            report.artifact_manifest
                        );
                    }
                }
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&reports)?),
            }
        }
        Command::MotionAnalyze { video, output } => {
            let report = reel::adapters::still_animatic::analyze_motion(&video)?;
            match output {
                OutputFormat::Text => println!(
                    "{} | transitions={} | near_stationary={} | fraction={:.4} | maximum={:.4} | passed={}",
                    report.input,
                    report.frame_transitions,
                    report.near_stationary_transitions,
                    report.near_stationary_fraction,
                    report.maximum_near_stationary_fraction,
                    report.passed
                ),
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
            }
            if !report.passed {
                anyhow::bail!(
                    "motion cadence failed: near-stationary fraction {:.4} exceeds {:.4}",
                    report.near_stationary_fraction,
                    report.maximum_near_stationary_fraction
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
            ensure_legacy_render_compatible(&manifest)?;
            let video = reel::render_scene_preview(&manifest, &scene, &platform)?;
            println!("{}", video.display());
        }
        Command::ScenePreviews { manifest, platform } => {
            ensure_legacy_render_compatible(&manifest)?;
            for video in reel::render_scene_previews(&manifest, &platform)? {
                println!("{}", video.display());
            }
        }
        Command::WorkPreview { manifest, platform } => {
            ensure_legacy_render_compatible(&manifest)?;
            let video = reel::render_work_preview(&manifest, &platform)?;
            println!("{}", video.display());
        }
        Command::ArtifactManifest { manifest, output } => {
            ensure_legacy_render_compatible(&manifest)?;
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
            ensure_legacy_render_compatible(&manifest)?;
            let sheet = reel::render_contact_sheet(&manifest, &platform)?;
            println!("{}", sheet.display());
        }
        Command::ShotCards { manifest, platform } => {
            ensure_legacy_render_compatible(&manifest)?;
            let video = reel::render_shot_cards(&manifest, &platform)?;
            println!("{}", video.display());
        }
        Command::Smoke { manifest } => {
            ensure_legacy_render_compatible(&manifest)?;
            let video = reel::render_smoke(&manifest)?;
            println!("{}", video.display());
        }
        Command::ReviewPack { manifest } => {
            ensure_legacy_render_compatible(&manifest)?;
            let report = reel::render_review_pack(&manifest)?;
            println!("{}", report.display());
        }
        Command::Demo { manifest } => {
            ensure_legacy_render_compatible(&manifest)?;
            let demo = reel::render_demo(&manifest)?;
            println!("{}", demo.display());
        }
        Command::RemotionPack {
            manifest,
            platform,
            scene,
        } => {
            ensure_legacy_render_compatible(&manifest)?;
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

fn ensure_legacy_render_compatible(manifest: &std::path::Path) -> Result<()> {
    if reel::production::is_production_manifest(manifest)? {
        reel::production::require_preview_ready(manifest)?;
        anyhow::bail!(
            "production v0.2 manifests use `animatic-render`; legacy card/preview renderers accept v0.1 manifests"
        );
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
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Print renderer-neutral export plans derived from a manifest.
    Plan {
        #[arg(default_value = "works/0001-ash-vale-last-road-before-winter/manifest.yaml")]
        manifest: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Validate an episodic-series index and all referenced child manifests.
    SeriesValidate {
        manifest: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Print deterministic season, episode, child, timing, and runtime order.
    SeriesPlan {
        manifest: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Report continuous canonical source coverage across a series.
    SeriesCoverage {
        manifest: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Report open human review and release-blocked episodes.
    SeriesReviewQueue {
        manifest: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Atomically compose referenced conformed scene packets into an episode packet.
    EpisodeCompose {
        manifest: PathBuf,
        episode: String,
        #[arg(long)]
        output_dir: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Import millisecond SRT captions as source- and speaker-assigned narration cues.
    CueImportSrt {
        manifest: PathBuf,
        captions: PathBuf,
        #[arg(long)]
        speaker: Option<String>,
        #[arg(long = "source-ref")]
        source_refs: Vec<String>,
        #[arg(long)]
        mapping: Option<PathBuf>,
        #[arg(long = "output")]
        output_path: PathBuf,
        #[arg(long = "format", value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Validate a shared, versioned continuity registry without exposing local assets.
    ContinuityValidate {
        registry: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Atomically conform an untimed/guide production manifest from measured narration cues.
    Conform {
        manifest: PathBuf,
        #[arg(long)]
        cues: PathBuf,
        #[arg(long)]
        output_dir: PathBuf,
        #[arg(long = "speaker-tempo", value_name = "SPEAKER=PERCENT")]
        speaker_tempo: Vec<String>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Export SRT captions from a conformed speaker-aware cue timeline.
    CaptionExport {
        manifest: PathBuf,
        #[arg(long = "output")]
        output_path: PathBuf,
    },
    /// Migrate a legacy manifest into the additive v0.2 production contract.
    Migrate {
        manifest: PathBuf,
        #[arg(long = "output")]
        output_path: PathBuf,
        #[arg(long)]
        normalize_timing: bool,
    },
    /// Report selected, omitted, invented, and unattributed source coverage.
    SourceCoverage {
        manifest: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Write a privacy-audited provider egress package without local paths.
    ProviderPackage {
        manifest: PathBuf,
        #[arg(long = "output")]
        output_path: PathBuf,
        #[arg(long = "format", value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Identify latest review candidates without inferring principal approval.
    ReviewSelect {
        #[arg(default_value = "works")]
        root: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Check long-still motion, focal, crop, and continuity quality controls.
    QualityCheck {
        manifest: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Render an asset-backed still-image animatic through FFmpeg with captions and provenance.
    AnimaticRender {
        manifest: PathBuf,
        #[arg(long)]
        asset_root: PathBuf,
        #[arg(long, required_unless_present = "silent", conflicts_with = "silent")]
        audio: Option<PathBuf>,
        /// Render without an audio stream for sound-optional delivery.
        #[arg(long, conflicts_with = "audio")]
        silent: bool,
        #[arg(long)]
        narration_only_audio: Option<PathBuf>,
        #[arg(long)]
        effects_music_audio: Option<PathBuf>,
        #[arg(long)]
        captions: PathBuf,
        #[arg(long = "output")]
        output_path: PathBuf,
        #[arg(long, default_value_t = 1280)]
        width: u32,
        #[arg(long, default_value_t = 720)]
        height: u32,
        #[arg(long, default_value_t = 24)]
        fps: u32,
        #[arg(long, default_value_t = 0.8)]
        transition_seconds: f64,
        #[arg(long, default_value = "ILLUSTRATED RECONSTRUCTION - PRIVATE REVIEW")]
        disclosure: String,
        /// Select smooth subpixel motion (default) or deterministic v0.2.1 zoompan reproduction.
        #[arg(long, value_enum, default_value_t = reel::adapters::still_animatic::MotionQuality::Smooth)]
        motion_quality: reel::adapters::still_animatic::MotionQuality,
        /// Select the motion progress curve used by the smooth backend.
        #[arg(long, value_enum, default_value_t = reel::adapters::still_animatic::MotionCurve::EaseInOut)]
        motion_curve: reel::adapters::still_animatic::MotionCurve,
        #[arg(long)]
        dry_run: bool,
        #[arg(long = "format", value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Measure near-stationary adjacent-frame cadence in a rendered moving shot.
    MotionAnalyze {
        video: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
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
