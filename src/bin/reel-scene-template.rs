//! Compile a scene's editable poem or chapter presentation from its selected
//! template definition and native language-local alignments.

use anyhow::{Context, Result, bail};
use reel_assembly::scene_authoring::{
    AssetRef, BINDINGS_SCHEMA, CATALOG_SCHEMA, EPISODE_SCHEMA, Episode, NativeAlignment,
    SCENE_SCHEMA, Scene, ScopedBindings, TemplateCatalog,
};
use reel_assembly::template_presentation::{
    EditableTextInvocation, EditableTextTemplate, INVOCATION_SCHEMA, PoemLine,
    PresentationSourceText, compile_layer, verify_source_text,
};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    io::Write,
    path::{Component, Path},
};

fn read<T: DeserializeOwned>(path: &str) -> Result<T> {
    serde_json::from_slice(&fs::read(path).with_context(|| path.to_owned())?)
        .with_context(|| path.to_owned())
}

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn selected<'a>(key: &str, scopes: &[&'a ScopedBindings]) -> Result<&'a AssetRef> {
    let found = scopes
        .iter()
        .filter_map(|scope| scope.assets.get(key))
        .collect::<Vec<_>>();
    if found.len() != 1
        || ![
            "selected-private-production",
            "principal-approved",
            "release-cleared",
        ]
        .contains(&found[0].selection_state.as_str())
    {
        bail!("asset binding {key} is absent, ambiguous, or unselected");
    }
    let asset = found[0];
    if asset.cache_uri != format!("cache://sha256/{}", asset.sha256) {
        bail!("asset binding {key} has invalid cache URI");
    }
    Ok(asset)
}

fn alignments(
    scene: &Scene,
    language: &str,
    paths_file: &str,
    scopes: &[&ScopedBindings],
) -> Result<BTreeMap<String, NativeAlignment>> {
    let paths: BTreeMap<String, String> = read(paths_file)?;
    let lane = scene
        .languages
        .get(language)
        .context("scene language missing")?;
    if paths.len() != lane.cues.len() {
        bail!("alignment path set differs from cue set");
    }
    let base = Path::new(paths_file)
        .parent()
        .context("alignment manifest has no parent")?;
    let mut out = BTreeMap::new();
    for cue in &lane.cues {
        let rel = paths.get(&cue.cue_id).context("alignment path missing")?;
        let local = Path::new(rel);
        if local.is_absolute()
            || local
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            bail!("alignment file name must be local and relative");
        }
        let bytes = fs::read(base.join(local))?;
        let binding = selected(
            cue.phrase_alignment_binding
                .as_deref()
                .context("cue has no selected alignment")?,
            scopes,
        )?;
        if sha(&bytes) != binding.sha256 || bytes.len() as u64 != binding.bytes {
            bail!(
                "native alignment bytes differ from selected binding for {}",
                cue.cue_id
            );
        }
        let parsed: NativeAlignment = serde_json::from_slice(&bytes)?;
        let take = selected(
            cue.take_binding
                .as_deref()
                .context("cue has no selected take")?,
            scopes,
        )?;
        if parsed.selected_take_sha256 != take.sha256 {
            bail!(
                "native alignment take differs from selected binding for {}",
                cue.cue_id
            );
        }
        out.insert(cue.cue_id.clone(), parsed);
    }
    Ok(out)
}

fn write_new(path: &str, bytes: &[u8]) -> Result<()> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(bytes)?;
    Ok(())
}

fn run() -> Result<()> {
    let args = env::args().collect::<Vec<_>>();
    let [
        _,
        command,
        catalog_path,
        definition_path,
        episode_authoring_path,
        scene_path,
        language,
        season_path,
        episode_path,
        scene_bindings_path,
        alignment_paths,
        source_text_path,
        flag_ass,
        output_ass,
        flag_receipt,
        output_receipt,
    ] = args.as_slice()
    else {
        bail!(
            "usage: reel-scene-template compile <catalog.json> <definition.json> <episode.json> <scene.json> <language> <season-bindings.json> <episode-bindings.json> <scene-bindings.json> <alignment-paths.json> <source-text.json> --output-ass <new.ass> --receipt <new.json>"
        );
    };
    if command != "compile" || flag_ass != "--output-ass" || flag_receipt != "--receipt" {
        bail!("expected compile --output-ass <new.ass> --receipt <new.json>");
    }
    let catalog: TemplateCatalog = read(catalog_path)?;
    let episode_authoring: Episode = read(episode_authoring_path)?;
    let scene: Scene = read(scene_path)?;
    if catalog.schema != CATALOG_SCHEMA
        || episode_authoring.schema != EPISODE_SCHEMA
        || !["ready-for-private-build", "scene-build-context"]
            .contains(&episode_authoring.authoring_state.as_str())
        || scene.schema != SCENE_SCHEMA
        || scene.authoring_state != "ready-for-private-build"
        || scene.episode_id != episode_authoring.episode_id
        || !episode_authoring.scene_ids.contains(&scene.scene_id)
    {
        bail!("scene is not ready for private build");
    }
    let use_ = scene
        .presentation
        .as_ref()
        .context("scene has no presentation invocation")?;
    let template_entry = catalog
        .templates
        .iter()
        .find(|entry| entry.template_id == use_.template_id)
        .context("template missing from catalog")?;
    if template_entry.kind != use_.role {
        bail!("template role mismatch");
    }
    let definition_bytes = fs::read(definition_path)?;
    let definition_sha256 = sha(&definition_bytes);
    if definition_sha256 != template_entry.definition_sha256 {
        bail!("template definition hash differs from catalog");
    }
    let definition: EditableTextTemplate = serde_json::from_slice(&definition_bytes)?;
    if definition.template_id != template_entry.template_id
        || definition.kind != template_entry.kind
    {
        bail!("template definition identity mismatch");
    }
    let content = use_
        .content
        .as_object()
        .context("scene presentation content must be an object")?;
    for key in &template_entry.required_content_keys {
        if !content.contains_key(key) {
            bail!("scene presentation missing {key}");
        }
    }
    if content.keys().any(|key| {
        matches!(
            key.as_str(),
            "layout" | "font" | "colors" | "duration_seconds" | "panel"
        )
    }) {
        bail!("scene content tries to own template layout");
    }
    let bilingual = scene.languages.len() > 1;
    let title = content
        .get("titles")
        .and_then(|v| v.get(language))
        .and_then(|v| v.as_str())
        .or_else(|| {
            (!bilingual)
                .then(|| content.get("title").and_then(|v| v.as_str()))
                .flatten()
        })
        .context("language-local presentation title missing")?;
    let line_value = content
        .get("lines_by_language")
        .and_then(|v| v.get(language))
        .or_else(|| (!bilingual).then(|| content.get("lines")).flatten());
    let lines: Vec<PoemLine> = match line_value {
        Some(value) => serde_json::from_value(value.clone())?,
        None => vec![],
    };
    if definition.kind == "opening-poem" && lines.is_empty() {
        bail!("language-local poem lines missing");
    }
    let invocation = EditableTextInvocation {
        schema: INVOCATION_SCHEMA.into(),
        template_id: use_.template_id.clone(),
        language: language.clone(),
        title: title.into(),
        byline: content
            .get("bylines")
            .and_then(|value| value.get(language))
            .and_then(|value| value.as_str())
            .map(str::to_owned),
        lines,
        chapter_number: content
            .get("chapter_number")
            .and_then(|v| v.as_str())
            .map(str::to_owned),
    };
    let season: ScopedBindings = read(season_path)?;
    let episode: ScopedBindings = read(episode_path)?;
    let scene_bindings: ScopedBindings = read(scene_bindings_path)?;
    if [&season, &episode, &scene_bindings]
        .iter()
        .any(|scope| scope.schema != BINDINGS_SCHEMA || scope.scope_id.is_empty())
        || episode.scope_id != episode_authoring.episode_id
        || scene_bindings.scope_id != scene.scene_id
        || season.scope_id != episode_authoring.season_id
    {
        bail!("asset binding files have wrong schema or scope identity");
    }
    let scopes = [&scene_bindings, &episode, &season];
    let source_key = content
        .get("source_text_bindings")
        .and_then(|value| value.get(language))
        .and_then(|value| value.as_str())
        .context("language-local source-text binding missing")?;
    let source_binding = selected(source_key, &scopes)?;
    let source_bytes = fs::read(source_text_path)?;
    if sha(&source_bytes) != source_binding.sha256
        || source_bytes.len() as u64 != source_binding.bytes
    {
        bail!("source-text file differs from selected scoped binding");
    }
    let source_text: PresentationSourceText = serde_json::from_slice(&source_bytes)?;
    let mut presentation_scope = scene.source_scope_ids.clone();
    for source_id in &scene.presentation_source_scope_ids {
        if source_id.is_empty() || presentation_scope.contains(source_id) {
            bail!("presentation source scope is empty or duplicates a spoken source ID");
        }
        presentation_scope.push(source_id.clone());
    }
    verify_source_text(
        &invocation,
        &source_text,
        &scene.source_authority_id,
        &presentation_scope,
    )?;
    let lane = scene
        .languages
        .get(language)
        .context("scene language missing")?;
    let (ordered_cues, native) = if definition.kind == "opening-poem" {
        (
            lane.cues
                .iter()
                .map(|cue| cue.cue_id.clone())
                .collect::<Vec<_>>(),
            alignments(&scene, language, alignment_paths, &scopes)?,
        )
    } else {
        (vec![], BTreeMap::new())
    };
    let line_cue_ids = content
        .get("line_cue_ids")
        .and_then(|v| v.get(language))
        .and_then(|v| v.as_array());
    if definition.kind == "opening-poem" && line_cue_ids.is_none() {
        bail!("poem needs a language-local line-to-cue map");
    }
    if let Some(list) = line_cue_ids {
        let declared = list
            .iter()
            .map(|v| v.as_str().context("line cue ID must be string"))
            .collect::<Result<Vec<_>>>()?;
        let actual = invocation
            .lines
            .iter()
            .map(|line| line.cue_id.as_str())
            .collect::<Vec<_>>();
        if declared != actual {
            bail!("line-to-cue map differs from source lines");
        }
        let covered = actual
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        let selected = ordered_cues
            .iter()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        if covered != selected {
            bail!("poem display does not cover every selected native cue");
        }
    }
    let compiled = compile_layer(&definition, &invocation, &ordered_cues, &native)?;
    let ass_bytes = compiled.ass.as_bytes();
    let receipt = serde_json::json!({
        "schema":"reel.editable-layer-compile-receipt.v1", "scene_id":scene.scene_id,
        "language":language, "template_id":compiled.template_id,
        "template_definition_sha256":definition_sha256, "ass_sha256":sha(ass_bytes),
        "source_text_sha256":source_binding.sha256,
        "source_text_state":source_text.text_state,
        "ass_bytes":ass_bytes.len(), "duration_samples":compiled.duration_samples,
        "sample_rate":compiled.sample_rate, "publication":"not-authorized"
    });
    write_new(output_ass, ass_bytes)?;
    write_new(output_receipt, &serde_json::to_vec_pretty(&receipt)?)?;
    println!("{} {} {}", scene.scene_id, language, sha(ass_bytes));
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}
