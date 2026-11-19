//! Original admitted synthetic movie; scattered returns and repinned wrong media.
use super::*;
use reel_workflow_native::{
    NativeInputs, NativeProfile, admission, film_inputs::Programs, movie, response::Returns,
    staging,
};
use std::{fs::File, io::Write};

pub(super) struct Metadata<'a>(pub(super) &'a Path);
impl publication::InputReader for Metadata<'_> {
    fn read(&mut self, a: &Artifact, max: u64) -> Result<Vec<u8>> {
        let index = a
            .logical_id
            .strip_prefix("input/")
            .unwrap()
            .parse::<usize>()
            .unwrap();
        let raw = fs::read(self.0.join(format!("synthetic-inputs/input-{index}.bin"))).unwrap();
        assert!(raw.len() as u64 <= max);
        Ok(raw)
    }
}
fn held(path: &Path) -> File {
    use std::os::windows::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(path)
        .unwrap()
}
pub(super) fn tool(path: PathBuf) -> Tool {
    let raw = fs::read(&path).unwrap();
    Tool {
        path,
        sha256: digest(&raw),
        bytes: raw.len() as u64,
    }
}
fn ref_value(path: &Path, raw: &[u8]) -> serde_json::Value {
    json!({"path":path,"sha256":digest(raw),"bytes":raw.len()})
}
pub(super) fn package(
    packet: &Packet,
    bound: &film_render::Bound,
    files: &BTreeMap<String, Vec<u8>>,
    root: &Path,
    attempt: &str,
) -> (ReturnedResult, Returns) {
    fs::create_dir(root).unwrap();
    fs::create_dir(root.join("objects")).unwrap();
    let receipt = film_render::capture_outputs(packet, bound, files).unwrap();
    let raw = serde_json::to_vec(&receipt).unwrap();
    let paths = bound
        .request()
        .output_paths
        .keys()
        .enumerate()
        .map(|(i, name)| (name.clone(), format!("objects/{i:03}.bin")))
        .collect::<BTreeMap<_, _>>();
    for (name, path) in &paths {
        fs::write(root.join(path), &files[&bound.request().output_paths[name]]).unwrap();
    }
    fs::write(root.join("receipt.json"), &raw).unwrap();
    (
        ReturnedResult {
            schema: RESULT_SCHEMA.into(),
            task_id: packet.task.id.clone(),
            task_key: packet.task_key.clone(),
            attempt_id: attempt.into(),
            artifacts: receipt.outputs,
            provenance: Provenance {
                mode: Mode::Tool,
                provider: None,
                model: None,
                executable: packet.task.tool.as_ref().map(|t| t.executable.clone()),
            },
            worker_receipt: Some(Artifact {
                logical_id: format!("workflow/worker/{}/{attempt}/receipt", packet.task_key),
                sha256: digest(&raw),
                bytes: raw.len() as u64,
                media_type: "application/json".into(),
            }),
        },
        Returns {
            schema: "reel.workflow-returns.v1".into(),
            root: root.into(),
            files: paths,
        },
    )
}
// Generate a self-consistent film from deliberately different synthetic picture
// primaries in a separate owned workspace. This is an adversarial fixture, not
// a production task or claim to the original task's execution authority.
fn alternate(
    original: &Path,
    owner: &Path,
    runtime: &Tool,
    programs: &Programs,
    packet: &Packet,
) -> BTreeMap<String, Vec<u8>> {
    let sources = owner.join("alternate-inputs");
    fs::create_dir(&sources).unwrap();
    let workspace = owner.join("alternate-render");
    fs::create_dir(&workspace).unwrap();
    let aliases: BTreeMap<String, String> =
        serde_json::from_slice(&fs::read(original.join("aliases.json")).unwrap()).unwrap();
    // Read the original producer request by its explicit fixture ticket locator.
    let old_ticket: staging::Ticket =
        serde_json::from_slice(&fs::read(original.join("movie-owned-ticket.json")).unwrap())
            .unwrap();
    let mut request: serde_json::Value = serde_json::from_slice(
        &fs::read(old_ticket.directory().join("movie/render-request.json")).unwrap(),
    )
    .unwrap();
    for (name, id) in &aliases {
        let asset = packet
            .task
            .inputs
            .iter()
            .find(|a| &a.logical_id == id)
            .unwrap();
        let index = id.strip_prefix("input/").unwrap().parse::<usize>().unwrap();
        let raw = fs::read(original.join(format!("synthetic-inputs/input-{index}.bin"))).unwrap();
        assert_eq!(asset.sha256, digest(&raw));
        fs::write(sources.join(name), &raw).unwrap();
    }
    let mut job: serde_json::Value =
        serde_json::from_slice(&fs::read(sources.join("job.json")).unwrap()).unwrap();
    for (name, index, color) in [("red.ppm", 0, [0, 255, 0]), ("blue.ppm", 1, [255, 255, 0])] {
        let mut raw = fs::read(sources.join(name)).unwrap();
        assert_eq!(raw.len(), 23);
        for pixel in 0..4 {
            raw[11 + pixel * 3..14 + pixel * 3].copy_from_slice(&color);
        }
        fs::write(sources.join(name), &raw).unwrap();
        job["pictures"][index]["source"]["sha256"] = json!(digest(&raw));
        job["pictures"][index]["source"]["bytes"] = json!(raw.len());
    }
    fs::write(sources.join("job.json"), serde_json::to_vec(&job).unwrap()).unwrap();
    for name in aliases.keys() {
        let path = sources.join(name).canonicalize().unwrap();
        let raw = fs::read(&path).unwrap();
        request["inputs"][name] = ref_value(&path, &raw);
    }
    let raw = fs::read(sources.join("job.json")).unwrap();
    request["job"]["sha256"] = json!(digest(&raw));
    request["job"]["bytes"] = json!(raw.len());
    request["runtime"] = serde_json::to_value(runtime).unwrap();
    request["workspace"] = json!(workspace.canonicalize().unwrap());
    let raw = serde_json::to_vec(&request).unwrap();
    let request_path = workspace.join("render-request.json");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&request_path)
        .unwrap();
    file.write_all(&raw).unwrap();
    file.sync_all().unwrap();
    drop(file);
    let _directory =
        reel_assets::jobs::open_workspace_directory(&sources.canonicalize().unwrap()).unwrap();
    let mut guards = aliases
        .keys()
        .map(|n| held(&sources.join(n)))
        .collect::<Vec<_>>();
    guards.extend([
        held(&request_path),
        held(&runtime.path),
        held(&programs.ffmpeg.path),
        held(&programs.ffprobe.path),
    ]);
    reel_assets::jobs::run_quiet(
        &runtime.path,
        &[
            "film-process-child".into(),
            request_path.to_string_lossy().into_owned(),
            digest(&raw),
            "--apply".into(),
        ],
        &workspace.canonicalize().unwrap(),
        std::time::Duration::from_secs(120),
        reel_assets::jobs::QuietLimits {
            memory_bytes: 4 * 1024 * 1024 * 1024,
            processes: 8,
        },
        || Ok(()),
    )
    .unwrap();
    let output = workspace.join("delivery");
    let receipt: reel::scene_delivery::Receipt =
        serde_json::from_slice(&fs::read(output.join("receipt.json")).unwrap()).unwrap();
    receipt
        .outputs
        .keys()
        .map(String::as_str)
        .chain(std::iter::once("receipt.json"))
        .map(|name| {
            (
                format!("delivery/{name}"),
                fs::read(output.join(name)).unwrap(),
            )
        })
        .collect()
}

#[test]
#[ignore = "Explicit original admitted movie and pinned verifier; scattered candidate check/repinned wrong-primary refusal only, retains roots and custody"]
fn scattered_movie_returns_verify_primaries_and_refuse_rehashed_wrong_scene() {
    let original = PathBuf::from(
        std::env::var_os("REEL_MOVIE_RECOVERY_FIXTURE").expect("Set original movie fixture"),
    );
    let packet: Packet =
        serde_json::from_slice(&fs::read(original.join("packet.json")).unwrap()).unwrap();
    let source: NativeProfile =
        serde_json::from_slice(&fs::read(original.join("source.json")).unwrap()).unwrap();
    let admission: admission::Profile =
        serde_json::from_slice(&fs::read(original.join("admission-profile.json")).unwrap())
            .unwrap();
    let receipt: admission::Receipt =
        serde_json::from_slice(&fs::read(original.join("admission-receipt.json")).unwrap())
            .unwrap();
    let old_ticket: staging::Ticket =
        serde_json::from_slice(&fs::read(original.join("movie-owned-ticket.json")).unwrap())
            .unwrap();
    let programs: Programs =
        serde_json::from_slice(&fs::read(original.join("programs.json")).unwrap()).unwrap();
    let verifier = tool(PathBuf::from(
        std::env::var_os("REEL_NATIVE_HANDOFF_TOOL").expect("Set verifier runtime"),
    ));
    let mut temp = tempfile::tempdir().unwrap();
    temp.disable_cleanup(true);
    let root = temp.path();
    let evidence = std::env::var_os("REEL_FILM_EVIDENCE_DIRECTORY").map(PathBuf::from);
    let marker = json!({"root":root,"original":original,"verifier":verifier});
    fs::write(
        root.join("original-operation.json"),
        serde_json::to_vec(&marker).unwrap(),
    )
    .unwrap();
    if let Some(e) = &evidence {
        fs::write(
            e.join("original-operation-roots.json"),
            serde_json::to_vec(&marker).unwrap(),
        )
        .unwrap();
    }
    let parent = root.join("verification-owned");
    fs::create_dir(&parent).unwrap();
    let ticket = staging::prepare(
        &packet,
        &source,
        &admission,
        &receipt,
        &staging::Profile {
            schema: "reel.workflow-staging.v1".into(),
            parent,
            maximum_bytes: 256 * 1024 * 1024,
            capacity_policy: reel_assets::admission::Policy {
                minimum_free_bytes: 100 * 1024 * 1024,
                minimum_free_basis_points: 0,
                max_object_bytes: 512 * 1024 * 1024,
                ..Default::default()
            },
        },
    )
    .unwrap();
    fs::write(
        root.join("verification-owned-ticket.json"),
        ticket.canonical_bytes().unwrap(),
    )
    .unwrap();
    let owner = ticket
        .resume_workspace(&packet, &source, &admission, &receipt)
        .unwrap();
    let before = fs::read(admission.index_root.join("claims.json")).unwrap();
    let bound = film_render::bind(&packet, &mut Metadata(&original)).unwrap();
    let files = bound
        .request()
        .output_paths
        .values()
        .map(|path| {
            (
                path.clone(),
                fs::read(old_ticket.directory().join("movie").join(path)).unwrap(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let (result, returns) = package(
        &packet,
        &bound,
        &files,
        &owner.path().join("good-return"),
        &receipt.attempt_id,
    );
    let program_map = BTreeMap::from([
        ("ffmpeg".into(), programs.ffmpeg.clone()),
        ("ffprobe".into(), programs.ffprobe.clone()),
    ]);
    let good = owner.path().join("good-check");
    fs::create_dir(&good).unwrap();
    let checked = movie::check_returned(
        &packet,
        NativeInputs::open_admitted(&packet, &source, &admission, &receipt).unwrap(),
        &result,
        &returns,
        &program_map,
        &verifier,
        &good,
    )
    .unwrap();
    assert!(
        fs::OpenOptions::new()
            .write(true)
            .open(returns.root.join(&returns.files["master"]))
            .is_err()
    );
    drop(checked);
    let corrupt = returns.root.join(&returns.files["master"]);
    let original_master = fs::read(&corrupt).unwrap();
    let mut changed = original_master.clone();
    changed[0] ^= 1;
    fs::write(&corrupt, &changed).unwrap();
    let refused = owner.path().join("bad-hash-check");
    fs::create_dir(&refused).unwrap();
    assert_eq!(
        movie::check_returned(
            &packet,
            NativeInputs::open_admitted(&packet, &source, &admission, &receipt).unwrap(),
            &result,
            &returns,
            &program_map,
            &verifier,
            &refused
        )
        .err()
        .unwrap()
        .kind,
        ErrorKind::Integrity
    );
    assert!(!refused.join("verify-request.json").exists());
    fs::write(&corrupt, &original_master).unwrap();
    let alternate = alternate(&original, owner.path(), &verifier, &programs, &packet);
    let mut wrong = files.clone();
    for (path, raw) in alternate {
        wrong.insert(path, raw);
    }
    // Supply self-consistent hashes and the original plan/map to challenge the
    // primary comparison, rather than merely making malformed container bytes.
    wrong.insert(
        "delivery/review-ids.json".into(),
        files["delivery/review-ids.json"].clone(),
    );
    let mut wrong_receipt: serde_json::Value =
        serde_json::from_slice(&wrong["delivery/receipt.json"]).unwrap();
    let old: serde_json::Value = serde_json::from_slice(&files["delivery/receipt.json"]).unwrap();
    wrong_receipt["plan"] = old["plan"].clone();
    wrong_receipt["outputs"]["review-ids.json"]["sha256"] =
        json!(digest(&wrong["delivery/review-ids.json"]));
    wrong_receipt["outputs"]["review-ids.json"]["bytes"] =
        json!(wrong["delivery/review-ids.json"].len());
    wrong.insert(
        "delivery/receipt.json".into(),
        serde_json::to_vec(&wrong_receipt).unwrap(),
    );
    let (bad_result, bad_returns) = package(
        &packet,
        &bound,
        &wrong,
        &owner.path().join("wrong-return"),
        &receipt.attempt_id,
    );
    let bad = owner.path().join("wrong-check");
    fs::create_dir(&bad).unwrap();
    let error = movie::check_returned(
        &packet,
        NativeInputs::open_admitted(&packet, &source, &admission, &receipt).unwrap(),
        &bad_result,
        &bad_returns,
        &program_map,
        &verifier,
        &bad,
    )
    .err()
    .unwrap();
    assert_eq!(error.kind, ErrorKind::Integrity);
    assert_eq!(error.code, "film-verification-primary-reference");
    assert_eq!(
        before,
        fs::read(admission.index_root.join("claims.json")).unwrap()
    );
    if let Some(e) = evidence {
        fs::write(e.join("qualification-result.json"),serde_json::to_vec(&json!({"root":root,"original":original,"workspace":owner.path(),
        "scattered_returns_verified":true,"output_mutation_refused":true,"hash_corruption_refused_before_child":true,"repinned_wrong_primary_refused":true,
        "frames":48,"claims_unchanged":true,"output_retained":false,"task_completed":false,"scratch_retired":false})).unwrap()).unwrap();
    }
}
