//! Explicit, offline editor export and optimistic native-draft application.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const FILES: &[&str] = &[
    "scene.json",
    "scene-bindings.json",
    "alignment.json",
    "selected-graph.json",
    "selected-pointer.json",
    "profile.json",
    "catalog.json",
    "episode.json",
    "policy.json",
    "season-bindings.json",
    "episode-bindings.json",
    "alignment-paths.json",
    "compile.json",
];
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
fn source(project: &Path) -> Result<(BTreeMap<String, Value>, Value)> {
    let mut docs: BTreeMap<String, Value> = BTreeMap::new();
    let mut hashes = serde_json::Map::new();
    for name in FILES {
        let b = bounded(&project.join(name))?;
        hashes.insert((*name).into(), json!(hash(&b)));
        docs.insert((*name).into(), serde_json::from_slice(&b)?);
    }
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
    Ok((docs, Value::Object(hashes)))
}
fn resolve(roots: &[PathBuf], asset: &Value) -> Result<PathBuf> {
    let h = text(asset, "sha256")?;
    if h.len() != 64
        || !h.bytes().all(|b| b.is_ascii_hexdigit())
        || asset["cache_uri"] != format!("cache://sha256/{h}")
    {
        return Err("invalid asset identity".into());
    }
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
        if metadata.len() != integer(asset, "bytes")? {
            return Err("cache byte mismatch".into());
        }
        let mut f = fs::File::open(&p)?;
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
fn link(source: &Path, target: &Path, expected_hash: &str) -> Result<()> {
    // Working editor files must never be writable aliases of canonical objects.
    let input = fs::File::open(source)?;
    let expected = input.metadata()?.len();
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)?;
    let mut reader = input.take(expected + 1);
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
fn rows(docs: &BTreeMap<String, Value>) -> Result<Value> {
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
    let language = docs["scene.json"]["language_event_bindings"][lang]
        .as_array()
        .ok_or("language events")?;
    if language
        .iter()
        .any(|e| !e["picture_binding_override"].is_null())
    {
        return Err(
            "picture binding overrides require native effective-binding editor support".into(),
        );
    }
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
        let binding = text(e, "picture_binding")?;
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
fn export(spec: &Path) -> Result<()> {
    let s = read(spec)?;
    let project = Path::new(text(&s, "project")?);
    let out = Path::new(text(&s, "output")?);
    if out.exists() {
        return Err("export exists; choose a new version".into());
    }
    let (docs, hashes) = source(project)?;
    let r = rows(&docs)?;
    let roots: Vec<PathBuf> = s["cache_roots"]
        .as_array()
        .ok_or("cache_roots required")?
        .iter()
        .map(|v| v.as_str().map(PathBuf::from).ok_or("invalid root"))
        .collect::<std::result::Result<_, _>>()?;
    let mut images = serde_json::Map::new();
    let mut originals = vec![];
    for row in r.as_array().ok_or("rows")? {
        let k = text(row, "picture_binding")?;
        let asset = &docs["scene-bindings.json"]["assets"][k];
        let h = text(asset, "sha256")?;
        let p = resolve(&roots, asset)?;
        images.insert(k.into(), json!(format!("media/{h}.png")));
        originals.push((p, h.to_owned()));
    }
    let movie = resolve(&roots, &s["movie"])?;
    let mut unique = BTreeMap::new();
    for (p, h) in &originals {
        unique.insert(h.clone(), fs::metadata(p)?.len());
    }
    unique.insert(
        text(&s["movie"], "sha256")?.to_owned(),
        integer(&s["movie"], "bytes")?,
    );
    let total = unique.values().try_fold(0u64, |sum, bytes| {
        sum.checked_add(*bytes).ok_or("export size overflow")
    })?;
    let budget = integer(&s, "max_export_copy_bytes")?;
    if budget > 1024 * 1024 * 1024 || total > budget {
        return Err("explicit editor-copy budget exceeded".into());
    }
    fs::create_dir_all(out.join("media"))?;
    for (p, h) in originals {
        let dest = out.join(format!("media/{h}.png"));
        if !dest.exists() {
            link(&p, &dest, &h)?;
        }
    }
    link(
        &movie,
        &out.join("performance.mp4"),
        text(&s["movie"], "sha256")?,
    )?;
    let data = json!({"schema":"reel.cel-sorter-session.v1","title":s["title"],"scene_id":docs["scene.json"]["scene_id"],"expected_inputs":hashes,"rows":r,"images":images,"sample_rate":docs["alignment.json"]["sample_rate"],"end_sample":docs["alignment.json"]["cue_end_sample"],"fps_numerator":docs["profile.json"]["frame_rate_numerator"],"fps_denominator":docs["profile.json"]["frame_rate_denominator"],"movie":"performance.mp4"});
    write(&out.join("session.json"), &data)?;
    fs::write(out.join("index.html"), include_str!("../cel_sorter.html"))?;
    fs::write(out.join("editor.js"), include_str!("../cel_sorter.js"))?;
    println!("{}", out.join("index.html").display());
    Ok(())
}
fn apply(project: &Path, request: &Path, out: &Path) -> Result<()> {
    let r = read(request)?;
    let (mut docs, hashes) = source(project)?;
    if r["schema"] != "reel.cel-sorter-edit.v1"
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
        e["picture_binding"] = json!(binding);
        events.push(e);
    }
    if total != integer(&docs["alignment.json"], "cue_end_sample")? {
        return Err("holds must cover the complete unchanged recording exactly".into());
    }
    let lang = text(&docs["alignment.json"], "language")?.to_owned();
    let old_language = docs["scene.json"]["language_event_bindings"][&lang]
        .as_array()
        .ok_or("missing language events")?
        .clone();
    let mut language = vec![];
    for row in edited {
        language.push(
            old_language
                .iter()
                .find(|e| e["semantic_id"] == row["semantic_id"])
                .ok_or("missing native language event")?
                .clone(),
        );
    }
    let mut slot_assets = BTreeMap::new();
    for (row, e) in edited.iter().zip(&language) {
        slot_assets.insert(
            text(e, "picture_slot_id")?.to_owned(),
            docs["scene-bindings.json"]["assets"][text(row, "picture_binding")?].clone(),
        );
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
    let old_hash = hash(&fs::read(project.join("alignment.json"))?);
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
    for slot in slots.iter_mut() {
        if let Some(asset) = slot_assets.get(slot["slot_id"].as_str().unwrap_or("")) {
            let selected = slot["selected_revision_id"].clone();
            let revision = slot["revisions"]
                .as_array_mut()
                .ok_or("revisions")?
                .iter_mut()
                .find(|x| x["revision_id"] == selected)
                .ok_or("selected revision")?;
            // Graph references have a smaller native contract than scoped bindings.
            // Preserve unknown revision/reference fields; update only identity keys.
            for key in ["logical_id", "cache_uri", "sha256"] {
                if !asset[key].is_null() {
                    revision["asset"][key] = asset[key].clone();
                }
            }
        }
    }
    let mut identity = graph.clone();
    identity
        .as_object_mut()
        .ok_or("graph object")?
        .remove("lock");
    let gh = hash(&serde_json::to_vec(&identity)?);
    graph["lock"]["sha256"] = json!(gh);
    let lock = graph["lock"].clone();
    docs.get_mut("selected-pointer.json").ok_or("pointer")?["selected_lock"] = lock;
    if out.exists() {
        return Err("draft destination exists".into());
    }
    fs::create_dir_all(out)?;
    for (name, doc) in docs {
        write(&out.join(name), &doc)?;
    }
    write(&out.join("edit-request.json"), &r)?;
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
    #[test]
    fn copied_bytes_are_checked_against_expected_identity() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("source");
        fs::write(&p, b"evil").unwrap();
        assert!(
            link(&p, &d.path().join("target"), &hash(b"good"))
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
        let alignment = json!({"schema":"reel.scene-native-alignment.v1","language":"es","sample_rate":48,"cue_end_sample":96,"semantic_markers":{"a":0,"b":48}});
        let ab = serde_json::to_vec(&alignment).unwrap();
        let ah = hash(&ab);
        let scene = json!({"schema":"reel.scene-authoring.v2","scene_id":"synthetic","canonical_cue_ids":["cue"],"languages":{"es":{}},"shared_events":[{"semantic_id":"a","picture_binding":"one","unknown":"keep"},{"semantic_id":"b","picture_binding":"two"}],"language_event_bindings":{"es":[{"semantic_id":"a","event_id":"event-a","picture_slot_id":"p1"},{"semantic_id":"b","event_id":"event-b","picture_slot_id":"p2"}]},"unknown":{"preserve":true}});
        let bindings = json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"synthetic","assets":{"clock":{"sha256":ah,"bytes":ab.len(),"cache_uri":format!("cache://sha256/{ah}")},"one":{"sha256":"first"},"two":{"sha256":"second"}}});
        let graph = json!({"nodes":[{"id":"synthetic","slots":["p1","p2"],"events":["event-a","event-b"]}],"events":[{"event_id":"event-a","picture":{}},{"event_id":"event-b","picture":{}}],"lock":{"logical_id":"draft","sha256":"old"},"slots":[{"slot_id":"p1","lane":"picture","selected_revision_id":"v1","revisions":[{"revision_id":"v1","asset":{"sha256":"first"}}]},{"slot_id":"p2","lane":"picture","selected_revision_id":"v1","revisions":[{"revision_id":"v1","asset":{"sha256":"second"}}]}]});
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
        assert_eq!(scene["unknown"], json!({"preserve":true}));
        assert_eq!(scene["shared_events"][1]["unknown"], "keep");
        let bindings = read(&out.join("scene-bindings.json")).unwrap();
        assert_eq!(
            bindings["assets"]["clock"]["sha256"],
            hash(&fs::read(out.join("alignment.json")).unwrap())
        );
        assert_eq!(
            read(&out.join("selected-graph.json")).unwrap()["slots"][1]["revisions"][0]["asset"]["sha256"],
            "first"
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
