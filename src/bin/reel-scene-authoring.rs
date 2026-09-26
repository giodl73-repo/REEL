use anyhow::{Context, Result, bail};
use reel::scene_authoring_inputs::read_verified_alignments;
use reel_assembly::scene_authoring::{
    Episode, RenderedSpan, Scene, ScenePolicy, ScopedBindings, TemplateCatalog, TriggerTextSpec,
    WordTimingEvidence, audit_rendered_compositions, compile_selected_event_request,
    resolve_episode_presentation, resolve_scene, resolve_text_triggers,
};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use std::{env, fs, path::Path};

fn read<T: DeserializeOwned>(path: &str) -> Result<T> {
    serde_json::from_slice(&fs::read(path).with_context(|| path.to_owned())?)
        .with_context(|| path.to_owned())
}

fn write_new(path: &str, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    Ok(())
}

fn run() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    match args.as_slice() {
        [
            _,
            command,
            evidence_path,
            spec_path,
            alignment_flag,
            alignment_path,
            receipt_flag,
            receipt_path,
        ] if command == "resolve-trigger-text"
            && alignment_flag == "--alignment"
            && receipt_flag == "--receipt" =>
        {
            if Path::new(alignment_path).exists() || Path::new(receipt_path).exists() {
                bail!("trigger outputs must be new");
            }
            let evidence_bytes = fs::read(evidence_path)?;
            let spec_bytes = fs::read(spec_path)?;
            let evidence: WordTimingEvidence = serde_json::from_slice(&evidence_bytes)?;
            let spec: TriggerTextSpec = serde_json::from_slice(&spec_bytes)?;
            let alignment = resolve_text_triggers(&evidence, &spec)?;
            let alignment_bytes = serde_json::to_vec_pretty(&alignment)?;
            let digest = |bytes: &[u8]| {
                Sha256::digest(bytes)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            };
            write_new(alignment_path, &alignment_bytes)?;
            let receipt = serde_json::json!({
                "schema":"reel.scene-trigger-resolution-receipt.v1",
                "language":alignment.language,"cue_id":alignment.cue_id,
                "selected_take_sha256":alignment.selected_take_sha256,
                "word_evidence_sha256":digest(&evidence_bytes),
                "trigger_spec_sha256":digest(&spec_bytes),
                "native_alignment_sha256":digest(&alignment_bytes),
                "state":"word-evidence-resolved; listening-and-review-held",
                "publication":"not-authorized"
            });
            write_new(receipt_path, &serde_json::to_vec_pretty(&receipt)?)?;
            println!(
                "{} {} semantic markers",
                alignment.cue_id,
                alignment.semantic_markers.len()
            );
        }
        [
            _,
            command,
            catalog,
            episode,
            scene,
            policy,
            season,
            episode_bindings,
            scene_bindings,
            language,
            flag,
            output,
        ] if command == "resolve-language" && flag == "--output" => {
            let resolved = resolve_scene(
                &read::<TemplateCatalog>(catalog)?,
                &read::<Episode>(episode)?,
                &read::<Scene>(scene)?,
                &read::<ScenePolicy>(policy)?,
                &read::<ScopedBindings>(season)?,
                &read::<ScopedBindings>(episode_bindings)?,
                &read::<ScopedBindings>(scene_bindings)?,
            )?;
            let fingerprint = resolved
                .language_fingerprints
                .get(language)
                .ok_or_else(|| anyhow::anyhow!("unknown language {language}"))?;
            let scoped = serde_json::json!({"schema":"reel.resolved-scene-language.v1",
                "scene_id":resolved.scene_id,"language":language,
                "fingerprint_sha256":fingerprint});
            write_new(output, &serde_json::to_vec_pretty(&scoped)?)?;
            println!("{} {} {}", resolved.scene_id, language, fingerprint);
        }
        [
            _,
            command,
            graph,
            pointer,
            scene_path,
            language_id,
            season_path,
            episode_path,
            scene_bindings_path,
            alignment_manifest,
            next_lock,
            flag,
            output,
        ] if command == "compile-events" && flag == "--output" => {
            let scene: Scene = read(scene_path)?;
            let season: ScopedBindings = read(season_path)?;
            let episode: ScopedBindings = read(episode_path)?;
            let scene_bindings: ScopedBindings = read(scene_bindings_path)?;
            let scopes = [&scene_bindings, &episode, &season];
            let alignments = read_verified_alignments(
                &scene,
                language_id,
                Path::new(alignment_manifest),
                &scopes,
            )?;
            let request = compile_selected_event_request(
                &read::<reel_assembly::Graph>(graph)?,
                &read::<reel_assembly::SelectedPointer>(pointer)?,
                &scene,
                language_id,
                &scopes,
                &alignments,
                next_lock,
            )?;
            write_new(output, &serde_json::to_vec_pretty(&request)?)?;
            println!(
                "{} {} REEL event bindings",
                scene.scene_id,
                request.bindings.len()
            );
        }
        [
            _,
            command,
            catalog,
            episode,
            season,
            episode_bindings,
            flag,
            output,
        ] if command == "resolve-episode-presentation" && flag == "--output" => {
            let resolved = resolve_episode_presentation(
                &read::<TemplateCatalog>(catalog)?,
                &read::<Episode>(episode)?,
                &read::<ScopedBindings>(season)?,
                &read::<ScopedBindings>(episode_bindings)?,
            )?;
            write_new(output, &serde_json::to_vec_pretty(&resolved)?)?;
            println!("{} {}", resolved.episode_id, resolved.fingerprint_sha256);
        }
        [
            _,
            command,
            catalog,
            episode,
            scene,
            policy,
            season,
            episode_bindings,
            scene_bindings,
            flag,
            output,
        ] if command == "resolve" && flag == "--output" => {
            let resolved = resolve_scene(
                &read::<TemplateCatalog>(catalog)?,
                &read::<Episode>(episode)?,
                &read::<Scene>(scene)?,
                &read::<ScenePolicy>(policy)?,
                &read::<ScopedBindings>(season)?,
                &read::<ScopedBindings>(episode_bindings)?,
                &read::<ScopedBindings>(scene_bindings)?,
            )?;
            write_new(output, &serde_json::to_vec_pretty(&resolved)?)?;
            println!("{} {}", resolved.scene_id, resolved.fingerprint_sha256);
        }
        [_, command, policy, spans, sample_rate, flag, output]
            if command == "audit-picture" && flag == "--output" =>
        {
            let sample_rate: u32 = sample_rate.parse()?;
            let runs = audit_rendered_compositions(
                &read::<ScenePolicy>(policy)?,
                sample_rate,
                &read::<Vec<RenderedSpan>>(spans)?,
            )?;
            write_new(output, &serde_json::to_vec_pretty(&runs)?)?;
            println!("{} visible composition runs checked", runs.len());
        }
        _ => bail!(
            "usage: reel-scene-authoring resolve <catalog.json> <episode.json> <scene.json> <policy.json> <season-bindings.json> <episode-bindings.json> <scene-bindings.json> --output <new.json>\n       reel-scene-authoring resolve-language <catalog.json> <episode.json> <scene.json> <policy.json> <season-bindings.json> <episode-bindings.json> <scene-bindings.json> <language> --output <new.json>\n       reel-scene-authoring resolve-episode-presentation <catalog.json> <episode.json> <season-bindings.json> <episode-bindings.json> --output <new.json>\n       reel-scene-authoring resolve-trigger-text <word-evidence.json> <trigger-spec.json> --alignment <new.json> --receipt <new.json>\n       reel-scene-authoring compile-events <graph.json> <pointer.json> <scene.json> <language> <season-bindings.json> <episode-bindings.json> <scene-bindings.json> <alignment-paths.json> <next-lock-id> --output <new.json>\n       reel-scene-authoring audit-picture <policy.json> <rendered-spans.json> <sample-rate> --output <new.json>"
        ),
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}
