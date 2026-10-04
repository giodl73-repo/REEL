use anyhow::{Context, Result, bail};
use reel::scene_authoring_inputs::read_verified_alignments;
use reel::scene_cue_compose::{
    ComposeManifest, ComposeWordEvidenceManifest, compose, compose_word_evidence,
};
use reel::scene_delivery_compile::{CompileManifest, compile_to_dir};
use reel_assembly::scene_authoring::{
    CacheObjectRef, Episode, ProviderWordImport, RenderedSpan, Scene, ScenePolicy, ScopedBindings,
    TemplateCatalog, TriggerTextSpec, WhisperCppWordImport, WordTimingEvidence,
    audit_rendered_compositions, authored_trigger_text, compile_selected_event_request,
    import_provider_words, import_whispercpp_words, resolve_episode_presentation, resolve_scene,
    resolve_text_triggers,
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

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn verify_cache_object(root: &Path, object: &CacheObjectRef) -> Result<()> {
    if object.sha256.len() != 64
        || !object.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        || object.cache_uri != format!("cache://sha256/{}", object.sha256)
        || object.bytes == 0
    {
        bail!("invalid cache object reference");
    }
    let path = root
        .join("objects")
        .join("sha256")
        .join(&object.sha256[..2])
        .join(&object.sha256);
    let bytes = fs::read(&path).with_context(|| path.display().to_string())?;
    if bytes.len() as u64 != object.bytes || digest(&bytes) != object.sha256 {
        bail!(
            "cache object hash or byte count mismatch: {}",
            path.display()
        );
    }
    Ok(())
}

fn run() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    match args.as_slice() {
        [_, command, project_root, manifest_path, cache_root] if command == "compose-cue" => {
            let manifest: ComposeManifest = read(manifest_path)?;
            let receipt = compose(Path::new(project_root), Path::new(cache_root), &manifest)?;
            println!(
                "{} {} {} samples composed from {} segments",
                receipt["cue_id"].as_str().unwrap_or("cue"),
                receipt["language"].as_str().unwrap_or("language"),
                receipt["sample_count"].as_u64().unwrap_or(0),
                receipt["segments"].as_array().map_or(0, Vec::len)
            );
        }
        [_, command, project_root, manifest_path, cache_root]
            if command == "compose-word-evidence" =>
        {
            let manifest: ComposeWordEvidenceManifest = read(manifest_path)?;
            let receipt =
                compose_word_evidence(Path::new(project_root), Path::new(cache_root), &manifest)?;
            println!(
                "{} {} {} words composed from {} segments",
                receipt["cue_id"].as_str().unwrap_or("cue"),
                receipt["language"].as_str().unwrap_or("language"),
                receipt["word_count"].as_u64().unwrap_or(0),
                receipt["segment_word_evidence"]
                    .as_array()
                    .map_or(0, Vec::len)
            );
        }
        [_, command, project_root, manifest_path] if command == "compile-delivery" => {
            let manifest: CompileManifest = read(manifest_path)?;
            let receipt = compile_to_dir(Path::new(project_root), &manifest)?;
            println!(
                "{} {} {} semantic events compiled",
                receipt["scene_id"].as_str().unwrap_or("scene"),
                receipt["language"].as_str().unwrap_or("language"),
                receipt["semantic_event_count"].as_u64().unwrap_or(0)
            );
        }
        [
            _,
            command,
            spec_path,
            asset_root,
            output_flag,
            output_path,
            receipt_flag,
            receipt_path,
        ] if command == "import-provider-words"
            && output_flag == "--output"
            && receipt_flag == "--receipt" =>
        {
            if Path::new(output_path).exists() || Path::new(receipt_path).exists() {
                bail!("provider word import outputs must be new");
            }
            let spec_bytes = fs::read(spec_path)?;
            let spec: ProviderWordImport = serde_json::from_slice(&spec_bytes)?;
            verify_cache_object(Path::new(asset_root), &spec.take)?;
            verify_cache_object(Path::new(asset_root), &spec.alignment)?;
            let response_name = Path::new(&spec.alignment_path);
            if response_name.components().count() != 1
                || !matches!(
                    response_name.components().next(),
                    Some(std::path::Component::Normal(_))
                )
            {
                bail!("provider alignment response path must be a local file name");
            }
            let source = Path::new(spec_path)
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(&spec.alignment_path);
            let response = fs::read(&source).with_context(|| source.display().to_string())?;
            if response.len() as u64 != spec.alignment.bytes
                || digest(&response) != spec.alignment.sha256
            {
                bail!("provider alignment response hash or byte count mismatch");
            }
            let evidence = import_provider_words(&response, &spec)?;
            let evidence_bytes = serde_json::to_vec_pretty(&evidence)?;
            write_new(output_path, &evidence_bytes)?;
            let receipt = serde_json::json!({
                "schema":"reel.provider-word-import-receipt.v1",
                "provider":spec.provider,
                "language":evidence.language,
                "cue_id":evidence.cue_id,
                "take_sha256":spec.take.sha256,
                "alignment_sha256":spec.alignment.sha256,
                "import_spec_sha256":digest(&spec_bytes),
                "word_evidence_sha256":digest(&evidence_bytes),
                "word_count":evidence.words.len(),
                "state":"provider word evidence; listening-and-text-review-held",
                "publication":"not-authorized"
            });
            write_new(receipt_path, &serde_json::to_vec_pretty(&receipt)?)?;
            println!(
                "{} {} provider-timed words",
                evidence.cue_id,
                evidence.words.len()
            );
        }
        [
            _,
            command,
            spec_path,
            asset_root,
            output_flag,
            output_path,
            receipt_flag,
            receipt_path,
        ] if command == "import-whispercpp-words"
            && output_flag == "--output"
            && receipt_flag == "--receipt" =>
        {
            if Path::new(output_path).exists() || Path::new(receipt_path).exists() {
                bail!("word import outputs must be new");
            }
            let spec_bytes = fs::read(spec_path)?;
            let spec: WhisperCppWordImport = serde_json::from_slice(&spec_bytes)?;
            let root = Path::new(asset_root);
            for object in [&spec.take, &spec.model, &spec.tool] {
                verify_cache_object(root, object)?;
            }
            let source = Path::new(spec_path)
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(&spec.transcription_path);
            let transcript = fs::read(&source).with_context(|| source.display().to_string())?;
            if transcript.len() as u64 != spec.transcription_bytes
                || digest(&transcript) != spec.transcription_sha256
            {
                bail!("whisper.cpp transcription hash or byte count mismatch");
            }
            let (evidence, diagnostics) = import_whispercpp_words(&transcript, &spec)?;
            let evidence_bytes = serde_json::to_vec_pretty(&evidence)?;
            write_new(output_path, &evidence_bytes)?;
            let receipt = serde_json::json!({
                "schema":"reel.whispercpp-word-import-receipt.v1",
                "language":evidence.language,
                "cue_id":evidence.cue_id,
                "take_sha256":spec.take.sha256,
                "model_sha256":spec.model.sha256,
                "tool_sha256":spec.tool.sha256,
                "transcription_sha256":spec.transcription_sha256,
                "import_spec_sha256":digest(&spec_bytes),
                "word_evidence_sha256":digest(&evidence_bytes),
                "word_count":evidence.words.len(),
                "diagnostics":diagnostics,
                "state":"machine-word-evidence; listening-and-text-review-held",
                "publication":"not-authorized"
            });
            write_new(receipt_path, &serde_json::to_vec_pretty(&receipt)?)?;
            println!("{} {} timed words", evidence.cue_id, evidence.words.len());
        }
        [
            _,
            command,
            scene_path,
            evidence_path,
            text_path,
            alignment_flag,
            alignment_path,
            receipt_flag,
            receipt_path,
        ] if command == "resolve-scene-trigger-text"
            && alignment_flag == "--alignment"
            && receipt_flag == "--receipt" =>
        {
            if Path::new(alignment_path).exists() || Path::new(receipt_path).exists() {
                bail!("trigger outputs must be new");
            }
            let scene_bytes = fs::read(scene_path)?;
            let evidence_bytes = fs::read(evidence_path)?;
            let text = fs::read_to_string(text_path)?;
            let scene: Scene = serde_json::from_slice(&scene_bytes)?;
            let evidence: WordTimingEvidence = serde_json::from_slice(&evidence_bytes)?;
            let spec = authored_trigger_text(&scene, &evidence, &text)?;
            let alignment = resolve_text_triggers(&evidence, &spec)?;
            let bytes = serde_json::to_vec_pretty(&alignment)?;
            let receipt = serde_json::json!({
                "schema":"reel.scene-trigger-resolution-receipt.v1",
                "language":alignment.language,"cue_id":alignment.cue_id,
                "selected_take_sha256":alignment.selected_take_sha256,
                "scene_sha256":digest(&scene_bytes),
                "word_evidence_sha256":digest(&evidence_bytes),
                "spoken_text_sha256":spec.spoken_text_sha256,
                "trigger_spec_sha256":digest(&serde_json::to_vec_pretty(&spec)?),
                "native_alignment_sha256":digest(&bytes),
                "state":"authored-phrases-resolved; listening-and-review-held",
                "publication":"not-authorized"
            });
            write_new(alignment_path, &bytes)?;
            write_new(receipt_path, &serde_json::to_vec_pretty(&receipt)?)?;
            println!(
                "{} {} authored semantic markers",
                alignment.cue_id,
                alignment.semantic_markers.len()
            );
        }
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
            episode_authoring_path,
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
            let episode_authoring: Episode = read(episode_authoring_path)?;
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
                &episode_authoring,
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
            "usage: reel-scene-authoring compose-cue <project-root> <compose-manifest.json> <cache-root>\n       reel-scene-authoring compose-word-evidence <project-root> <word-compose-manifest.json> <cache-root>\n       reel-scene-authoring compile-delivery <project-root> <compile-manifest.json>\n       reel-scene-authoring resolve <catalog.json> <episode.json> <scene.json> <policy.json> <season-bindings.json> <episode-bindings.json> <scene-bindings.json> --output <new.json>\n       reel-scene-authoring resolve-language <catalog.json> <episode.json> <scene.json> <policy.json> <season-bindings.json> <episode-bindings.json> <scene-bindings.json> <language> --output <new.json>\n       reel-scene-authoring resolve-episode-presentation <catalog.json> <episode.json> <season-bindings.json> <episode-bindings.json> --output <new.json>\n       reel-scene-authoring import-provider-words <import-spec.json> <asset-root> --output <word-evidence.json> --receipt <new.json>\n       reel-scene-authoring import-whispercpp-words <import-spec.json> <asset-root> --output <word-evidence.json> --receipt <new.json>\n       reel-scene-authoring resolve-scene-trigger-text <scene.json> <word-evidence.json> <exact-spoken-text.txt> --alignment <new.json> --receipt <new.json>\n       reel-scene-authoring resolve-trigger-text <word-evidence.json> <trigger-spec.json> --alignment <new.json> --receipt <new.json>\n       reel-scene-authoring compile-events <graph.json> <pointer.json> <episode.json> <scene.json> <language> <season-bindings.json> <episode-bindings.json> <scene-bindings.json> <alignment-paths.json> <next-lock-id> --output <new.json>\n       reel-scene-authoring audit-picture <policy.json> <rendered-spans.json> <sample-rate> --output <new.json>"
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
