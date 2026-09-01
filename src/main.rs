use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;

fn print_report(report: &impl Serialize, output: OutputFormat) -> Result<()> {
    match output {
        OutputFormat::Text | OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(report)?)
        }
    }
    Ok(())
}

fn run_animatic_receipt(
    artifact_manifest: &PathBuf,
    output_path: &PathBuf,
    output: OutputFormat,
) -> Result<()> {
    let receipt =
        reel::adapters::still_animatic::write_animatic_receipt(artifact_manifest, output_path)?;
    match output {
        OutputFormat::Text => println!(
            "{} | source={} | output={} | {}x{}@{} | duration={}ms | verified={}",
            output_path.display(),
            receipt.source_artifact_sha256,
            receipt.output_sha256,
            receipt.width,
            receipt.height,
            receipt.fps,
            receipt.duration_ms,
            receipt.verified
        ),
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&receipt)?),
    }
    Ok(())
}

fn run_animatic_receipt_check(
    receipt: &PathBuf,
    video: &PathBuf,
    output: OutputFormat,
) -> Result<()> {
    let report = reel::adapters::still_animatic::check_animatic_receipt(receipt, video)?;
    print_report(&report, output)
}

fn main() -> Result<()> {
    std::thread::Builder::new()
        .name("reel-cli".to_string())
        .stack_size(8 * 1024 * 1024)
        .spawn(run_cli)
        .context("failed to start REEL CLI worker")?
        .join()
        .map_err(|_| anyhow!("REEL CLI worker panicked"))?
}

fn run_cli() -> Result<()> {
    let cli = Cli::parse();

    match *cli.command {
        Command::Validate { manifest, output } => {
            if reel::production::is_production_manifest(&manifest)? {
                let loaded = reel::production::load(&manifest)?;
                let report = reel::production::validate(&loaded)?;
                match output {
                    OutputFormat::Text => println!(
                        "manifest ok: {} version={} profile={} timing={} scenes={} shots={} stills={} videos={} audio_events={} beats={} ducking={} mastering={} speakers={} cues={} duration={} timing_ready={} generation_ready={} asset_ready={} preview_ready={} delivery_ready={} blockers={} gated={}",
                        report.manifest,
                        report.version,
                        report.profile,
                        report.timing_status,
                        report.scenes,
                        report.shots,
                        report.still_events,
                        report.video_events,
                        report.audio_events,
                        report.beat_markers,
                        report.narration_ducking,
                        report.audio_mastering,
                        report.speakers,
                        report.narration_cues,
                        report
                            .duration_ms
                            .map(|value| format!("{value}ms"))
                            .unwrap_or_else(|| "untimed".to_string()),
                        report.timing_ready,
                        report.generation_ready,
                        report.asset_ready,
                        report.preview_ready,
                        report.delivery_ready,
                        report.semantic_blockers.join("|"),
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
        Command::SeriesTimingAudit {
            manifest,
            neighbor_drift_percent,
            output,
        } => {
            let report = reel::series::timing_audit(&manifest, neighbor_drift_percent)?;
            print_report(&report, output)?;
        }
        Command::SeriesCoverage { manifest, output } => {
            let report = reel::series::coverage(&manifest)?;
            print_report(&report, output)?;
        }
        Command::SeriesReviewQueue {
            manifest,
            decision_index,
            output,
        } => {
            let report =
                reel::series::review_queue_with_decisions(&manifest, decision_index.as_deref())?;
            print_report(&report, output)?;
        }
        Command::ShowrunnerValidate { plan, output } => {
            let report = reel::showrunner::validate(&plan)?;
            match output {
                OutputFormat::Text => print!("{}", reel::showrunner::validation_markdown(&report)),
                OutputFormat::Json => print_report(&report, output)?,
            }
        }
        Command::ShowrunnerAudit { plan, output } => {
            let report = reel::showrunner::audit(&plan)?;
            match output {
                OutputFormat::Text => print!("{}", reel::showrunner::audit_markdown(&report)),
                OutputFormat::Json => print_report(&report, output)?,
            }
        }
        Command::ShowrunnerRevelationMap { plan, output } => {
            let report = reel::showrunner::revelation_map(&plan)?;
            match output {
                OutputFormat::Text => print!("{}", reel::showrunner::revelation_markdown(&report)),
                OutputFormat::Json => print_report(&report, output)?,
            }
        }
        Command::ShowrunnerRhythmAudit { plan, output } => {
            let report = reel::showrunner::rhythm_audit(&plan)?;
            match output {
                OutputFormat::Text => print!("{}", reel::showrunner::rhythm_markdown(&report)),
                OutputFormat::Json => print_report(&report, output)?,
            }
        }
        Command::ShowrunnerReviewQueue { plan, output } => {
            let report = reel::showrunner::review_queue(&plan)?;
            match output {
                OutputFormat::Text => {
                    print!("{}", reel::showrunner::review_queue_markdown(&report))
                }
                OutputFormat::Json => print_report(&report, output)?,
            }
        }
        Command::ShowrunnerReviewPack {
            plan,
            output,
            output_path,
        } => {
            let report = reel::showrunner::review_pack(&plan)?;
            let rendered = match output {
                OutputFormat::Text => reel::showrunner::review_pack_markdown(&report),
                OutputFormat::Json => serde_json::to_string_pretty(&report)?,
            };
            if let Some(path) = output_path {
                std::fs::write(&path, rendered)
                    .with_context(|| format!("failed to write {}", path.display()))?;
                println!("{}", path.display());
            } else {
                print!("{rendered}");
            }
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
        Command::CaptionCheck {
            captions,
            max_chars_per_line,
            max_lines_per_cue,
            max_reading_speed_cps,
            min_duration_ms,
            output,
        } => {
            let report = reel::caption::check(
                &captions,
                reel::caption::CaptionThresholds {
                    max_chars_per_line,
                    max_lines_per_cue,
                    max_reading_speed_cps,
                    min_duration_ms,
                },
            )?;
            print_report(&report, output)?;
            if !report.passed {
                return Err(anyhow!(
                    "caption accessibility check failed with {} violation(s)",
                    report.violations.len()
                ));
            }
        }
        Command::AudioCheck {
            audio,
            narration_stem,
            effects_music_stem,
            manifest,
            profile,
            report,
            output,
        } => {
            let checked = reel::audio_quality::check(reel::audio_quality::AudioCheckOptions {
                audio: &audio,
                narration_stem: narration_stem.as_deref(),
                effects_music_stem: effects_music_stem.as_deref(),
                manifest: manifest.as_deref(),
                profile,
            })?;
            if let Some(path) = report {
                reel::audio_quality::write_report(&path, &checked)?;
            }
            print_report(&checked, output)?;
            if !checked.passed {
                anyhow::bail!(
                    "audio quality check failed with {} violation(s)",
                    checked.violations.len()
                );
            }
        }
        Command::MusicSourceValidate { source, output } => {
            let report = reel_music::source::validate(&source)?;
            print_report(&report, output)?;
        }
        Command::MusicNeutralPlan {
            source,
            output_path,
            output,
        } => {
            let report = reel_music::neutral::write_plan(&source, &output_path)?;
            print_report(&report, output)?;
        }
        Command::MusicNeutralCheck {
            plan,
            source,
            candidate_pcm,
            output,
        } => {
            let report = reel_music::neutral::check(&plan, &source, &candidate_pcm)?;
            print_report(&report, output)?;
        }
        Command::MusicRepairPlan { repair, output } => {
            let report = reel_music::repair::validate(&repair)?;
            print_report(&report, output)?;
        }
        Command::MusicRepairCompile {
            repair,
            output_path,
            output,
        } => {
            let report = reel_music::edl::write(&repair, &output_path)?;
            print_report(&report, output)?;
        }
        Command::MusicRepairRender {
            edl,
            repair,
            output_pcm,
            evidence_path,
            output,
        } => {
            let report = reel::music_render::render(&edl, &repair, &output_pcm, &evidence_path)?;
            print_report(&report, output)?;
        }
        Command::MusicRepairEvidenceCheck {
            evidence,
            edl,
            repair,
            candidate_pcm,
            output,
        } => {
            let report = reel::music_render::check(&evidence, &edl, &repair, &candidate_pcm)?;
            print_report(&report, output)?;
        }
        Command::MusicAnalysisValidate { analysis, output } => {
            let report = reel_music::analysis::validate(&analysis)?;
            print_report(&report, output)?;
        }
        Command::MusicModelValidate { model, output } => {
            let report = reel_music::model::validate(&model)?;
            print_report(&report, output)?;
        }
        Command::SongValidate { manifest, output } => {
            let report = reel::song::validate(&manifest)?;
            print_report(&report, output)?;
        }
        Command::SongEnginePlan {
            manifest,
            output_dir,
            output,
        } => {
            let report = reel::song::write_plan(&manifest, &output_dir)?;
            print_report(&report, output)?;
        }
        Command::SongEnginePlanCheck {
            packet_dir,
            manifest,
            output,
        } => {
            let report = reel::song::check(&packet_dir, &manifest)?;
            print_report(&report, output)?;
        }
        Command::SongEngineDoctor { manifest, output } => {
            let report = reel::song::doctor(&manifest)?;
            let ready = report.ready;
            print_report(&report, output)?;
            if !ready {
                anyhow::bail!("local song engine is not ready");
            }
        }
        Command::VoicePerformancePlan {
            manifest,
            performance,
            engine,
            engine_version,
            reference_audio,
            seed,
            output_dir,
            output,
        } => {
            let report = reel::voice_performance::write_plan(reel::voice_performance::Options {
                manifest: &manifest,
                performance: &performance,
                engine,
                engine_version: &engine_version,
                reference_audio: reference_audio.as_deref(),
                seed,
                output_dir: &output_dir,
            })?;
            print_report(&report, output)?;
        }
        Command::VoicePerformancePlanCheck {
            packet_dir,
            manifest,
            performance,
            reference_audio,
            output,
        } => {
            let report = reel::voice_performance::check(
                &packet_dir,
                &manifest,
                &performance,
                reference_audio.as_deref(),
            )?;
            print_report(&report, output)?;
        }
        Command::VoiceProsodyEvidence {
            packet_dir,
            measurements,
            rendered_audio,
            output_dir,
            output,
        } => {
            let report = reel::voice_performance::write_prosody_evidence(
                reel::voice_performance::ProsodyOptions {
                    packet_dir: &packet_dir,
                    measurements: &measurements,
                    rendered_audio: &rendered_audio,
                    output_dir: &output_dir,
                },
            )?;
            print_report(&report, output)?;
        }
        Command::VoiceProsodyEvidenceCheck {
            evidence_dir,
            packet_dir,
            measurements,
            rendered_audio,
            output,
        } => {
            let report = reel::voice_performance::check_prosody_evidence(
                &evidence_dir,
                &packet_dir,
                &measurements,
                &rendered_audio,
            )?;
            print_report(&report, output)?;
        }
        Command::VoiceConsistencyCheck {
            manifest,
            profile,
            measurements,
            report,
            output,
        } => {
            let checked = reel::voice_consistency::check(&manifest, &profile, &measurements)?;
            if let Some(path) = report {
                reel::voice_consistency::write_report(&path, &checked)?;
            }
            print_report(&checked, output)?;
            if !checked.passed {
                anyhow::bail!(
                    "voice consistency check failed with {} violation(s)",
                    checked.violations.len()
                );
            }
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
        Command::RenderDoctor { output } => {
            let report = reel::adapters::ffmpeg::FfmpegAdapter.render_environment()?;
            match output {
                OutputFormat::Text => {
                    println!(
                        "render environment | transport={} | ffmpeg={} | ffprobe={} | passed={}",
                        report.transport,
                        report.ffmpeg_version,
                        report.ffprobe_version,
                        report.passed
                    );
                    for check in &report.checks {
                        println!(
                            "  {} | available={} | {}",
                            check.id, check.available, check.evidence
                        );
                    }
                }
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
            }
            if !report.passed {
                anyhow::bail!(
                    "render environment is missing required capabilities: {}",
                    report.missing().join(", ")
                );
            }
        }
        Command::AnimaticRender {
            manifest,
            asset_root,
            audio,
            audio_check_report,
            silent,
            narration_only_audio,
            effects_music_audio,
            captions,
            no_captions: _,
            caption_options,
            output_path,
            width,
            height,
            fps,
            edit_mode,
            transition_seconds,
            disclosure,
            motion_quality,
            motion_curve,
            encoding_preset,
            dry_run,
            output,
        } => {
            let effective_transition_seconds = match edit_mode {
                reel::adapters::still_animatic::EditMode::Cinematic => transition_seconds,
                reel::adapters::still_animatic::EditMode::Montage => 0.0,
            };
            let base_options = reel::adapters::still_animatic::AnimaticRenderOptions {
                manifest,
                asset_root,
                audio,
                audio_check_report,
                silent,
                captions,
                caption_presentation: caption_options.caption_presentation,
                caption_profile: caption_options.caption_profile,
                speaker_label_policy: caption_options.speaker_label_policy,
                speaker_reintroduce_after_ms: caption_options.speaker_reintroduce_after_ms,
                caption_thresholds: reel::caption::CaptionThresholds {
                    max_chars_per_line: caption_options.max_caption_chars_per_line,
                    max_lines_per_cue: caption_options.max_caption_lines_per_cue,
                    max_reading_speed_cps: caption_options.max_caption_reading_speed_cps,
                    min_duration_ms: caption_options.min_caption_duration_ms,
                },
                caption_policy_note: caption_options.caption_policy_note,
                output: output_path,
                width,
                height,
                fps,
                transition_seconds: effective_transition_seconds,
                disclosure,
                motion_quality,
                motion_curve,
                encoding_preset,
                dry_run,
            };
            let requested_manifest = reel::production::load(&base_options.manifest)?.manifest;
            let requested = requested_manifest.quality_controls.ab_outputs;
            let has_manifest_audio = !requested_manifest.audio_events.is_empty();
            if has_manifest_audio && !requested.is_empty() {
                anyhow::bail!(
                    "manifest audio_events cannot be combined with pre-mixed A/B audio outputs"
                );
            }
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
        Command::MotionCheck {
            manifest,
            video,
            output,
        } => {
            let report = reel::adapters::still_animatic::check_motion(&manifest, &video)?;
            match output {
                OutputFormat::Text => {
                    println!(
                        "{} | shots={} | passed={}",
                        report.video,
                        report.shots.len(),
                        report.passed
                    );
                    for shot in &report.shots {
                        println!(
                            "  {} | {} | {} | stationary={:.4} | passed={}",
                            shot.shot_id,
                            shot.treatment,
                            shot.expectation,
                            shot.near_stationary_fraction,
                            shot.passed
                        );
                    }
                }
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
            }
            if !report.passed {
                anyhow::bail!("manifest-aware motion check failed");
            }
        }
        Command::AnimaticCheck {
            artifact_manifest,
            output,
        } => {
            let report = reel::adapters::still_animatic::check_animatic(&artifact_manifest)?;
            print_report(&report, output)?;
        }
        Command::AnimaticAudioRender {
            manifest,
            asset_root,
            output_path,
            dry_run,
            output,
        } => {
            let report = reel::audio_preview::render_audio_preview(
                &reel::audio_preview::AudioPreviewOptions {
                    manifest,
                    asset_root,
                    output: output_path,
                    dry_run,
                },
            )?;
            print_report(&report, output)?;
        }
        Command::AnimaticAudioCheck {
            artifact_report,
            output,
        } => {
            let report = reel::audio_preview::check_audio_preview(&artifact_report)?;
            print_report(&report, output)?;
        }
        Command::AnimaticRemux {
            picture_artifact,
            audio_artifact,
            output_path,
            output,
        } => {
            let report = reel::audio_preview::remux_picture(
                &picture_artifact,
                &audio_artifact,
                &output_path,
            )?;
            print_report(&report, output)?;
        }
        Command::AnimaticRemuxCheck {
            artifact_report,
            output,
        } => {
            let report = reel::audio_preview::check_picture_remux(&artifact_report)?;
            print_report(&report, output)?;
        }
        Command::AnimaticLock {
            artifact_manifest,
            output_dir,
            output,
        } => {
            let report = reel::selection_lock::lock_selection(&artifact_manifest, &output_dir)?;
            print_report(&report, output)?;
        }
        Command::AnimaticLockCheck { packet, output } => {
            let report = reel::selection_lock::check_selection_lock(&packet)?;
            print_report(&report, output)?;
        }
        Command::PlanningDerive {
            locked_manifest,
            output_path,
            reason,
            changed_dimensions,
            output,
        } => {
            let report = reel::selection_lock::derive_planning_manifest(
                &locked_manifest,
                &output_path,
                &reason,
                &changed_dimensions,
            )?;
            print_report(&report, output)?;
        }
        Command::CaptionLayout {
            artifact_manifest,
            output_dir,
            output,
        } => {
            let report = reel::caption_layout::write_packet(&artifact_manifest, &output_dir)?;
            print_report(&report, output)?;
        }
        Command::ComparisonCompose {
            contract,
            output_path,
            output,
        } => {
            let report = reel::comparison::compose(&contract, &output_path)?;
            print_report(&report, output)?;
        }
        Command::ComparisonReceiptCheck {
            receipt,
            video,
            output,
        } => {
            let report = reel::comparison::check_receipt(&receipt, &video)?;
            print_report(&report, output)?;
        }
        Command::ComparisonLayout {
            artifact,
            output_dir,
            output,
        } => {
            let report = reel::comparison::write_layout_packet(&artifact, &output_dir)?;
            print_report(&report, output)?;
        }
        Command::ComparisonLayoutCheck { packet_dir, output } => {
            let report = reel::comparison::check_layout_packet(&packet_dir)?;
            print_report(&report, output)?;
        }
        Command::ReviewRecord {
            target,
            finding,
            output_path,
            output,
        } => {
            let report = reel::review_decision::write_record(&target, &finding, &output_path)?;
            print_report(&report, output)?;
        }
        Command::AnimaticReceipt {
            artifact_manifest,
            output_path,
            output,
        } => {
            run_animatic_receipt(&artifact_manifest, &output_path, output)?;
        }
        Command::AnimaticReceiptCheck {
            receipt,
            video,
            output,
        } => {
            run_animatic_receipt_check(&receipt, &video, output)?;
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
    command: Box<Command>,
}

#[derive(Debug, Args)]
struct AnimaticCaptionArgs {
    /// Strict renderer-neutral cue/speaker mapping used for visible speaker badges.
    #[arg(long)]
    caption_presentation: Option<PathBuf>,
    /// Select deterministic caption and speaker-badge geometry.
    #[arg(long, value_enum, default_value_t = reel::caption_presentation::CaptionProfile::PrivateReview)]
    caption_profile: reel::caption_presentation::CaptionProfile,
    /// Select when explicit audience-facing speaker labels are shown.
    #[arg(long, value_enum, default_value_t = reel::caption_presentation::SpeakerLabelPolicy::None)]
    speaker_label_policy: reel::caption_presentation::SpeakerLabelPolicy,
    #[arg(long)]
    speaker_reintroduce_after_ms: Option<u64>,
    #[arg(long, default_value_t = 42)]
    max_caption_chars_per_line: usize,
    #[arg(long, default_value_t = 2)]
    max_caption_lines_per_cue: usize,
    #[arg(long, default_value_t = 20.0)]
    max_caption_reading_speed_cps: f64,
    #[arg(long, default_value_t = 1_000)]
    min_caption_duration_ms: u64,
    /// Required note when overriding the default caption accessibility policy.
    #[arg(long)]
    caption_policy_note: Option<String>,
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
    /// Compare planned episode and season runtime ranges with available timing.
    SeriesTimingAudit {
        manifest: PathBuf,
        #[arg(long, default_value_t = 35.0)]
        neighbor_drift_percent: f64,
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
        #[arg(long)]
        decision_index: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Validate a series-bound showrunner control sidecar.
    ShowrunnerValidate {
        plan: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Run combined function, rhythm, viewpoint, revelation, and scale audits.
    ShowrunnerAudit {
        plan: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Print ordered audience-revelation threads and viewpoint findings.
    ShowrunnerRevelationMap {
        plan: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Audit episode function, tone, intensity, transition, and production-scale cadence.
    ShowrunnerRhythmAudit {
        plan: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Report distinct open human reviewers without synthesizing their opinions.
    ShowrunnerReviewQueue {
        plan: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Print one combined machine-audit and human-review packet.
    ShowrunnerReviewPack {
        plan: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
        #[arg(long)]
        output_path: Option<PathBuf>,
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
    /// Audit SRT readability and emit no caption text or local paths.
    CaptionCheck {
        captions: PathBuf,
        #[arg(long, default_value_t = 42)]
        max_chars_per_line: usize,
        #[arg(long, default_value_t = 2)]
        max_lines_per_cue: usize,
        #[arg(long, default_value_t = 20.0)]
        max_reading_speed_cps: f64,
        #[arg(long, default_value_t = 1_000)]
        min_duration_ms: u64,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Audit local audio loudness, peaks, streams, duration, silence, and optional stems.
    AudioCheck {
        audio: PathBuf,
        #[arg(long, requires = "effects_music_stem")]
        narration_stem: Option<PathBuf>,
        #[arg(long, requires = "narration_stem")]
        effects_music_stem: Option<PathBuf>,
        #[arg(long)]
        manifest: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = reel::audio_quality::AudioProfile::PrivateReview)]
        profile: reel::audio_quality::AudioProfile,
        /// Atomically retain the strict path-free JSON report for later artifact binding.
        #[arg(long)]
        report: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Validate an immutable raw-PCM music source and its authority boundary.
    MusicSourceValidate {
        source: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Write a deterministic full-range keep/lock plan for neutral reconstruction.
    MusicNeutralPlan {
        source: PathBuf,
        #[arg(long)]
        output_path: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Verify a neutral plan and candidate PCM against the exact source signal.
    MusicNeutralCheck {
        plan: PathBuf,
        source: PathBuf,
        candidate_pcm: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Validate a deterministic music-repair plan and complete changed/locked coverage.
    MusicRepairPlan {
        repair: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Compile a validated cut-only repair into a canonical, sample-exact EDL.
    MusicRepairCompile {
        repair: PathBuf,
        #[arg(long)]
        output_path: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Render a cut-only EDL to raw PCM and write strict local seam evidence.
    MusicRepairRender {
        edl: PathBuf,
        repair: PathBuf,
        #[arg(long)]
        output_pcm: PathBuf,
        #[arg(long)]
        evidence_path: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Recompute and verify retained evidence for a rendered music repair.
    MusicRepairEvidenceCheck {
        evidence: PathBuf,
        edl: PathBuf,
        repair: PathBuf,
        candidate_pcm: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Validate immutable external analyzer evidence without promoting estimates.
    MusicAnalysisValidate {
        analysis: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Validate a corrected editable music model and its event-level provenance.
    MusicModelValidate {
        model: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Validate an exact-lyrics, rights-gated local song-generation contract.
    SongValidate {
        manifest: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Compile a private local-engine request and a path-free, lyric-free receipt.
    SongEnginePlan {
        manifest: PathBuf,
        #[arg(long)]
        output_dir: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Re-verify a song-engine packet against its manifest, lyrics, and references.
    SongEnginePlanCheck {
        packet_dir: PathBuf,
        manifest: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Diagnose the declared local song engine without downloading or generating.
    SongEngineDoctor {
        manifest: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Validate and compile exact cue spans into an auditable engine performance plan.
    VoicePerformancePlan {
        manifest: PathBuf,
        performance: PathBuf,
        #[arg(long, value_enum)]
        engine: reel::voice_performance::VoiceEngine,
        #[arg(long)]
        engine_version: String,
        #[arg(long)]
        reference_audio: Option<PathBuf>,
        #[arg(long, default_value_t = 0)]
        seed: u64,
        #[arg(long)]
        output_dir: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Re-verify a performance plan against its manifest, sidecar, and reference audio.
    VoicePerformancePlanCheck {
        packet_dir: PathBuf,
        manifest: PathBuf,
        performance: PathBuf,
        #[arg(long)]
        reference_audio: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Bind measured rendered-audio prosody to a performance plan and evaluate contour intent.
    VoiceProsodyEvidence {
        packet_dir: PathBuf,
        measurements: PathBuf,
        rendered_audio: PathBuf,
        #[arg(long)]
        output_dir: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Recompute and verify a prosody-evidence packet against its bound inputs.
    VoiceProsodyEvidenceCheck {
        evidence_dir: PathBuf,
        packet_dir: PathBuf,
        measurements: PathBuf,
        rendered_audio: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Gate auditions or complete scenes against approved cross-scene voice identities, pace, and pauses.
    VoiceConsistencyCheck {
        manifest: PathBuf,
        profile: PathBuf,
        measurements: PathBuf,
        /// Atomically retain the strict path-free JSON report for artifact binding.
        #[arg(long)]
        report: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
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
    /// Verify FFmpeg/ffprobe and every capability required by the animatic pipeline.
    RenderDoctor {
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Render a manifest-owned still/video and audio-event timeline through FFmpeg.
    AnimaticRender {
        manifest: PathBuf,
        #[arg(long)]
        asset_root: PathBuf,
        /// Use a pre-mixed master instead of manifest audio_events.
        #[arg(long, conflicts_with = "silent")]
        audio: Option<PathBuf>,
        /// Bind a successful path-free audio-check report to artifact lineage.
        #[arg(long, requires = "audio")]
        audio_check_report: Option<PathBuf>,
        /// Render without an audio stream; conflicts with a manifest that owns audio_events.
        #[arg(long, conflicts_with = "audio")]
        silent: bool,
        #[arg(long)]
        narration_only_audio: Option<PathBuf>,
        #[arg(long)]
        effects_music_audio: Option<PathBuf>,
        #[arg(
            long,
            required_unless_present = "no_captions",
            conflicts_with = "no_captions"
        )]
        captions: Option<PathBuf>,
        /// Render without burned-in captions or speaker badges.
        #[arg(long)]
        no_captions: bool,
        #[command(flatten)]
        caption_options: Box<AnimaticCaptionArgs>,
        #[arg(long = "output")]
        output_path: PathBuf,
        #[arg(long, default_value_t = 1280)]
        width: u32,
        #[arg(long, default_value_t = 720)]
        height: u32,
        #[arg(long, default_value_t = 24)]
        fps: u32,
        /// Select the default cinematic dissolve language or hard-cut montage assembly.
        #[arg(long, value_enum, default_value_t = reel::adapters::still_animatic::EditMode::Cinematic)]
        edit_mode: reel::adapters::still_animatic::EditMode,
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
        /// Select the H.264 encoding speed/quality preset.
        #[arg(long, value_enum, default_value_t = reel::adapters::still_animatic::EncodingPreset::Slow)]
        encoding_preset: reel::adapters::still_animatic::EncodingPreset,
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
    /// Verify moving shots and intentional holds against a manifest timeline.
    MotionCheck {
        manifest: PathBuf,
        video: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Verify a rendered animatic, its inputs, streams, captions, and artifact lineage.
    AnimaticCheck {
        artifact_manifest: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Render only manifest-owned audio events for rapid mix review.
    AnimaticAudioRender {
        manifest: PathBuf,
        #[arg(long)]
        asset_root: PathBuf,
        #[arg(long = "output")]
        output_path: PathBuf,
        #[arg(long)]
        dry_run: bool,
        #[arg(long = "format", value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Verify an audio-only preview and its manifest/source lineage.
    AnimaticAudioCheck {
        artifact_report: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Reuse verified picture while replacing its audio with a verified audio preview.
    AnimaticRemux {
        picture_artifact: PathBuf,
        audio_artifact: PathBuf,
        #[arg(long = "output")]
        output_path: PathBuf,
        #[arg(long = "format", value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Re-verify a cached-picture remux and both source artifact lineages.
    AnimaticRemuxCheck {
        artifact_report: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Verify a selected proof and atomically create a receipt-bound locked manifest packet.
    AnimaticLock {
        artifact_manifest: PathBuf,
        #[arg(long)]
        output_dir: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Re-verify a selection lock packet, its source artifact, and selected output.
    AnimaticLockCheck {
        packet: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Create an unlocked, lineage-bearing planning derivative from a locked manifest.
    PlanningDerive {
        locked_manifest: PathBuf,
        #[arg(long = "output")]
        output_path: PathBuf,
        #[arg(long)]
        reason: String,
        #[arg(long = "changed-dimension", required = true)]
        changed_dimensions: Vec<String>,
        #[arg(long = "format", value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Write artifact-bound caption-region geometry and representative-frame evidence.
    CaptionLayout {
        artifact_manifest: PathBuf,
        #[arg(long)]
        output_dir: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Compose verified variants under a strict one-dimension comparison contract.
    ComparisonCompose {
        contract: PathBuf,
        #[arg(long = "output")]
        output_path: PathBuf,
        #[arg(long = "format", value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Verify a path-free comparison receipt against the intentionally shared video.
    ComparisonReceiptCheck {
        receipt: PathBuf,
        video: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Write a private artifact-bound packet of comparison-slate geometry and frames.
    ComparisonLayout {
        artifact: PathBuf,
        #[arg(long)]
        output_dir: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Verify a private comparison-layout packet, its artifact, video, and images.
    ComparisonLayoutCheck {
        packet_dir: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Append an independent private human finding bound to an exact artifact hash.
    ReviewRecord {
        target: PathBuf,
        finding: PathBuf,
        #[arg(long = "output")]
        output_path: PathBuf,
        #[arg(long = "format", value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Verify locally and write a path-free receipt safe for intentional sharing.
    AnimaticReceipt {
        artifact_manifest: PathBuf,
        #[arg(long = "output")]
        output_path: PathBuf,
        #[arg(long = "format", value_enum, default_value_t = OutputFormat::Text)]
        output: OutputFormat,
    },
    /// Verify a strict path-free receipt against the intentionally shared video.
    AnimaticReceiptCheck {
        receipt: PathBuf,
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
