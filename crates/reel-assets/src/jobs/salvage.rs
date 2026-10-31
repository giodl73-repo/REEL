//! Explicit recovery of a quiescent, unpublished workspace. Never execute tools
//! or remove scratch. Existing output bytes are failed-job candidates only.
use super::*;
use crate::Config;
use flate2::{Compression, write::GzEncoder};
use std::io::{Cursor, Seek, SeekFrom};

pub struct Salvage<'a> {
    pub workspace: &'a Path,
    pub request_sha256: &'a str,
    pub max_workspace_bytes: u64,
    pub apply: bool,
}

fn guard(path: &Path) -> Result<File> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || lifecycle::is_reparse(&metadata) {
        return Err("Salvage requires regular non-reparse files".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1).custom_flags(0x0020_0000);
    }
    Ok(options.open(path)?)
}
fn pin(file: &mut File, maximum: u64) -> Result<RetainedObject> {
    let size = file.metadata()?.len();
    if size > maximum {
        return Err("Salvage candidate exceeds its declared bound".into());
    }
    file.seek(SeekFrom::Start(0))?;
    let mut hash = Sha256::new();
    let mut count = 0u64;
    let mut buffer = [0u8; 128 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        count = count
            .checked_add(n as u64)
            .ok_or("Salvage byte count overflow")?;
        if count > maximum {
            return Err("Salvage candidate changed or exceeded bound".into());
        }
        hash.update(&buffer[..n]);
    }
    if count != size {
        return Err("Salvage candidate changed while reading".into());
    }
    file.seek(SeekFrom::Start(0))?;
    Ok(RetainedObject {
        sha256: format!("{:x}", hash.finalize()),
        bytes: size,
    })
}
fn quiescent(cfg: &Config, job: &str) -> Result<()> {
    let coordination = cfg
        .coordination_root
        .as_ref()
        .ok_or("Explicit shared coordination required")?;
    let _lock = admission::lock_for(coordination, Duration::from_secs(2))?;
    for extension in ["json", "lease"] {
        match fs::symlink_metadata(
            coordination
                .join("admissions")
                .join(format!("{job}.{extension}")),
        ) {
            Ok(_) => return Err("Job reservation remains; preserve workspace".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
fn publish(
    root: &Path,
    cfg: &Config,
    cache: &Path,
    object: &RetainedObject,
    reader: &mut dyn Read,
) -> Result<()> {
    if active_existing(cfg, &object.sha256, object.bytes)?.is_none() {
        crate::recover_from_reader(root, reader, cache, &object.sha256, object.bytes)?;
    }
    Ok(())
}
/// The caller must hold the original source's fence and retain the exact
/// request/owner admission. Dry-run performs no publication or retention. Apply
/// invokes the supplied retention sink only after all candidates are published.
/// Neither mode removes scratch or infers success from a result.json file.
pub fn salvage_admitted(
    root: &Path,
    spec: Salvage<'_>,
    context: &mut RunContext<'_>,
) -> Result<Value> {
    if !cfg!(windows)
        || !spec.workspace.is_absolute()
        || spec.max_workspace_bytes == 0
        || !valid_oid(spec.request_sha256)
    {
        return Err(
            "Salvage requires Windows guards and explicit bounded workspace/request".into(),
        );
    }
    let cfg = context.config;
    let parent = spec
        .workspace
        .parent()
        .ok_or("Workspace parent missing")?
        .canonicalize()?;
    let mut roots = cfg.cache_roots.iter().filter(|root| {
        root.canonicalize()
            .is_ok_and(|r| parent == r.join("staging/caminos"))
    });
    let cache = roots.next().ok_or("Workspace outside admitted staging")?;
    if roots.next().is_some() {
        return Err("Ambiguous workspace enrollment".into());
    }
    let workspace = lifecycle::checked_workspace(cfg, cache, spec.workspace)?;
    let id = workspace
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("Invalid workspace identity")?;
    if id.len() > 100 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return Err("Invalid workspace identity".into());
    }
    quiescent(cfg, id)?;
    let before = snapshot_owned_workspace(cfg, cache, &workspace)?;
    let size = before.entries.values().try_fold(0u64, |sum, e| {
        sum.checked_add(e.bytes).ok_or("Workspace size overflow")
    })?;
    if size > spec.max_workspace_bytes {
        return Err("Workspace exceeds salvage bound".into());
    }
    let mut manifest = guard(&workspace.join("job.json"))?;
    let manifest_pin = pin(&mut manifest, 256 * 1024)?;
    if manifest_pin.sha256 != spec.request_sha256 {
        return Err("Salvage request differs from admission".into());
    }
    let mut request = Vec::new();
    manifest.read_to_end(&mut request)?;
    let job = parse_job(&request)?;
    let mut originals = Vec::new();
    for (role, assets) in [
        (ResourceRole::Input, &job.inputs),
        (ResourceRole::Tool, &job.tools),
    ] {
        for asset in assets.values() {
            context
                .authority
                .require(role, &asset.sha256, asset.bytes)?;
            let path = active_existing(cfg, &asset.sha256, asset.bytes)?
                .ok_or("Canonical original absent; preserve workspace")?;
            let mut held = guard(&path)?;
            let observed = pin(&mut held, asset.bytes)?;
            if observed.sha256 != asset.sha256 || observed.bytes != asset.bytes {
                return Err("Original changed while verifying".into());
            }
            originals.push(held);
        }
    }
    // Installed tools are not executed or copied; retain the original declared
    // role without claiming an obsolete installation is currently available.
    for tool in job.installed_tools.values() {
        context
            .authority
            .require(ResourceRole::InstalledTool, &tool.sha256, tool.bytes)?;
    }
    let mut outputs = BTreeMap::new();
    let mut held_outputs = BTreeMap::new();
    for (name, output) in &job.outputs {
        let path = workspace.join(relative(&output.path)?);
        match fs::symlink_metadata(&path) {
            Ok(_) => {
                let mut held = guard(&path)?;
                let object = pin(&mut held, output.max_bytes)?;
                outputs.insert(name.clone(), object);
                held_outputs.insert(name.clone(), held);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
    }
    let mut diagnostics = BTreeMap::new();
    let mut diagnostic_guards = Vec::new();
    for name in before.entries.keys().filter(|name| {
        !name.contains('/')
            && ((name.starts_with("step-") && name.ends_with(".log")) || name.ends_with(".pid"))
    }) {
        if diagnostics.len() >= 68 {
            return Err("Diagnostic count exceeds salvage bound".into());
        }
        let mut held = guard(&workspace.join(name))?;
        let skip = held.metadata()?.len().saturating_sub(16 * 1024);
        held.seek(SeekFrom::Start(skip))?;
        let mut text = Vec::new();
        (&mut held).take(16 * 1024).read_to_end(&mut text)?;
        diagnostics.insert(
            name.clone(),
            json!({"truncated_prefix_bytes":skip,"text":String::from_utf8_lossy(&text)}),
        );
        diagnostic_guards.push(held);
    }
    let output_pins: BTreeMap<_, _> = outputs
        .iter()
        .map(|(name, o)| (name.clone(), json!({"sha256":o.sha256,"bytes":o.bytes})))
        .collect();
    let failure = json!({"schema":"reel.failed-job-result.v1","job_id":id,"manifest_sha256":manifest_pin.sha256,"outputs":{},"error":"Interrupted workspace salvaged without execution or success attestation","creative_authority":"not-granted-by-this-receipt"});
    let evidence = serde_json::to_vec(
        &json!({"schema":"reel.job-retirement-evidence.v1","request_utf8":std::str::from_utf8(&request)?,"result":failure,"declared_retained_outputs":output_pins,"diagnostics":diagnostics,"scope":"Interrupted workspace: declared candidates only; no execution, approval or removal"}),
    )?;
    if evidence.len() > 2 * 1024 * 1024 {
        return Err("Salvage evidence exceeds bound".into());
    }
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(&evidence)?;
    let evidence = encoder.finish()?;
    let evidence_pin = RetainedObject {
        sha256: format!("{:x}", Sha256::digest(&evidence)),
        bytes: evidence.len() as u64,
    };
    let publish_bytes = outputs
        .values()
        .try_fold(manifest_pin.bytes + evidence_pin.bytes, |n, o| {
            n.checked_add(o.bytes).ok_or("Salvage publication overflow")
        })?;
    if snapshot_owned_workspace(cfg, cache, &workspace)? != before {
        return Err("Workspace changed during salvage inspection".into());
    }
    quiescent(cfg, id)?;
    if spec.apply {
        let _reservation = admission::reserve_on(root, cfg, cache, publish_bytes)?;
        for (name, object) in &outputs {
            publish(
                root,
                cfg,
                cache,
                object,
                held_outputs.get_mut(name).unwrap(),
            )?;
        }
        publish(root, cfg, cache, &manifest_pin, &mut Cursor::new(&request))?;
        publish(root, cfg, cache, &evidence_pin, &mut Cursor::new(&evidence))?;
        let mut published_guards = Vec::new();
        for object in outputs.values().chain([&manifest_pin, &evidence_pin]) {
            let path = active_existing(cfg, &object.sha256, object.bytes)?
                .ok_or("Salvage publication unavailable")?;
            let mut held = guard(&path)?;
            let checked = pin(&mut held, object.bytes)?;
            if checked.sha256 != object.sha256 || checked.bytes != object.bytes {
                return Err("Published candidate changed".into());
            }
            published_guards.push(held);
        }
        if snapshot_owned_workspace(cfg, cache, &workspace)? != before {
            return Err("Workspace changed before salvage retention".into());
        }
        quiescent(cfg, id)?;
        let registration =
            crate::Temporary(crate::unique(&root.join(".local"), &evidence_pin.sha256));
        let assets:Vec<_>=outputs.values().chain([&manifest_pin,&evidence_pin]).map(|o|json!({"sha256":o.sha256,"bytes":o.bytes,"cache_uri":format!("cache://sha256/{}",o.sha256),"provenance":"Explicit interrupted-job salvage; no execution attestation","rights":"Private job custody; upstream restrictions remain; no publication approval","selection_state":"retained-private-job-candidate"})).collect();
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&registration.0)?;
        file.write_all(&serde_json::to_vec(&json!({"assets":assets}))?)?;
        file.sync_all()?;
        drop(file);
        context.retention.retain(&RetainedJob {
            id: id.into(),
            succeeded: false,
            request: manifest_pin,
            evidence: evidence_pin,
            outputs,
            registration: registration.0.clone(),
        })?;
    }
    Ok(
        json!({"schema":"reel.unpublished-workspace-salvage.experimental-1","job_id":id,"applied":spec.apply,"succeeded":false,"declared_candidates":output_pins.len(),"workspace_logical_bytes_verified":size,"publication_bytes":publish_bytes,"workspace_retained":true,"staging_retired":false,"authorizes_workspace_removal":false,"production_claim":null,"authorizes_execution":false,"authorizes_eviction":false}),
    )
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::tiering::tests::Fixture;
    struct Authority;
    impl ResourceAuthority for Authority {
        fn require(&self, _: ResourceRole, _: &str, _: u64) -> Result<()> {
            Ok(())
        }
    }
    #[derive(Default)]
    struct Sink {
        calls: usize,
        fail: bool,
        request: String,
        evidence: String,
    }
    impl RetentionSink for Sink {
        fn retain(&mut self, batch: &RetainedJob) -> Result<()> {
            self.calls += 1;
            assert!(!batch.succeeded());
            assert_eq!(batch.outputs().len(), 1);
            assert!(batch.registration.is_file());
            self.request = batch.request().sha256().into();
            self.evidence = batch.evidence().sha256().into();
            if self.fail {
                Err("injected source retention failure".into())
            } else {
                Ok(())
            }
        }
    }
    fn setup() -> (Fixture, Config, PathBuf, String) {
        let f = Fixture::new();
        let source = f.put(b"original", false);
        let tool = f.put(b"deliberately not an executable", false);
        let mut cfg = config(f.root()).unwrap();
        cfg.path_policy = crate::PathPolicy::ConfiguredLocalRoots;
        cfg.coordination_root = Some(f.root().join("coordination"));
        fs::create_dir(cfg.coordination_root.as_ref().unwrap()).unwrap();
        let workspace = f.root().join("fast/staging/caminos/job-interrupted");
        fs::create_dir_all(&workspace).unwrap();
        let request=serde_json::to_vec(&json!({"budget_bytes":1024*1024,"scratch_bytes":65536,"inputs":{"in.bin":{"sha256":source,"bytes":8}},"tools":{"tool.exe":{"sha256":tool,"bytes":b"deliberately not an executable".len()}},"steps":[{"tool":"tool.exe","args":[],"timeout_seconds":1}],"outputs":{"candidate":{"path":"out.bin","max_bytes":1024},"missing":{"path":"missing.bin","max_bytes":1024}}})).unwrap();
        let hash = format!("{:x}", Sha256::digest(&request));
        fs::write(workspace.join("job.json"), &request).unwrap();
        fs::write(workspace.join("out.bin"), b"partial candidate").unwrap();
        fs::write(workspace.join("step-0.log"), vec![b'a'; 20000]).unwrap();
        fs::write(workspace.join("intermediate.bin"), b"never published").unwrap();
        (f, cfg, workspace, hash)
    }
    #[test]
    fn interrupted_salvage_dry_run_failure_retry_retains_candidates_without_execution_or_removal() {
        let (f, cfg, workspace, hash) = setup();
        let before = snapshot_owned_workspace(&cfg, &cfg.cache_roots[0], &workspace).unwrap();
        let mut sink = Sink::default();
        let call = |sink: &mut Sink, apply| {
            salvage_admitted(
                f.root(),
                Salvage {
                    workspace: &workspace,
                    request_sha256: &hash,
                    max_workspace_bytes: 1024 * 1024,
                    apply,
                },
                &mut RunContext {
                    config: &cfg,
                    authority: &Authority,
                    retention: sink,
                },
            )
        };
        let dry = call(&mut sink, false).unwrap();
        assert_eq!(dry["applied"], false);
        assert_eq!(dry["declared_candidates"], 1);
        assert_eq!(sink.calls, 0);
        let candidate = format!("{:x}", Sha256::digest(b"partial candidate"));
        assert!(active_existing(&cfg, &candidate, 17).unwrap().is_none());
        sink.fail = true;
        assert!(call(&mut sink, true).is_err());
        assert_eq!(
            before,
            snapshot_owned_workspace(&cfg, &cfg.cache_roots[0], &workspace).unwrap()
        );
        let evidence = sink.evidence.clone();
        sink.fail = false;
        let applied = call(&mut sink, true).unwrap();
        assert_eq!(sink.calls, 2);
        assert_eq!(sink.evidence, evidence);
        assert_eq!(applied["succeeded"], false);
        assert_eq!(applied["workspace_retained"], true);
        assert_eq!(applied["authorizes_workspace_removal"], false);
        assert_eq!(
            before,
            snapshot_owned_workspace(&cfg, &cfg.cache_roots[0], &workspace).unwrap()
        );
        let path = active_existing(
            &cfg,
            &sink.evidence,
            fs::metadata(crate::object(&cfg.cache_roots[0], &sink.evidence))
                .unwrap()
                .len(),
        )
        .unwrap()
        .unwrap();
        let mut raw = Vec::new();
        flate2::read::GzDecoder::new(File::open(path).unwrap())
            .read_to_end(&mut raw)
            .unwrap();
        let e: Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(e["result"]["schema"], "reel.failed-job-result.v1");
        assert_eq!(
            e["diagnostics"]["step-0.log"]["truncated_prefix_bytes"],
            3616
        );
        assert!(e["declared_retained_outputs"].get("missing").is_none());
        let intermediate = format!("{:x}", Sha256::digest(b"never published"));
        assert!(active_existing(&cfg, &intermediate, 15).unwrap().is_none());
    }
    #[test]
    fn unsafe_or_unowned_interrupted_workspace_never_publishes() {
        for case in [
            "reservation",
            "lease",
            "manifest",
            "oversized",
            "missing-original",
            "workspace-bound",
            "wrong-scope",
        ] {
            let (f, cfg, workspace, hash) = setup();
            let mut requested = workspace.clone();
            let mut max = 1024 * 1024;
            match case {
                "reservation" | "lease" => {
                    let dir = cfg.coordination_root.as_ref().unwrap().join("admissions");
                    fs::create_dir(&dir).unwrap();
                    fs::write(
                        dir.join(format!(
                            "job-interrupted.{}",
                            if case == "lease" { "lease" } else { "json" }
                        )),
                        b"held",
                    )
                    .unwrap();
                }
                "manifest" => fs::write(workspace.join("job.json"), b"{}").unwrap(),
                "oversized" => fs::write(workspace.join("out.bin"), vec![0; 1025]).unwrap(),
                "missing-original" => {
                    let oid = format!("{:x}", Sha256::digest(b"original"));
                    for cache in &cfg.cache_roots {
                        fs::remove_file(crate::object(cache, &oid)).unwrap();
                    }
                }
                "workspace-bound" => max = 1,
                _ => {
                    requested = f.root().join("job-outside");
                    fs::create_dir(&requested).unwrap();
                }
            }
            let mut sink = Sink::default();
            let result = salvage_admitted(
                f.root(),
                Salvage {
                    workspace: &requested,
                    request_sha256: &hash,
                    max_workspace_bytes: max,
                    apply: true,
                },
                &mut RunContext {
                    config: &cfg,
                    authority: &Authority,
                    retention: &mut sink,
                },
            );
            assert!(result.is_err(), "{case}");
            assert_eq!(sink.calls, 0, "{case}");
            assert!(workspace.exists());
            let candidate = format!("{:x}", Sha256::digest(b"partial candidate"));
            assert!(
                active_existing(&cfg, &candidate, 17).unwrap().is_none(),
                "{case}"
            );
        }
    }
}
