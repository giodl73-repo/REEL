use anyhow::{Context, Result, bail};
use reel_assembly::scene_authoring::{
    Episode, NativeAlignment, RenderedSpan, Scene, ScenePolicy, ScopedBindings, TemplateCatalog,
    audit_rendered_compositions, compile_selected_event_request, resolve_episode_presentation,
    resolve_scene,
};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, env, fs, path::Path};

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

fn read_verified_alignments(
    scene: &Scene,
    language_id: &str,
    manifest_path: &str,
    scopes: &[&ScopedBindings],
) -> Result<BTreeMap<String, NativeAlignment>> {
    let paths: BTreeMap<String, String> = read(manifest_path)?;
    let lane = scene
        .languages
        .get(language_id)
        .context("scene language missing")?;
    if paths.len() != lane.cues.len() {
        bail!("alignment path set does not match cue set");
    }
    let base = Path::new(manifest_path)
        .parent()
        .context("alignment manifest has no directory")?;
    let mut verified = BTreeMap::new();
    for cue in &lane.cues {
        let relative = paths
            .get(&cue.cue_id)
            .context("cue alignment path missing")?;
        let path = Path::new(relative);
        if path.is_absolute()
            || path
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            bail!("alignment paths must be local relative names");
        }
        let bytes = fs::read(base.join(path))?;
        let actual_sha = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let key = cue
            .phrase_alignment_binding
            .as_deref()
            .context("cue lacks alignment binding")?;
        let matches = scopes
            .iter()
            .filter_map(|scope| scope.assets.get(key))
            .collect::<Vec<_>>();
        if matches.len() != 1
            || matches[0].sha256 != actual_sha
            || matches[0].bytes != bytes.len() as u64
        {
            bail!(
                "alignment {} differs from selected scoped bytes",
                cue.cue_id
            );
        }
        verified.insert(cue.cue_id.clone(), serde_json::from_slice(&bytes)?);
    }
    Ok(verified)
}

fn run() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    match args.as_slice() {
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
            let alignments =
                read_verified_alignments(&scene, language_id, alignment_manifest, &scopes)?;
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
            "usage: reel-scene-authoring resolve <catalog.json> <episode.json> <scene.json> <policy.json> <season-bindings.json> <episode-bindings.json> <scene-bindings.json> --output <new.json>\n       reel-scene-authoring resolve-episode-presentation <catalog.json> <episode.json> <season-bindings.json> <episode-bindings.json> --output <new.json>\n       reel-scene-authoring compile-events <graph.json> <pointer.json> <scene.json> <language> <season-bindings.json> <episode-bindings.json> <scene-bindings.json> <alignment-paths.json> <next-lock-id> --output <new.json>\n       reel-scene-authoring audit-picture <policy.json> <rendered-spans.json> <sample-rate> --output <new.json>"
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
