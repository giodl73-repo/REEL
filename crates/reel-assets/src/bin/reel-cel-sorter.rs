//! Explicit, offline editor export and optimistic native-draft application.
use reel_assets::cel_source::{editor::load, project::Project, sequence::Sequence};
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
#[cfg(test)]
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
#[cfg(test)]
fn native_graph(docs: &BTreeMap<String, Value>) -> Result<reel_assembly::Graph> {
    let graph = serde_json::from_value(docs["selected-graph.json"].clone())?;
    let pointer = serde_json::from_value(docs["selected-pointer.json"].clone())?;
    reel_assembly::validate_selected_graph(&pointer, &graph)?;
    Ok(graph)
}
#[cfg(test)]
fn rows(docs: &BTreeMap<String, Value>) -> Result<Value> {
    Ok(Sequence::new(docs, false)?.rows())
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
    export_mode(spec, true)
}
fn export_mode(spec: &Path, media: bool) -> Result<()> {
    let s = read(spec)?;
    if !media
        && s.as_object()
            .ok_or("export spec must be an object")?
            .keys()
            .any(|key| !["project", "output", "title"].contains(&key.as_str()))
    {
        return Err("controls-only spec accepts project, output and title only".into());
    }
    let project = Path::new(text(&s, "project")?);
    let out = Path::new(text(&s, "output")?);
    if out.exists() {
        return Err("export exists; choose a new version".into());
    }
    let project = Project::open(project)?;
    let (docs, hashes) = load(&project)?;
    let sequence = Sequence::for_project(&docs, &project)?;
    let r = sequence.rows();
    let (images, movie) = if media {
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
        (plan.images, json!("performance.mp4"))
    } else {
        fs::create_dir_all(out)?;
        (serde_json::Map::new(), Value::Null)
    };
    let mut data = json!({"schema":project.session_schema(),"title":s["title"],"scene_id":docs["scene.json"]["scene_id"],"expected_inputs":hashes,"rows":r,"images":images,"fps_numerator":docs["profile.json"]["frame_rate_numerator"],"fps_denominator":docs["profile.json"]["frame_rate_denominator"],"movie":movie});
    if !media {
        data["media_mode"] = json!("controls-only");
    }
    data.as_object_mut()
        .ok_or("session")?
        .extend(sequence.session_clock().as_object().ok_or("clock")?.clone());
    if let Some(hash) = project.descriptor_sha256() {
        data["expected_project"] = json!(hash);
    }
    if let Some(language) = project.language() {
        data["language"] = json!(language);
    }
    write(&out.join("session.json"), &data)?;
    fs::write(out.join("index.html"), include_str!("../cel_sorter.html"))?;
    fs::write(out.join("editor.js"), include_str!("../cel_sorter.js"))?;
    println!("{}", out.join("index.html").display());
    Ok(())
}
fn apply(project: &Path, request: &Path, out: &Path) -> Result<()> {
    reel_assets::cel_source::editor::apply(project, request, out)?;
    println!(
        "Native candidate saved; source selection and approvals unchanged: {}",
        out.display()
    );
    Ok(())
}
fn inspect_project(input: &Path, out: &Path) -> Result<()> {
    let project = Project::open(input)?;
    let (docs, hashes) = load(&project)?;
    let sequence = Sequence::for_project(&docs, &project)?;
    let rows = sequence.rows();
    let count = rows.as_array().ok_or("rows")?.len();
    let mut request = json!({"schema":project.session_schema().replace("session","edit"),"scene_id":docs["scene.json"]["scene_id"],"expected_inputs":hashes,"rows":rows,"notes":"","human_approval":false});
    if let Some(hash) = project.descriptor_sha256() {
        request["expected_project"] = json!(hash);
    }
    if let Some(language) = project.language() {
        request["language"] = json!(language);
    }
    let report = json!({"schema":"reel.cel-sorter-inspection.v1","edit_request":request,"clock":sequence.session_clock(),"media_resolved":false});
    if serde_json::to_vec(&report)?.len() > 2 * 1024 * 1024 {
        return Err("Private inspection exceeds control budget".into());
    }
    write(out, &report)?;
    println!(
        "{}",
        json!({"cels":count,"language_scoped":project.language().is_some(),"protected_languages":sequence.other_cues.iter().map(|cue|&cue.language).collect::<BTreeSet<_>>().len(),"media_resolved":false})
    );
    Ok(())
}
fn main() -> Result<()> {
    let a: Vec<String> = env::args().collect();
    match a.get(1).map(String::as_str){Some("inspect") if a.len()==4=>inspect_project(Path::new(&a[2]),Path::new(&a[3])),Some("export") if a.len()==3=>export(Path::new(&a[2])),Some("export-controls") if a.len()==3=>export_mode(Path::new(&a[2]),false),Some("apply") if a.len()==5=>apply(Path::new(&a[2]),Path::new(&a[3]),Path::new(&a[4])),_=>Err("usage: reel-cel-sorter inspect <project> <new-private-report> | export|export-controls <spec.json> | apply <native-project> <edit.json> <new-draft-dir>".into())}
}

#[cfg(test)]
#[path = "../../tests/fixtures/cel_native.rs"]
mod cel_fixtures;

#[cfg(test)]
mod tests {
    use super::cel_fixtures::{asset, fixture, graph_asset, multi_fixture, save_graph};
    use super::*;
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
        binding["semantic_trigger_id"] = json!("c");
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

    // Two exact recording clocks, off-frame cuts and distinct trigger/semantic IDs.
    #[test]
    fn multi_cue_sample_edits_reopen_with_exact_native_local_clocks_and_provenance() {
        use reel_assets::cel_source::project::alignment_role;
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("source");
        let descriptor = multi_fixture(&source);
        let project = Project::open(&descriptor).unwrap();
        let (docs, hashes) = load(&project).unwrap();
        let sequence = Sequence::new(&docs, true).unwrap();
        assert_eq!(sequence.rows()[0]["samples"], 47);
        assert_eq!(sequence.rows()[2]["sample_rate"], 44);
        assert_eq!(sequence.session_clock()["cues"][1]["start_seconds"], 2.0);
        let mut edited = sequence.rows().as_array().unwrap().clone();
        edited.swap(0, 1);
        edited[0]["samples"] = json!(31);
        edited[1]["samples"] = json!(65);
        edited[0]["comment"] = json!("Keep this synthetic review note");
        edited.remove(2);
        edited[2]["samples"] = json!(88);
        let request = d.path().join("request.json");
        let out = d.path().join("draft");
        write(&request,&json!({"schema":"reel.cel-sorter-edit.v3","scene_id":"synthetic","expected_project":project.descriptor_sha256(),"expected_inputs":hashes,"rows":edited})).unwrap();
        apply(&descriptor, &request, &out).unwrap();
        let next = Project::open(&out.join("cel-sorter-project.json")).unwrap();
        let (draft, _) = load(&next).unwrap();
        let rows = Sequence::new(&draft, true).unwrap().rows();
        assert_eq!(rows.as_array().unwrap().len(), 3);
        assert_eq!(rows[0]["semantic_id"], "b");
        assert_eq!(rows[2]["samples"], 88);
        assert_eq!(
            draft[&alignment_role("es-cue")]["semantic_markers"],
            json!({"tb":0,"ta":31,"unrelated":12})
        );
        assert_eq!(
            draft[&alignment_role("second-cue")]["semantic_markers"],
            json!({"td":0,"other":13})
        );
        let events = &draft["selected-graph.json"]["events"];
        assert_eq!(events[2]["phrase_start_seconds"], 0.0);
        assert_eq!(events[2]["phrase_end_seconds"], 2.0);
        assert_eq!(
            draft["scene-bindings.json"]["assets"]["clock"]["selection_state"],
            "selected-private-production"
        );
        assert_eq!(
            draft["scene-bindings.json"]["assets"]["unused-alias"],
            docs["scene-bindings.json"]["assets"]["unused-alias"]
        );
        assert_eq!(
            read(&out.join("edit-request.json")).unwrap()["rows"][0]["comment"],
            edited[0]["comment"]
        );
        assert_eq!(project.load().unwrap().hashes, hashes);
        assert!(apply(&descriptor, &request, &out).is_err());
    }
    #[test]
    fn multi_cue_refuses_cross_cue_moves_rate_changes_missing_cues_and_balanced_global_drift() {
        let d = tempfile::tempdir().unwrap();
        let descriptor = multi_fixture(&d.path().join("source"));
        let project = Project::open(&descriptor).unwrap();
        let (docs, hashes) = load(&project).unwrap();
        let sequence = Sequence::new(&docs, true).unwrap();
        let original = sequence.rows().as_array().unwrap().clone();
        for case in 0..6 {
            let mut edited = original.clone();
            match case {
                0 => edited.swap(1, 2),
                1 => edited[0]["cue_id"] = json!("second-cue"),
                2 => edited[0]["sample_rate"] = json!(44),
                3 => edited.truncate(2),
                4 => {
                    edited[0]["samples"] = json!(95);
                    edited[2]["samples"] = json!(0);
                }
                _ => {
                    edited[0]["samples"] = json!(48);
                    edited[2]["samples"] = json!(42);
                }
            }
            assert!(sequence.plan(&edited).is_err(), "case {case}");
        }
        for version in ["reel.cel-sorter-edit.v1", "reel.cel-sorter-edit.v2"] {
            let request = d.path().join(format!("{version}.json"));
            let out = d.path().join(format!("{version}-output"));
            write(&request,&json!({"schema":version,"scene_id":"synthetic","expected_project":project.descriptor_sha256(),"expected_inputs":hashes,"rows":original})).unwrap();
            assert!(apply(&descriptor, &request, &out).is_err());
            assert!(!out.exists());
        }
        let mut missing = docs.clone();
        missing.remove("alignment:second-cue");
        assert!(Sequence::new(&missing, true).is_err());
        let alignment = source_alignment_path(&descriptor);
        let mut bytes = fs::read(&alignment).unwrap();
        bytes.push(b'\n');
        fs::write(alignment, bytes).unwrap();
        assert!(load(&Project::open(&descriptor).unwrap()).is_err());
    }
    fn source_alignment_path(descriptor: &Path) -> PathBuf {
        descriptor.parent().unwrap().join("timing/second.json")
    }
    #[test]
    fn multi_cue_export_uses_v3_without_ambiguous_global_sample_clock() {
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("source");
        let descriptor = multi_fixture(&source);
        let cache = d.path().join("cache");
        for body in [
            b"first".as_slice(),
            b"second".as_slice(),
            b"clip".as_slice(),
        ] {
            let h = hash(body);
            let path = cache.join("objects/sha256").join(&h[..2]).join(&h);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, body).unwrap();
        }
        let spec = json!({"project":descriptor,"cache_roots":[cache],"movie":asset(b"clip"),"max_export_copy_bytes":15,"output":d.path().join("editor")});
        let request = d.path().join("export.json");
        write(&request, &spec).unwrap();
        export(&request).unwrap();
        let session = read(&d.path().join("editor/session.json")).unwrap();
        assert_eq!(session["schema"], "reel.cel-sorter-session.v3");
        assert!(session.get("end_sample").is_none());
        assert_eq!(session["cues"].as_array().unwrap().len(), 2);
        assert_eq!(session["rows"][0]["samples"], 47);
    }
    #[test]
    fn scoped_export_never_resolves_other_language_media() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let descriptor = multi_fixture(&source);
        super::cel_fixtures::add_english_lane(&source, &descriptor);
        let cache = temp.path().join("cache");
        for body in [
            b"first".as_slice(),
            b"second".as_slice(),
            b"clip".as_slice(),
        ] {
            let hash = hash(body);
            let path = cache.join("objects/sha256").join(&hash[..2]).join(&hash);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, body).unwrap();
        }
        let spec = temp.path().join("spec.json");
        let output = temp.path().join("editor");
        write(&spec,&json!({"project":descriptor,"output":output,"cache_roots":[cache],"movie":asset(b"clip"),"max_export_copy_bytes":15})).unwrap();
        export(&spec).unwrap();
        let session = read(&output.join("session.json")).unwrap();
        assert_eq!(session["schema"], "reel.cel-sorter-session.v4");
        assert_eq!(session["language"], "es");
        assert!(session["images"].get("english-image").is_none());
        assert_eq!(session["rows"].as_array().unwrap().len(), 4);
    }
    #[test]
    fn controls_only_export_preserves_native_scope_without_media_and_refuses_ambiguous_specs() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let descriptor = multi_fixture(&source);
        super::cel_fixtures::add_english_lane(&source, &descriptor);
        super::cel_fixtures::separate_language_graphs(&source, &descriptor);
        let output = temp.path().join("controls");
        let spec = temp.path().join("export.json");
        write(
            &spec,
            &json!({"project":descriptor,"output":output,"title":"Synthetic controls"}),
        )
        .unwrap();
        export_mode(&spec, false).unwrap();
        let session = read(&output.join("session.json")).unwrap();
        assert_eq!(session["media_mode"], "controls-only");
        assert_eq!(session["language"], "es");
        assert!(session["movie"].is_null());
        assert!(session["images"].as_object().unwrap().is_empty());
        assert_eq!(fs::read_dir(&output).unwrap().count(), 3);
        let report = temp.path().join("inspection.json");
        inspect_project(&descriptor, &report).unwrap();
        let request = read(&report).unwrap()["edit_request"].clone();
        for key in ["expected_project", "expected_inputs", "rows"] {
            assert_eq!(session[key], request[key]);
        }
        assert!(export_mode(&spec, false).is_err());
        let bad = temp.path().join("ambiguous.json");
        let rejected = temp.path().join("rejected");
        write(
            &bad,
            &json!({"project":descriptor,"output":rejected,"movie":asset(b"not read")}),
        )
        .unwrap();
        assert!(export_mode(&bad, false).is_err());
        assert!(!rejected.exists());
    }
    #[test]
    fn metadata_inspection_handles_separate_graphs_without_media_or_output_overwrite() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let descriptor = multi_fixture(&source);
        super::cel_fixtures::add_english_lane(&source, &descriptor);
        super::cel_fixtures::separate_language_graphs(&source, &descriptor);
        let report = temp.path().join("private.json");
        inspect_project(&descriptor, &report).unwrap();
        let data = read(&report).unwrap();
        assert_eq!(data["edit_request"]["language"], "es");
        assert_eq!(data["edit_request"]["rows"].as_array().unwrap().len(), 4);
        assert_eq!(data["media_resolved"], false);
        assert!(inspect_project(&descriptor, &report).is_err());
    }
    #[test]
    fn delivery_clock_observations_do_not_change_native_editability() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("source");
        fixture(&root);
        let (mut docs, _) = source(&root).unwrap();
        for (rate, state) in [
            (None, "not-declared"),
            (Some(48), "matches-profile"),
            (Some(44100), "profile-rate-mismatch"),
        ] {
            docs.get_mut("profile.json").unwrap()["sample_rate"] = json!(rate);
            let sequence = Sequence::new(&docs, false).unwrap();
            assert_eq!(sequence.session_clock()["delivery_clock"]["state"], state);
            assert!(sequence.plan(sequence.rows().as_array().unwrap()).is_ok());
        }
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
