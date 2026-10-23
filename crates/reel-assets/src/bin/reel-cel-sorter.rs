//! Explicit, offline editor export and optimistic native-draft application.
use reel_assets::cel_source::project::Project;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
#[cfg(test)]
use reel_assets::cel_source::project::FILES;
fn hash(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}
fn read(p: &Path) -> Result<Value> {
    let b = bounded(p)?;
    Ok(serde_json::from_slice(&b)?)
}
fn bounded(p: &Path) -> Result<Vec<u8>> {
    let mut b = vec![];
    fs::File::open(p)?
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut b)?;
    if b.len() > 2 * 1024 * 1024 {
        return Err("control file too large".into());
    }
    Ok(b)
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key]
        .as_str()
        .ok_or_else(|| format!("missing string {key}").into())
}
fn integer(v: &Value, key: &str) -> Result<u64> {
    v[key]
        .as_u64()
        .ok_or_else(|| format!("missing integer {key}").into())
}
fn write(p: &Path, v: &Value) -> Result<()> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(p)?;
    f.write_all(&serde_json::to_vec(v)?)?;
    Ok(())
}
#[cfg(test)]
fn source(project: &Path) -> Result<(BTreeMap<String, Value>, Value)> {
    load(&Project::open(project)?)
}
fn load(project: &Project) -> Result<(BTreeMap<String, Value>, Value)> {
    let snapshot = project.load()?;
    let docs = snapshot.controls;
    let hashes = snapshot.hashes;
    let alignment_size = snapshot.bytes["alignment.json"];
    if docs["scene.json"]["schema"] != "reel.scene-authoring.v2"
        || docs["scene-bindings.json"]["schema"] != "reel.scene-asset-bindings.v1"
        || docs["alignment.json"]["schema"] != "reel.scene-native-alignment.v1"
    {
        return Err("unsupported native input".into());
    }
    if docs["scene.json"]["scene_id"] != docs["scene-bindings.json"]["scope_id"] {
        return Err("scope mismatch".into());
    }
    if docs["scene.json"]["languages"].as_object().map(|x| x.len()) != Some(1)
        || docs["scene.json"]["canonical_cue_ids"]
            .as_array()
            .map(|x| x.len())
            != Some(1)
    {
        return Err("editor requires one language and one full recording cue".into());
    }
    let lang = text(&docs["alignment.json"], "language")?;
    let cues = docs["scene.json"]["languages"][lang]["cues"]
        .as_array()
        .ok_or("missing native recording cue")?;
    if cues.len() != 1 {
        return Err("editor requires one full recording cue per language".into());
    }
    let alignment_binding = text(&cues[0], "phrase_alignment_binding")?;
    let bound = &docs["scene-bindings.json"]["assets"][alignment_binding];
    if bound["sha256"] != hashes["alignment.json"] || bound["bytes"] != alignment_size {
        return Err("native alignment binding differs from exact intake bytes".into());
    }
    Ok((docs, hashes))
}
fn identity(asset: &Value) -> Result<(&str, u64)> {
    let h = text(asset, "sha256")?;
    if h.len() != 64
        || !h
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || asset["cache_uri"] != format!("cache://sha256/{h}")
    {
        return Err("invalid asset identity".into());
    }
    Ok((h, integer(asset, "bytes")?))
}
fn resolve(roots: &[PathBuf], asset: &Value) -> Result<PathBuf> {
    let (h, bytes) = identity(asset)?;
    for root in roots {
        let p = root.join("objects/sha256").join(&h[..2]).join(h);
        let metadata = match fs::symlink_metadata(&p) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("cache object is not a regular file".into());
        }
        if metadata.len() != bytes {
            return Err("cache byte mismatch".into());
        }
        let file = fs::File::open(&p)?;
        if file.metadata()?.len() != bytes {
            return Err("cache object changed before verification".into());
        }
        let mut f = file.take(bytes.checked_add(1).ok_or("asset size overflow")?);
        let mut digest = Sha256::new();
        let mut b = [0; 65536];
        loop {
            let n = f.read(&mut b)?;
            if n == 0 {
                break;
            }
            digest.update(&b[..n]);
        }
        if format!("{:x}", digest.finalize()) != h {
            return Err("cache hash mismatch".into());
        }
        return Ok(p);
    }
    Err(format!("active cache object absent {h}").into())
}
fn link(source: &Path, target: &Path, expected_hash: &str, expected: u64) -> Result<()> {
    // Working editor files must never be writable aliases of canonical objects.
    let input = fs::File::open(source)?;
    if input.metadata()?.len() != expected {
        return Err("export source size changed before copy".into());
    }
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)?;
    let mut reader = input.take(expected.checked_add(1).ok_or("asset size overflow")?);
    let mut digest = Sha256::new();
    let mut copied = 0u64;
    let mut buffer = [0; 65536];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        output.write_all(&buffer[..count])?;
        digest.update(&buffer[..count]);
        copied += count as u64;
    }
    if copied != expected || format!("{:x}", digest.finalize()) != expected_hash {
        return Err("export copy hash/size mismatch".into());
    }
    Ok(())
}
fn native_graph(docs: &BTreeMap<String, Value>) -> Result<reel_assembly::Graph> {
    let graph = serde_json::from_value(docs["selected-graph.json"].clone())?;
    let pointer = serde_json::from_value(docs["selected-pointer.json"].clone())?;
    reel_assembly::validate_selected_graph(&pointer, &graph)?;
    Ok(graph)
}
fn rows(docs: &BTreeMap<String, Value>) -> Result<Value> {
    let native = reel_assets::cel_source::inspect(
        &serde_json::to_vec(&docs["scene.json"])?,
        &serde_json::to_vec(&docs["scene-bindings.json"])?,
    )?;
    if native.imported_evidence {
        return Err("cel editing requires native ready authoring, not imported evidence".into());
    }
    let selected = native_graph(docs)?;
    for cel in &native.cels {
        let slot = selected
            .slots
            .iter()
            .find(|slot| slot.slot_id == cel.picture_slot_id)
            .ok_or("native picture slot missing")?;
        let revision = slot
            .revisions
            .iter()
            .find(|revision| Some(&revision.revision_id) == slot.selected_revision_id.as_ref())
            .ok_or("native picture revision missing")?;
        let event = selected
            .events
            .iter()
            .find(|event| event.event_id == cel.event_id)
            .ok_or("native picture event missing")?;
        if slot.lane != reel_assembly::Lane::Picture
            || slot.disposition != reel_assembly::Disposition::Selected
            || revision.asset.sha256 != cel.asset.sha256
            || event.picture.sha256 != cel.asset.sha256
        {
            return Err("native effective picture differs from selected graph".into());
        }
    }
    let effective = native
        .cels
        .iter()
        .map(|cel| {
            Ok((
                cel.shared_semantic_id
                    .as_deref()
                    .ok_or("missing shared semantic identity")?,
                cel,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let alignment = &docs["alignment.json"];
    let sr = integer(alignment, "sample_rate")?;
    let fps = integer(&docs["profile.json"], "frame_rate_numerator")?;
    let den = integer(&docs["profile.json"], "frame_rate_denominator")?;
    if sr == 0 || fps == 0 || den == 0 || sr.checked_mul(den).is_none() || (sr * den) % fps != 0 {
        return Err("unsupported nonintegral sample/frame clock".into());
    }
    let spf = sr * den / fps;
    let events = docs["scene.json"]["shared_events"]
        .as_array()
        .ok_or("no picture events")?;
    let markers = alignment["semantic_markers"].as_object().ok_or("markers")?;
    if markers.len() != events.len() || events.len() > 1000 {
        return Err("unsupported extra markers or excessive events".into());
    }
    let lang = text(alignment, "language")?;
    let cues = docs["scene.json"]["languages"][lang]["cues"]
        .as_array()
        .ok_or("missing native recording cue")?;
    if cues.len() != 1 {
        return Err("editor requires one full recording cue per language".into());
    }
    let cue = &cues[0];
    let take = text(cue, "take_binding")?;
    let measured: reel_assembly::scene_authoring::NativeAlignment =
        serde_json::from_value(alignment.clone())?;
    reel_assembly::scene_authoring::validate_native_alignment(
        &measured,
        lang,
        text(cue, "cue_id")?,
        text(&docs["scene-bindings.json"]["assets"][take], "sha256")?,
    )?;
    let language = docs["scene.json"]["language_event_bindings"][lang]
        .as_array()
        .ok_or("language events")?;
    let native_ids = language
        .iter()
        .map(|e| text(e, "event_id"))
        .collect::<Result<BTreeSet<_>>>()?;
    let graph = &docs["selected-graph.json"];
    let graph_ids = graph["events"]
        .as_array()
        .ok_or("graph events")?
        .iter()
        .map(|e| text(e, "event_id"))
        .collect::<Result<BTreeSet<_>>>()?;
    let nodes = graph["nodes"].as_array().ok_or("graph nodes")?;
    let declared_slots = language
        .iter()
        .map(|e| text(e, "picture_slot_id"))
        .collect::<Result<BTreeSet<_>>>()?;
    let graph_slots = graph["slots"]
        .as_array()
        .ok_or("graph slots")?
        .iter()
        .filter(|s| s["lane"] == "picture")
        .map(|s| text(s, "slot_id"))
        .collect::<Result<BTreeSet<_>>>()?;
    if language.len() != events.len()
        || native_ids.len() != events.len()
        || native_ids != graph_ids
        || nodes.len() != 1
        || nodes[0]["id"] != docs["scene.json"]["scene_id"]
        || declared_slots != graph_slots
    {
        return Err("unsupported unrelated graph scope or nonunique event bindings".into());
    }
    let mut ordered = vec![];
    for e in events {
        let id = text(e, "semantic_id")?;
        let binding = effective
            .get(id)
            .ok_or("missing effective native event")?
            .picture_binding
            .as_str();
        let sample = alignment["semantic_markers"][id]
            .as_u64()
            .ok_or("missing marker")?;
        if sample % spf != 0 {
            return Err("marker not frame aligned".into());
        }
        if docs["scene-bindings.json"]["assets"][binding].is_null() {
            return Err("unbound picture".into());
        }
        ordered.push((sample, id, binding));
    }
    ordered.sort_by_key(|x| x.0);
    if ordered.is_empty() || ordered[0].0 != 0 || ordered.windows(2).any(|x| x[0].0 == x[1].0) {
        return Err("ambiguous or incomplete event clock".into());
    }
    let end = integer(alignment, "cue_end_sample")?;
    if end % spf != 0 {
        return Err("end not frame aligned".into());
    }
    let mut result = vec![];
    for (i, (sample, id, binding)) in ordered.iter().enumerate() {
        let next = ordered.get(i + 1).map(|x| x.0).unwrap_or(end);
        if next <= *sample {
            return Err("invalid event duration".into());
        }
        result.push(json!({"semantic_id":id,"picture_binding":binding,"frames":(next-sample)/spf}));
    }
    Ok(json!(result))
}
struct MediaPlan {
    assets: BTreeMap<String, Value>,
    pictures: BTreeSet<String>,
    images: serde_json::Map<String, Value>,
    movie_hash: String,
}
fn preflight(docs: &BTreeMap<String, Value>, rows: &Value, spec: &Value) -> Result<MediaPlan> {
    let mut plan = MediaPlan {
        assets: BTreeMap::new(),
        pictures: BTreeSet::new(),
        images: serde_json::Map::new(),
        movie_hash: String::new(),
    };
    for row in rows.as_array().ok_or("rows")? {
        let key = text(row, "picture_binding")?;
        let asset = &docs["scene-bindings.json"]["assets"][key];
        let (hash, bytes) = identity(asset)?;
        if let Some(previous) = plan.assets.get(hash)
            && integer(previous, "bytes")? != bytes
        {
            return Err("conflicting sizes for one media identity".into());
        }
        plan.assets
            .entry(hash.into())
            .or_insert_with(|| asset.clone());
        plan.pictures.insert(hash.into());
        plan.images
            .insert(key.into(), json!(format!("media/{hash}.png")));
    }
    let (movie_hash, movie_bytes) = identity(&spec["movie"])?;
    if let Some(previous) = plan.assets.get(movie_hash)
        && integer(previous, "bytes")? != movie_bytes
    {
        return Err("conflicting sizes for one media identity".into());
    }
    // Pictures share one destination per hash. The movie is a second output
    // name even if it has the same hash as a picture, so count its copy twice.
    let total = plan
        .assets
        .values()
        .try_fold(movie_bytes, |sum, asset| -> Result<u64> {
            sum.checked_add(integer(asset, "bytes")?)
                .ok_or_else(|| "export size overflow".into())
        })?;
    let budget = integer(spec, "max_export_copy_bytes")?;
    if budget > 1024 * 1024 * 1024 || total > budget {
        return Err("explicit editor-copy budget exceeded".into());
    }
    plan.movie_hash = movie_hash.into();
    plan.assets
        .entry(movie_hash.into())
        .or_insert_with(|| spec["movie"].clone());
    Ok(plan)
}
fn resolve_plan(
    plan: &MediaPlan,
    mut resolver: impl FnMut(&Value) -> Result<PathBuf>,
) -> Result<BTreeMap<String, PathBuf>> {
    plan.assets
        .iter()
        .map(|(hash, asset)| Ok((hash.clone(), resolver(asset)?)))
        .collect()
}
fn export(spec: &Path) -> Result<()> {
    let s = read(spec)?;
    let project = Path::new(text(&s, "project")?);
    let out = Path::new(text(&s, "output")?);
    if out.exists() {
        return Err("export exists; choose a new version".into());
    }
    let project = Project::open(project)?;
    let (docs, hashes) = load(&project)?;
    let r = rows(&docs)?;
    // Decide the complete declared copy budget before touching any media.
    let plan = preflight(&docs, &r, &s)?;
    let roots: Vec<PathBuf> = s["cache_roots"]
        .as_array()
        .ok_or("cache_roots required")?
        .iter()
        .map(|v| v.as_str().map(PathBuf::from).ok_or("invalid root"))
        .collect::<std::result::Result<_, _>>()?;
    let resolved = resolve_plan(&plan, |asset| resolve(&roots, asset))?;
    fs::create_dir_all(out.join("media"))?;
    for h in &plan.pictures {
        let dest = out.join(format!("media/{h}.png"));
        link(&resolved[h], &dest, h, integer(&plan.assets[h], "bytes")?)?;
    }
    link(
        &resolved[&plan.movie_hash],
        &out.join("performance.mp4"),
        &plan.movie_hash,
        integer(&s["movie"], "bytes")?,
    )?;
    let mut data = json!({"schema":"reel.cel-sorter-session.v1","title":s["title"],"scene_id":docs["scene.json"]["scene_id"],"expected_inputs":hashes,"rows":r,"images":plan.images,"sample_rate":docs["alignment.json"]["sample_rate"],"end_sample":docs["alignment.json"]["cue_end_sample"],"fps_numerator":docs["profile.json"]["frame_rate_numerator"],"fps_denominator":docs["profile.json"]["frame_rate_denominator"],"movie":"performance.mp4"});
    if let Some(hash) = project.descriptor_sha256() {
        data["schema"] = json!("reel.cel-sorter-session.v2");
        data["expected_project"] = json!(hash);
    }
    write(&out.join("session.json"), &data)?;
    fs::write(out.join("index.html"), include_str!("../cel_sorter.html"))?;
    fs::write(out.join("editor.js"), include_str!("../cel_sorter.js"))?;
    println!("{}", out.join("index.html").display());
    Ok(())
}
fn apply(project: &Path, request: &Path, out: &Path) -> Result<()> {
    let r = read(request)?;
    let project = Project::open(project)?;
    let (mut docs, hashes) = load(&project)?;
    if !project.accepts_edit(&r)
        || r["expected_inputs"] != hashes
        || r["scene_id"] != docs["scene.json"]["scene_id"]
    {
        return Err("stale or unsupported edit; source hashes differ".into());
    }
    let original = rows(&docs)?;
    let original = original.as_array().ok_or("rows")?;
    let edited = r["rows"].as_array().ok_or("missing rows")?;
    if edited.is_empty() || edited.len() > original.len() {
        return Err("unsupported event insertion".into());
    }
    let known: BTreeSet<&str> = original
        .iter()
        .filter_map(|x| x["semantic_id"].as_str())
        .collect();
    let bindings: BTreeSet<&str> = original
        .iter()
        .filter_map(|x| x["picture_binding"].as_str())
        .collect();
    let mut seen = BTreeSet::new();
    let spf = integer(&docs["alignment.json"], "sample_rate")?
        * integer(&docs["profile.json"], "frame_rate_denominator")?
        / integer(&docs["profile.json"], "frame_rate_numerator")?;
    let mut total = 0u64;
    let mut markers = serde_json::Map::new();
    let old_events = docs["scene.json"]["shared_events"]
        .as_array()
        .ok_or("events")?
        .clone();
    let lang = text(&docs["alignment.json"], "language")?.to_owned();
    let old_language = docs["scene.json"]["language_event_bindings"][&lang]
        .as_array()
        .ok_or("missing language events")?
        .clone();
    let mut events = vec![];
    for row in edited {
        let id = text(row, "semantic_id")?;
        let binding = text(row, "picture_binding")?;
        let frames = integer(row, "frames")?;
        if !known.contains(id) || !bindings.contains(binding) || !seen.insert(id) || frames == 0 {
            return Err("invalid stable event, binding or hold".into());
        }
        markers.insert(id.into(), json!(total));
        total = total
            .checked_add(frames.checked_mul(spf).ok_or("duration overflow")?)
            .ok_or("duration overflow")?;
        let mut e = old_events
            .iter()
            .find(|e| e["semantic_id"] == id)
            .ok_or("event absent")?
            .clone();
        let realization = old_language
            .iter()
            .find(|event| event["semantic_id"] == id)
            .ok_or("missing native language event")?;
        // Preserve shared defaults when the source deliberately chose a local
        // override. A no-op round trip must not rewrite the editorial layer.
        if realization["picture_binding_override"].is_null() {
            e["picture_binding"] = json!(binding);
        }
        events.push(e);
    }
    if total != integer(&docs["alignment.json"], "cue_end_sample")? {
        return Err("holds must cover the complete unchanged recording exactly".into());
    }
    let mut language = vec![];
    for row in edited {
        let mut event = old_language
            .iter()
            .find(|e| e["semantic_id"] == row["semantic_id"])
            .ok_or("missing native language event")?
            .clone();
        if !event["picture_binding_override"].is_null() {
            event["picture_binding_override"] = row["picture_binding"].clone();
        }
        language.push(event);
    }
    let mut slot_assets: BTreeMap<String, Value> = BTreeMap::new();
    for (row, e) in edited.iter().zip(&language) {
        let slot = text(e, "picture_slot_id")?;
        let desired = &docs["scene-bindings.json"]["assets"][text(row, "picture_binding")?];
        if let Some(previous) = slot_assets.get(slot)
            && ["logical_id", "cache_uri", "sha256"]
                .iter()
                .any(|key| previous[*key] != desired[*key])
        {
            return Err("one selected picture slot cannot bind different edited assets".into());
        }
        slot_assets.insert(slot.to_owned(), desired.clone());
    }
    let mut graph_events = vec![];
    if let Some(existing) = docs["selected-graph.json"]["events"].as_array() {
        let mut start = 0u64;
        for (row, e) in edited.iter().zip(&language) {
            let event_id = text(e, "event_id")?;
            let mut event = existing
                .iter()
                .find(|x| x["event_id"] == event_id)
                .ok_or("missing native graph event")?
                .clone();
            let end = start + integer(row, "frames")? * spf;
            event["phrase_start_seconds"] =
                json!(start as f64 / integer(&docs["alignment.json"], "sample_rate")? as f64);
            event["phrase_end_seconds"] =
                json!(end as f64 / integer(&docs["alignment.json"], "sample_rate")? as f64);
            let asset = &docs["scene-bindings.json"]["assets"][text(row, "picture_binding")?];
            for key in ["logical_id", "sha256"] {
                event["picture"][key] = asset[key].clone();
            }
            graph_events.push(event);
            start = end;
        }
    }
    docs.get_mut("scene.json").ok_or("scene")?["shared_events"] = json!(events);
    docs.get_mut("scene.json").ok_or("scene")?["language_event_bindings"][&lang] = json!(language);
    docs.get_mut("alignment.json").ok_or("alignment")?["semantic_markers"] = Value::Object(markers);
    let alignment_bytes = serde_json::to_vec(&docs["alignment.json"])?;
    let ah = hash(&alignment_bytes);
    let assets = docs.get_mut("scene-bindings.json").ok_or("bindings")?["assets"]
        .as_object_mut()
        .ok_or("assets")?;
    let old_hash = text(&hashes, "alignment.json")?;
    let mut rebound = 0;
    for asset in assets.values_mut() {
        if asset["sha256"] == old_hash {
            asset["sha256"] = json!(ah);
            asset["bytes"] = json!(alignment_bytes.len());
            asset["cache_uri"] = json!(format!("cache://sha256/{ah}"));
            rebound += 1;
        }
    }
    if rebound != 1 {
        return Err("alignment binding not uniquely found".into());
    }
    let graph = docs.get_mut("selected-graph.json").ok_or("graph")?;
    if !graph_events.is_empty() {
        graph["events"] = json!(graph_events);
    }
    if let Some(nodes) = graph["nodes"].as_array_mut() {
        if nodes.len() != 1 {
            return Err("unsupported graph scope: requires one scene node".into());
        }
        nodes[0]["events"] = json!(
            language
                .iter()
                .map(|e| e["event_id"].clone())
                .collect::<Vec<_>>()
        );
        let retained_slots = slot_assets.keys().cloned().collect::<BTreeSet<_>>();
        let picture_slots = old_language
            .iter()
            .filter_map(|e| e["picture_slot_id"].as_str())
            .collect::<BTreeSet<_>>();
        nodes[0]["slots"]
            .as_array_mut()
            .ok_or("node slots")?
            .retain(|s| {
                !picture_slots.contains(s.as_str().unwrap_or(""))
                    || retained_slots.contains(s.as_str().unwrap_or(""))
            });
    }
    let slots = graph["slots"].as_array_mut().ok_or("graph slots")?;
    slots.retain(|s| {
        s["lane"] != "picture" || slot_assets.contains_key(s["slot_id"].as_str().unwrap_or(""))
    });
    let mut typed: reel_assembly::Graph = serde_json::from_value(graph.clone())?;
    let mut revisions = Vec::new();
    for slot in &typed.slots {
        if let Some(asset) = slot_assets.get(&slot.slot_id) {
            let current = slot
                .revisions
                .iter()
                .find(|revision| Some(&revision.revision_id) == slot.selected_revision_id.as_ref())
                .ok_or("missing selected picture revision")?;
            let desired = reel_assembly::Asset {
                logical_id: text(asset, "logical_id")?.into(),
                cache_uri: text(asset, "cache_uri")?.into(),
                sha256: text(asset, "sha256")?.into(),
            };
            if current.asset.logical_id != desired.logical_id
                || current.asset.sha256 != desired.sha256
                || current.asset.cache_uri != desired.cache_uri
            {
                let id = hash(&serde_json::to_vec(
                    &json!({"source_lock":typed.lock,"slot":slot.slot_id,"asset":desired}),
                )?);
                revisions.push(reel_assembly::SlotRevisionSelection {
                    slot_id: slot.slot_id.clone(),
                    revision: reel_assembly::Revision {
                        revision_id: format!("cel-edit-{id}"),
                        supersedes: Some(current.revision_id.clone()),
                        asset: desired,
                    },
                });
            }
        }
    }
    if revisions.is_empty() {
        typed.lock = reel_assembly::graph_lock(&typed, &typed.lock.logical_id)?;
    } else {
        typed = reel_assembly::append_selected_revisions(
            &typed,
            &reel_assembly::SlotRevisionBatchRequest {
                schema: reel_assembly::REVISION_BATCH_REQUEST_SCHEMA.into(),
                next_lock_logical_id: typed.lock.logical_id.clone(),
                revisions,
            },
        )?;
    }
    *graph = serde_json::to_value(typed)?;
    let lock = graph["lock"].clone();
    docs.get_mut("selected-pointer.json").ok_or("pointer")?["selected_lock"] = lock;
    project.write_draft(out, &docs, &r)?;
    println!(
        "Native draft saved; no selection or approval changed: {}",
        out.display()
    );
    Ok(())
}
fn main() -> Result<()> {
    let a: Vec<String> = env::args().collect();
    match a.get(1).map(String::as_str){Some("export") if a.len()==3=>export(Path::new(&a[2])),Some("apply") if a.len()==5=>apply(Path::new(&a[2]),Path::new(&a[3]),Path::new(&a[4])),_=>Err("usage: reel-cel-sorter export <spec.json> | apply <native-project> <edit.json> <new-draft-dir>".into())}
}

#[cfg(test)]
mod tests {
    use super::*;
    fn asset(body: &[u8]) -> Value {
        let sha256 = hash(body);
        json!({"logical_id":format!("synthetic/{sha256}"),"sha256":sha256,"bytes":body.len(),"cache_uri":format!("cache://sha256/{sha256}"),"selection_state":"selected-private-production"})
    }
    fn graph_asset(body: &[u8]) -> Value {
        let value = asset(body);
        json!({"logical_id":value["logical_id"],"cache_uri":value["cache_uri"],"sha256":value["sha256"]})
    }
    fn save_graph(project: &Path, mut graph: Value) {
        let typed: reel_assembly::Graph = serde_json::from_value(graph.clone()).unwrap();
        graph["lock"] = serde_json::to_value(
            reel_assembly::graph_lock(&typed, &typed.lock.logical_id).unwrap(),
        )
        .unwrap();
        let pointer = json!({"schema":reel_assembly::POINTER_SCHEMA,"logical_id":"synthetic-current","selected_lock":graph["lock"]});
        fs::write(
            project.join("selected-graph.json"),
            serde_json::to_vec(&graph).unwrap(),
        )
        .unwrap();
        fs::write(
            project.join("selected-pointer.json"),
            serde_json::to_vec(&pointer).unwrap(),
        )
        .unwrap();
    }
    fn media_fixture(project: &Path) -> Value {
        fixture(project);
        let mut bindings = read(&project.join("scene-bindings.json")).unwrap();
        bindings["assets"]["one"] = asset(b"cell");
        bindings["assets"]["two"] = asset(b"cell");
        fs::write(
            project.join("scene-bindings.json"),
            serde_json::to_vec(&bindings).unwrap(),
        )
        .unwrap();
        let mut graph = read(&project.join("selected-graph.json")).unwrap();
        for slot in graph["slots"].as_array_mut().unwrap() {
            slot["revisions"][0]["asset"] = graph_asset(b"cell");
        }
        for event in graph["events"].as_array_mut().unwrap() {
            event["picture"] =
                json!({"logical_id":asset(b"cell")["logical_id"],"sha256":hash(b"cell")});
        }
        save_graph(project, graph);
        json!({"project":project,"cache_roots":[],"movie":asset(b"clip"),"max_export_copy_bytes":8})
    }
    #[test]
    fn export_refuses_overbudget_before_missing_media_lookup_or_output_creation() {
        let d = tempfile::tempdir().unwrap();
        let mut spec = media_fixture(&d.path().join("source"));
        spec["output"] = json!(d.path().join("output"));
        spec["max_export_copy_bytes"] = json!(7);
        let path = d.path().join("spec.json");
        write(&path, &spec).unwrap();
        assert!(
            export(&path)
                .unwrap_err()
                .to_string()
                .contains("budget exceeded")
        );
        assert!(!d.path().join("output").exists());
    }
    #[test]
    fn repeated_cels_resolve_once_per_identity_and_copy_destinations_are_accounted() {
        let d = tempfile::tempdir().unwrap();
        let spec = media_fixture(&d.path().join("source"));
        let (docs, _) = source(&d.path().join("source")).unwrap();
        let repeated = json!(
            (0..1000)
                .map(|i| json!({"picture_binding":if i%2==0 {"one"} else {"two"}}))
                .collect::<Vec<_>>()
        );
        let plan = preflight(&docs, &repeated, &spec).unwrap();
        let mut calls = BTreeMap::new();
        let resolved = resolve_plan(&plan, |asset| {
            *calls.entry(text(asset, "sha256")?.to_owned()).or_insert(0) += 1;
            Ok(PathBuf::from("synthetic"))
        })
        .unwrap();
        assert_eq!(calls.len(), 2);
        assert!(calls.values().all(|n| *n == 1));
        assert_eq!(resolved.len(), 2);
        assert_eq!(plan.pictures.len(), 1);
        assert_eq!(plan.images.len(), 2);
        let mut same_movie = spec.clone();
        same_movie["movie"] = asset(b"cell");
        same_movie["max_export_copy_bytes"] = json!(7);
        assert!(preflight(&docs, &repeated, &same_movie).is_err());
        same_movie["max_export_copy_bytes"] = json!(8);
        assert!(preflight(&docs, &repeated, &same_movie).is_ok());
        same_movie["movie"]["bytes"] = json!(3);
        assert!(
            preflight(&docs, &repeated, &same_movie)
                .err()
                .unwrap()
                .to_string()
                .contains("conflicting sizes")
        );
    }
    #[test]
    fn export_deduplicates_images_without_creating_writable_canonical_aliases() {
        let d = tempfile::tempdir().unwrap();
        let mut spec = media_fixture(&d.path().join("source"));
        let cache = d.path().join("cache");
        for body in [b"cell", b"clip"] {
            let sha256 = hash(body);
            let directory = cache.join("objects/sha256").join(&sha256[..2]);
            fs::create_dir_all(&directory).unwrap();
            fs::write(directory.join(sha256), body).unwrap();
        }
        let out = d.path().join("output");
        spec["output"] = json!(out);
        spec["cache_roots"] = json!([cache]);
        let path = d.path().join("spec.json");
        write(&path, &spec).unwrap();
        export(&path).unwrap();
        assert_eq!(fs::read_dir(out.join("media")).unwrap().count(), 1);
        fs::write(out.join(format!("media/{}.png", hash(b"cell"))), b"edit").unwrap();
        assert_eq!(
            fs::read(
                cache
                    .join("objects/sha256")
                    .join(&hash(b"cell")[..2])
                    .join(hash(b"cell"))
            )
            .unwrap(),
            b"cell"
        );
        assert_eq!(fs::read(out.join("performance.mp4")).unwrap(), b"clip");
        assert!(export(&path).is_err());
    }
    #[test]
    fn grown_source_is_refused_before_copy_destination_creation() {
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("source");
        let destination = d.path().join("destination");
        fs::write(&source, b"grown source").unwrap();
        assert!(link(&source, &destination, &hash(b"cell"), 4).is_err());
        assert!(!destination.exists());
    }
    #[test]
    fn copied_bytes_are_checked_against_expected_identity() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("source");
        fs::write(&p, b"evil").unwrap();
        assert!(
            link(&p, &d.path().join("target"), &hash(b"good"), 4)
                .unwrap_err()
                .to_string()
                .contains("hash/size")
        );
    }
    fn fixture(project: &Path) {
        fs::create_dir_all(project).unwrap();
        for name in FILES {
            write(&project.join(name), &json!({"unknown":{"preserve":true}})).unwrap();
        }
        let alignment = json!({"schema":"reel.scene-native-alignment.v1","language":"es","cue_id":"es-cue","selected_take_sha256":hash(b"voice"),"sample_rate":48,"cue_end_sample":96,"semantic_markers":{"a":0,"b":48}});
        let ab = serde_json::to_vec(&alignment).unwrap();
        let ah = hash(&ab);
        let scene = json!({"schema":"reel.scene-authoring.v2","scene_id":"synthetic","episode_id":"synthetic-episode","authoring_state":"ready-for-private-build","source_authority_id":"synthetic-authority","canonical_cue_ids":["cue"],
            "languages":{"es":{"cues":[{"cue_id":"es-cue","source_id":"cue","exact_text_sha256":hash(b"text"),"narration_slot_id":"narration","take_binding":"take","phrase_alignment_binding":"clock"}]}},
            "shared_events":[{"semantic_id":"a","canonical_cue_id":"cue","picture_binding":"one","score":{"disposition":"silence"}},{"semantic_id":"b","canonical_cue_id":"cue","picture_binding":"two","score":{"disposition":"silence"}}],
            "language_event_bindings":{"es":[{"semantic_id":"a","event_id":"event-a","cue_id":"es-cue","semantic_trigger_id":"trigger-a","picture_slot_id":"p1"},{"semantic_id":"b","event_id":"event-b","cue_id":"es-cue","semantic_trigger_id":"trigger-b","picture_slot_id":"p2"}]},
            "presentation":{"role":"synthetic","template_id":"synthetic-template","content":{"unknown":{"preserve":true}}}});
        let mut clock = asset(&ab);
        clock["logical_id"] = json!("clock");
        assert_eq!(clock["sha256"], ah);
        let bindings = json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"synthetic","assets":{"clock":clock,"take":asset(b"voice"),"one":asset(b"first"),"two":asset(b"second")}});
        let graph = json!({"schema":reel_assembly::GRAPH_SCHEMA,"nodes":[{"id":"synthetic","slots":["p1","p2"],"events":["event-a","event-b"]}],
            "events":[{"event_id":"event-a","scene_id":"synthetic","language":"es","narration":{"logical_id":asset(b"voice")["logical_id"],"sha256":hash(b"voice")},"picture":{"logical_id":asset(b"first")["logical_id"],"sha256":hash(b"first")},"phrase_start_seconds":0.0,"phrase_end_seconds":1.0},
                {"event_id":"event-b","scene_id":"synthetic","language":"es","narration":{"logical_id":asset(b"voice")["logical_id"],"sha256":hash(b"voice")},"picture":{"logical_id":asset(b"second")["logical_id"],"sha256":hash(b"second")},"phrase_start_seconds":1.0,"phrase_end_seconds":2.0}],
            "lock":{"logical_id":"draft","sha256":hash(b"old")},"slots":[{"slot_id":"p1","beat_id":"cue","lane":"picture","disposition":"selected","selected_revision_id":"v1","revisions":[{"revision_id":"v1","asset":graph_asset(b"first")}]},
                {"slot_id":"p2","beat_id":"cue","lane":"picture","disposition":"selected","selected_revision_id":"v1","revisions":[{"revision_id":"v1","asset":graph_asset(b"second")}]}]});
        let graph_copy = graph.clone();
        for (name, v) in [
            ("scene.json", scene),
            ("scene-bindings.json", bindings),
            ("alignment.json", alignment),
            ("selected-graph.json", graph),
            (
                "profile.json",
                json!({"frame_rate_numerator":24,"frame_rate_denominator":1}),
            ),
        ] {
            fs::write(project.join(name), serde_json::to_vec(&v).unwrap()).unwrap();
        }
        save_graph(project, graph_copy);
    }
    fn override_fixture(project: &Path) {
        fixture(project);
        let mut scene = read(&project.join("scene.json")).unwrap();
        scene["language_event_bindings"]["es"][0]["picture_binding_override"] = json!("alternate");
        let mut bindings = read(&project.join("scene-bindings.json")).unwrap();
        bindings["assets"]["alternate"] = asset(b"alternate picture");
        let mut graph = read(&project.join("selected-graph.json")).unwrap();
        graph["slots"][0]["revisions"][0]["asset"] = graph_asset(b"alternate picture");
        graph["events"][0]["picture"] = json!({"logical_id":asset(b"alternate picture")["logical_id"],"sha256":hash(b"alternate picture")});
        for (name, doc) in [
            ("scene.json", scene),
            ("scene-bindings.json", bindings),
            ("selected-graph.json", graph),
        ] {
            fs::write(project.join(name), serde_json::to_vec(&doc).unwrap()).unwrap();
        }
        let graph = read(&project.join("selected-graph.json")).unwrap();
        save_graph(project, graph);
    }
    fn selected_hash(docs: &BTreeMap<String, Value>, slot_id: &str) -> String {
        let graph = native_graph(docs).unwrap();
        let slot = graph
            .slots
            .iter()
            .find(|slot| slot.slot_id == slot_id)
            .unwrap();
        slot.revisions
            .iter()
            .find(|revision| Some(&revision.revision_id) == slot.selected_revision_id.as_ref())
            .unwrap()
            .asset
            .sha256
            .clone()
    }
    fn map_fixture(project: &Path, minimal: bool) -> PathBuf {
        let mut files = BTreeMap::new();
        for (index, role) in FILES.iter().enumerate() {
            let original = project.join(role);
            if minimal && index >= 6 {
                fs::remove_file(original).unwrap();
                continue;
            }
            let relative = format!("native/group-{}/control-{index}.json", index % 3);
            let destination = project.join(&relative);
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            fs::rename(original, destination).unwrap();
            files.insert((*role).to_owned(), relative);
        }
        let descriptor = project.join("editor-project.json");
        write(
            &descriptor,
            &json!({"schema":"reel.cel-sorter-project.v1","root":".","files":files}),
        )
        .unwrap();
        descriptor
    }
    #[test]
    fn mapped_six_control_native_draft_reopens_without_canonical_filenames() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("source");
        fixture(&p);
        let profile = map_fixture(&p, true);
        let project = Project::open(&profile).unwrap();
        let (docs, hashes) = source(&profile).unwrap();
        assert_eq!(docs.len(), 6);
        assert!(!p.join("scene.json").exists());
        let mut edited = rows(&docs).unwrap();
        edited[0]["frames"] = json!(12);
        edited[1]["frames"] = json!(36);
        let request = d.path().join("edit.json");
        write(&request,&json!({"schema":"reel.cel-sorter-edit.v2","expected_project":project.descriptor_sha256(),"expected_inputs":hashes,"scene_id":"synthetic","rows":edited})).unwrap();
        let out = d.path().join("candidate");
        apply(&profile, &request, &out).unwrap();
        assert!(!out.join("scene.json").exists());
        assert!(out.join("native/group-0/control-0.json").is_file());
        let (draft, _) = source(&out.join("cel-sorter-project.json")).unwrap();
        assert_eq!(rows(&draft).unwrap()[0]["frames"], 12);
        native_graph(&draft).unwrap();
        assert_eq!(source(&profile).unwrap().1, hashes);
    }
    #[test]
    fn mapped_edits_refuse_downgrade_or_changed_file_routes_with_identical_content() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("source");
        fixture(&p);
        let profile = map_fixture(&p, true);
        let project = Project::open(&profile).unwrap();
        let (docs, hashes) = source(&profile).unwrap();
        let mut request = json!({"schema":"reel.cel-sorter-edit.v1","expected_inputs":hashes,"scene_id":"synthetic","rows":rows(&docs).unwrap()});
        let path = d.path().join("edit.json");
        write(&path, &request).unwrap();
        assert!(apply(&profile, &path, &d.path().join("downgraded")).is_err());
        request["schema"] = json!("reel.cel-sorter-edit.v2");
        request["expected_project"] = json!(project.descriptor_sha256());
        fs::write(&path, serde_json::to_vec(&request).unwrap()).unwrap();
        let mut descriptor = read(&profile).unwrap();
        let previous = descriptor["files"]["scene.json"].as_str().unwrap();
        fs::copy(p.join(previous), p.join("identical-scene.json")).unwrap();
        descriptor["files"]["scene.json"] = json!("identical-scene.json");
        fs::write(&profile, serde_json::to_vec(&descriptor).unwrap()).unwrap();
        assert_eq!(source(&profile).unwrap().1, hashes);
        let out = d.path().join("stale");
        assert!(
            apply(&profile, &path, &out)
                .unwrap_err()
                .to_string()
                .contains("stale")
        );
        assert!(!out.exists());
    }
    #[test]
    fn mapped_export_emits_v2_and_optional_context_keeps_its_relative_references() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("source");
        let mut spec = media_fixture(&p);
        let profile = map_fixture(&p, false);
        let mut descriptor = read(&profile).unwrap();
        let compile_path = p.join(descriptor["files"]["compile.json"].as_str().unwrap());
        let compile = json!({"scene":descriptor["files"]["scene.json"],"scene_bindings":descriptor["files"]["scene-bindings.json"],"custom":{"preserve":true}});
        fs::write(compile_path, serde_json::to_vec(&compile).unwrap()).unwrap();
        let cache = d.path().join("cache");
        for body in [b"cell", b"clip"] {
            let h = hash(body);
            let dir = cache.join("objects/sha256").join(&h[..2]);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join(h), body).unwrap();
        }
        let editor = d.path().join("editor");
        spec["project"] = json!(profile);
        spec["output"] = json!(editor);
        spec["cache_roots"] = json!([cache]);
        let spec_path = d.path().join("export.json");
        write(&spec_path, &spec).unwrap();
        export(&spec_path).unwrap();
        let session = read(&editor.join("session.json")).unwrap();
        assert_eq!(session["schema"], "reel.cel-sorter-session.v2");
        assert_eq!(
            session["expected_project"],
            Project::open(&profile)
                .unwrap()
                .descriptor_sha256()
                .unwrap()
        );
        let request = d.path().join("edit.json");
        write(&request,&json!({"schema":"reel.cel-sorter-edit.v2","expected_project":session["expected_project"],"expected_inputs":session["expected_inputs"],"scene_id":session["scene_id"],"rows":session["rows"]})).unwrap();
        let out = d.path().join("candidate");
        apply(&profile, &request, &out).unwrap();
        let (draft, _) = source(&out.join("cel-sorter-project.json")).unwrap();
        assert_eq!(draft["compile.json"], compile);
        assert!(
            out.join(draft["compile.json"]["scene"].as_str().unwrap())
                .exists()
        );
        descriptor["root"] = json!(".");
        assert_eq!(
            read(&out.join("cel-sorter-project.json")).unwrap(),
            descriptor
        );
    }
    #[test]
    fn effective_override_noop_preserves_shared_default_and_native_graph_identity() {
        let d = tempfile::tempdir().unwrap();
        let project = d.path().join("source");
        override_fixture(&project);
        let (docs, hashes) = source(&project).unwrap();
        let original = rows(&docs).unwrap();
        assert_eq!(original[0]["picture_binding"], "alternate");
        let request = d.path().join("edit.json");
        write(&request,&json!({"schema":"reel.cel-sorter-edit.v1","scene_id":"synthetic","expected_inputs":hashes,"rows":original})).unwrap();
        let out = d.path().join("draft");
        apply(&project, &request, &out).unwrap();
        let (draft, _) = source(&out).unwrap();
        assert_eq!(draft["scene.json"], docs["scene.json"]);
        assert_eq!(rows(&draft).unwrap(), original);
        assert_eq!(selected_hash(&draft, "p1"), hash(b"alternate picture"));
        assert_eq!(
            draft["selected-graph.json"]["events"][0]["picture"]["sha256"],
            hash(b"alternate picture")
        );
        assert_eq!(source(&project).unwrap().1, hashes);
    }
    #[test]
    fn override_substitution_stays_local_and_matches_native_materialization() {
        let d = tempfile::tempdir().unwrap();
        let project = d.path().join("source");
        override_fixture(&project);
        let (docs, hashes) = source(&project).unwrap();
        let mut edited = rows(&docs).unwrap();
        edited[0]["picture_binding"] = json!("two");
        let request = d.path().join("edit.json");
        write(&request,&json!({"schema":"reel.cel-sorter-edit.v1","scene_id":"synthetic","expected_inputs":hashes,"rows":edited})).unwrap();
        let out = d.path().join("draft");
        apply(&project, &request, &out).unwrap();
        let (draft, _) = source(&out).unwrap();
        assert_eq!(
            draft["scene.json"]["shared_events"][0]["picture_binding"],
            "one"
        );
        assert_eq!(
            draft["scene.json"]["language_event_bindings"]["es"][0]["picture_binding_override"],
            "two"
        );
        assert_eq!(rows(&draft).unwrap()[0]["picture_binding"], "two");
        assert_eq!(selected_hash(&draft, "p1"), hash(b"second"));
        assert_eq!(
            draft["selected-graph.json"]["events"][0]["picture"]["sha256"],
            hash(b"second")
        );
        assert_eq!(source(&project).unwrap().1, hashes);
        let graph = native_graph(&draft).unwrap();
        let slot = graph
            .slots
            .iter()
            .find(|slot| slot.slot_id == "p1")
            .unwrap();
        assert_eq!(slot.revisions.len(), 2);
        assert_eq!(slot.revisions[0].asset.sha256, hash(b"alternate picture"));
        assert_eq!(slot.revisions[1].supersedes.as_deref(), Some("v1"));
    }
    #[test]
    fn timing_only_edit_uses_native_full_graph_lock_without_changing_slot_revisions() {
        let d = tempfile::tempdir().unwrap();
        let project = d.path().join("source");
        fixture(&project);
        let (docs, hashes) = source(&project).unwrap();
        let before = native_graph(&docs).unwrap();
        let mut edited = rows(&docs).unwrap();
        edited[0]["frames"] = json!(12);
        edited[1]["frames"] = json!(36);
        let request = d.path().join("edit.json");
        write(&request,&json!({"schema":"reel.cel-sorter-edit.v1","scene_id":"synthetic","expected_inputs":hashes,"rows":edited})).unwrap();
        let out = d.path().join("draft");
        apply(&project, &request, &out).unwrap();
        let (draft, _) = source(&out).unwrap();
        let after = native_graph(&draft).unwrap();
        assert_eq!(
            serde_json::to_value(&before.slots).unwrap(),
            serde_json::to_value(&after.slots).unwrap()
        );
        assert_ne!(before.lock.sha256, after.lock.sha256);
        assert_eq!(
            after.lock.sha256,
            reel_assembly::graph_lock(&after, &after.lock.logical_id)
                .unwrap()
                .sha256
        );
        assert_eq!(after.events[0].phrase_end_seconds, 0.5);
        assert_eq!(source(&project).unwrap().1, hashes);
    }
    #[test]
    fn mismatched_native_graph_pointer_or_effective_picture_rejects() {
        let d = tempfile::tempdir().unwrap();
        let project = d.path().join("source");
        fixture(&project);
        let (docs, _) = source(&project).unwrap();
        for case in 0..3 {
            let mut changed = docs.clone();
            match case {
                0 => {
                    changed.get_mut("selected-pointer.json").unwrap()["selected_lock"]["sha256"] =
                        json!(hash(b"different"))
                }
                1 => {
                    changed.get_mut("selected-graph.json").unwrap()["events"][0]["picture"]["sha256"] =
                        json!(hash(b"different"))
                }
                _ => {
                    changed.get_mut("selected-graph.json").unwrap()["nodes"][0]["slots"] =
                        json!(["missing"])
                }
            }
            assert!(rows(&changed).is_err(), "case {case}");
        }
    }
    #[test]
    fn shared_picture_slot_cannot_select_two_different_edited_assets() {
        let d = tempfile::tempdir().unwrap();
        let project = d.path().join("source");
        fixture(&project);
        let (mut docs, _) = source(&project).unwrap();
        let scene = docs.get_mut("scene.json").unwrap();
        let mut third = scene["shared_events"][1].clone();
        third["semantic_id"] = json!("c");
        scene["shared_events"][1]["picture_binding"] = json!("one");
        scene["shared_events"].as_array_mut().unwrap().push(third);
        let mut binding = scene["language_event_bindings"]["es"][1].clone();
        binding["semantic_id"] = json!("c");
        binding["event_id"] = json!("event-c");
        binding["picture_slot_id"] = json!("p3");
        scene["language_event_bindings"]["es"][1]["picture_slot_id"] = json!("p1");
        scene["language_event_bindings"]["es"]
            .as_array_mut()
            .unwrap()
            .push(binding);
        docs.get_mut("alignment.json").unwrap()["semantic_markers"] = json!({"a":0,"b":32,"c":64});
        let alignment = serde_json::to_vec(&docs["alignment.json"]).unwrap();
        docs.get_mut("scene-bindings.json").unwrap()["assets"]["clock"] = asset(&alignment);
        let mut graph = docs["selected-graph.json"].clone();
        let mut event = graph["events"][1].clone();
        event["event_id"] = json!("event-c");
        event["phrase_start_seconds"] = json!(64.0 / 48.0);
        graph["events"][0]["phrase_end_seconds"] = json!(32.0 / 48.0);
        graph["events"][1]["picture"] = graph["events"][0]["picture"].clone();
        graph["events"][1]["phrase_start_seconds"] = json!(32.0 / 48.0);
        graph["events"][1]["phrase_end_seconds"] = json!(64.0 / 48.0);
        graph["events"].as_array_mut().unwrap().push(event);
        graph["slots"][1]["slot_id"] = json!("p3");
        graph["nodes"][0]["slots"] = json!(["p1", "p3"]);
        graph["nodes"][0]["events"] = json!(["event-a", "event-b", "event-c"]);
        for name in ["scene.json", "alignment.json", "scene-bindings.json"] {
            fs::write(project.join(name), serde_json::to_vec(&docs[name]).unwrap()).unwrap();
        }
        save_graph(&project, graph);
        let (docs, hashes) = source(&project).unwrap();
        let mut edited = rows(&docs).unwrap();
        edited[0]["picture_binding"] = json!("two");
        let request = d.path().join("edit.json");
        write(&request,&json!({"schema":"reel.cel-sorter-edit.v1","scene_id":"synthetic","expected_inputs":hashes,"rows":edited})).unwrap();
        let out = d.path().join("draft");
        assert!(
            apply(&project, &request, &out)
                .unwrap_err()
                .to_string()
                .contains("one selected picture slot")
        );
        assert!(!out.exists());
        assert_eq!(source(&project).unwrap().1, hashes);
    }
    #[test]
    fn native_scope_unknown_fields_and_malformed_overrides_reject() {
        let d = tempfile::tempdir().unwrap();
        let project = d.path().join("source");
        fixture(&project);
        let (docs, _) = source(&project).unwrap();
        for invalid in [json!("missing"), json!(""), json!(7)] {
            let mut changed = docs.clone();
            changed.get_mut("scene.json").unwrap()["language_event_bindings"]["es"][0]["picture_binding_override"] =
                invalid;
            assert!(rows(&changed).is_err());
        }
        let mut changed = docs.clone();
        changed.get_mut("scene.json").unwrap()["future_grammar"] = json!(true);
        assert!(rows(&changed).is_err());
        let mut changed = docs.clone();
        changed.get_mut("scene.json").unwrap()["shared_events"][0]["canonical_cue_id"] =
            json!("outside-source");
        assert!(rows(&changed).is_err());
    }
    #[test]
    fn native_alignment_identity_bounds_and_exact_binding_must_match() {
        let d = tempfile::tempdir().unwrap();
        let project = d.path().join("source");
        fixture(&project);
        let (docs, _) = source(&project).unwrap();
        for case in 0..3 {
            let mut changed = docs.clone();
            let alignment = changed.get_mut("alignment.json").unwrap();
            match case {
                0 => alignment["cue_id"] = json!("wrong"),
                1 => alignment["selected_take_sha256"] = json!(hash(b"wrong")),
                _ => alignment["semantic_markers"]["b"] = json!(96),
            }
            assert!(rows(&changed).is_err());
        }
        let mut alignment = fs::read(project.join("alignment.json")).unwrap();
        alignment.push(b'\n');
        fs::write(project.join("alignment.json"), &alignment).unwrap();
        assert!(
            source(&project)
                .err()
                .unwrap()
                .to_string()
                .contains("exact intake bytes")
        );
        let mut bindings = docs["scene-bindings.json"].clone();
        bindings["assets"]["clock"] = asset(&alignment);
        fs::write(
            project.join("scene-bindings.json"),
            serde_json::to_vec(&bindings).unwrap(),
        )
        .unwrap();
        let (updated, _) = source(&project).unwrap();
        assert_eq!(rows(&updated).unwrap(), rows(&docs).unwrap());
    }
    #[test]
    fn native_round_trip_and_rejections() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("source");
        fixture(&p);
        let (docs, hashes) = source(&p).unwrap();
        let old = rows(&docs).unwrap();
        let mut reordered = old.as_array().unwrap().clone();
        reordered.reverse();
        reordered[0]["picture_binding"] = json!("one");
        let request = d.path().join("edit.json");
        let mut edit = json!({"schema":"reel.cel-sorter-edit.v1","scene_id":"synthetic","expected_inputs":hashes,"rows":reordered});
        write(&request, &edit).unwrap();
        let out = d.path().join("draft");
        apply(&p, &request, &out).unwrap();
        let alignment = read(&out.join("alignment.json")).unwrap();
        assert_eq!(alignment["semantic_markers"], json!({"b":0,"a":48}));
        let scene = read(&out.join("scene.json")).unwrap();
        assert_eq!(
            scene["presentation"]["content"]["unknown"],
            json!({"preserve":true})
        );
        assert_eq!(
            read(&out.join("policy.json")).unwrap()["unknown"],
            json!({"preserve":true})
        );
        let bindings = read(&out.join("scene-bindings.json")).unwrap();
        assert_eq!(
            bindings["assets"]["clock"]["sha256"],
            hash(&fs::read(out.join("alignment.json")).unwrap())
        );
        assert_eq!(
            selected_hash(&source(&out).unwrap().0, "p2"),
            hash(b"first")
        );
        assert_eq!(source(&p).unwrap().1, hashes);
        assert!(apply(&p, &request, &out).is_err());
        edit["rows"][0]["frames"] = json!(25);
        fs::write(&request, serde_json::to_vec(&edit).unwrap()).unwrap();
        assert!(apply(&p, &request, &d.path().join("bad-duration")).is_err());
        edit["rows"][0]["frames"] = json!(24);
        edit["rows"][1]["semantic_id"] = edit["rows"][0]["semantic_id"].clone();
        fs::write(&request, serde_json::to_vec(&edit).unwrap()).unwrap();
        assert!(apply(&p, &request, &d.path().join("duplicate")).is_err());
        fs::write(p.join("episode.json"), b"{}").unwrap();
        assert!(
            apply(&p, &request, &d.path().join("stale"))
                .unwrap_err()
                .to_string()
                .contains("stale")
        );
    }
    #[test]
    fn invalid_cache_identity_fails_before_lookup() {
        assert!(resolve(&[], &json!({"sha256":"../x","cache_uri":"x","bytes":1})).is_err());
    }
    #[test]
    fn corrupt_first_tier_never_falls_back() {
        let d = tempfile::tempdir().unwrap();
        let h = hash(b"good");
        let a = json!({"sha256":h,"bytes":4,"cache_uri":format!("cache://sha256/{h}")});
        let roots = vec![d.path().join("e"), d.path().join("d")];
        for (root, b) in roots.iter().zip([b"bad!", b"good"]) {
            let p = root.join("objects/sha256").join(&h[..2]);
            fs::create_dir_all(&p).unwrap();
            fs::write(p.join(&h), b).unwrap();
        }
        assert!(
            resolve(&roots, &a)
                .unwrap_err()
                .to_string()
                .contains("hash mismatch")
        );
    }
}
