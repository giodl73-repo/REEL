//! Actual metadata exchange, disabled upload, verified publication and recheck.
use super::*;
use git_asset_core::tree::MemoryNodes;
use git_asset_store::existing::RequiredProtection;
use std::{
    ffi::OsString,
    io::Read,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
fn executable() -> Result<PathBuf> {
    Ok(std::env::current_exe()?
        .parent()
        .ok_or("sample binary directory")?
        .join(format!("git-asset-lfs{}", std::env::consts::EXE_SUFFIX)))
}
fn execute(args: &[OsString]) -> Result<(bool, Value)> {
    let mut child = Command::new(executable()?)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("missing upload stdout".into());
    };
    let Some(stderr) = child.stderr.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("missing upload stderr".into());
    };
    let out = std::thread::spawn(move || {
        let mut b = Vec::new();
        stdout.take(65537).read_to_end(&mut b).map(|_| b)
    });
    let err = std::thread::spawn(move || {
        let mut b = Vec::new();
        stderr.take(65537).read_to_end(&mut b).map(|_| b)
    });
    let deadline = Instant::now() + Duration::from_secs(20);
    let status: Result<_> = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Err(error) => break Err(error.into()),
            Ok(None) => {}
        }
        if Instant::now() >= deadline {
            break Err("upload child deadline".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    if status.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let stdout_result = out.join();
    let stderr_result = err.join();
    let status = status?;
    let stdout = stdout_result.map_err(|_| "upload stdout worker")??;
    let stderr = stderr_result.map_err(|_| "upload stderr worker")??;
    if stdout.len() > 65536 || stderr.len() > 65536 || !stderr.is_empty() {
        return Err("upload child bounded-output contract".into());
    }
    Ok((status.success(), serde_json::from_slice(&stdout)?))
}
pub(super) fn preflight() -> Result<()> {
    if !executable()?.is_file() {
        return Err(
            "build sibling git-asset-lfs with schema,upload before running this sample".into(),
        );
    }
    let (ok, v) = execute(&["upload".into(), "describe".into()])?;
    if !ok
        || v["schema"] != "tig.lfs-upload-interface.design-1"
        || v["retained_custody"] != false
        || v["removal_authority"] != false
    {
        return Err("sibling git-asset-lfs needs the upload feature".into());
    }
    Ok(())
}
fn call(action: &str, profile: &Path, document: &Path) -> Result<(bool, Value)> {
    execute(&[
        OsString::from("upload"),
        OsString::from(action),
        profile.as_os_str().to_owned(),
        document.as_os_str().to_owned(),
    ])
}
fn good(action: &str, profile: &Path, document: &Path) -> Result<Value> {
    let (ok, v) = call(action, profile, document)?;
    if !ok {
        return Err(format!("fixture CLI {action} refused: {v}").into());
    }
    Ok(v)
}
fn local(d: &Demo, head: &str) -> Result<Value> {
    if git(&d.git, &d.root.join("project"), &["rev-parse", "HEAD"])? != head {
        return Err("remote teaching changed Git HEAD".into());
    }
    Ok(
        json!({"source":d.namespaces[0].checkpoint(),"ledgers":d.ledgers.iter().map(CapacityLedger::checkpoint).collect::<Vec<_>>(),"copies":d.copy_snapshot()?,"seed_attempts":d.seed.attempts,"git_head":head}),
    )
}
fn same_receiver(a: &Value, b: &Value) -> bool {
    [
        "store",
        "capacity",
        "checkpoint",
        "objects",
        "registered",
        "put_count",
        "put_payload_bytes",
    ]
    .iter()
    .all(|k| a[*k] == b[*k])
}
struct Capture<'a> {
    d: &'a mut Demo,
    head: &'a str,
    common: Value,
    reports: Vec<Object>,
}
impl Capture<'_> {
    fn record(
        &mut self,
        id: &str,
        phase: &str,
        before: &Value,
        after: &Value,
        witness: (Value, Value),
        cli: Value,
    ) -> Result<()> {
        let (p, status) = self.d.with_placement(|e| {
            let p = e.plan(None)?;
            let s = e.status(&p)?;
            Ok((p, s))
        })?;
        if !status.page_converged
            || status.storage_converged != Some(true)
            || !status.effects.is_empty()
            || status.stream_bytes != 0
        {
            return Err("remote publication changed local placement".into());
        }
        let mut v = self.common.clone();
        v["lesson"] = id.into();
        v["phase"] = phase.into();
        v["previous_report"] = self
            .d
            .reports
            .last()
            .ok_or("missing exchange predecessor")?["report"]
            .clone();
        v["local_plan"] = serde_json::to_value(p)?;
        v["local_status"] = serde_json::to_value(status)?;
        v["local_after"] = local(self.d, self.head)?;
        if v["local_before"] != v["local_after"] {
            return Err("upload changed source or managed copies".into());
        }
        v["endpoint_before"] = before.clone();
        v["endpoint_after"] = after.clone();
        v["witness_before"] = witness.0;
        v["witness_after"] = witness.1;
        v["cli"] = cli;
        let stage = self.d.fixture["stages"]
            .as_array()
            .ok_or("missing stages")?
            .iter()
            .find(|s| s["id"] == id)
            .ok_or("missing remote oracle")?;
        for name in self.d.objects.keys() {
            let observed = ["working", "history", "backup"]
                .into_iter()
                .filter(|r| v["local_after"]["copies"][*r].get(name).is_some())
                .collect::<BTreeSet<_>>();
            let expected = stage["expected_copies"][name]
                .as_array()
                .ok_or("missing remote local oracle")?
                .iter()
                .map(|s| s.as_str().ok_or("invalid remote oracle"))
                .collect::<std::result::Result<BTreeSet<_>, _>>()?;
            if observed != expected {
                return Err("remote local physical oracle".into());
            }
        }
        let expected = stage["expected_remote_receiver"]
            .as_array()
            .ok_or("missing receiver oracle")?
            .iter()
            .map(|n| -> Result<Value> {
                Ok(serde_json::to_value(
                    &self.d.objects[n.as_str().ok_or("invalid receiver oracle")?],
                )?)
            })
            .collect::<Result<Vec<_>>>()?;
        let actual = after["objects"]
            .as_array()
            .ok_or("missing received objects")?;
        if actual.len() != expected.len() || expected.iter().any(|o| !actual.contains(o)) {
            return Err("receiver physical oracle".into());
        }
        let file = format!("reports/{id}.json");
        let o = save(&self.d.root.join(&file), &v)?;
        if o.bytes > 65536 {
            return Err("remote report budget".into());
        }
        fs::copy(
            self.d.root.join(&file),
            Layout::Flat.object_path(&self.d.root.join("lesson-evidence"), &o.sha256),
        )?;
        self.d.reports.push(json!({"lesson":id,"report":o,"file":file,"title":id.replace('-'," "),"event":stage["event"]}));
        self.reports.push(o);
        Ok(())
    }
}
pub(super) fn run(d: &mut Demo, head: &str) -> Result<Value> {
    d.seed.disabled = true;
    let view = d.repository.commit(GitId::parse(head)?)?;
    let snapshot = d.repository.native(
        &view,
        Scope::new([LogicalPath::parse("records")?])?,
        Scope::new([LogicalPath::parse("definitions")?])?,
        native::Limits::default(),
    )?;
    let entries = Demo::flat_entries(&snapshot, None)?;
    let objects = entries
        .values()
        .map(|o| (o.sha256.clone(), o.clone()))
        .collect::<BTreeMap<_, _>>()
        .into_values()
        .collect::<Vec<_>>();
    if objects.len() != 2
        || !objects.contains(&d.objects["fixed"])
        || !objects.contains(&d.objects["shared"])
        || objects.contains(&d.objects["original"])
    {
        return Err("remote selected scope".into());
    }
    let mut native_nodes = MemoryNodes::default();
    let (native_tree, _) = Tree::build(&entries, &mut native_nodes, TreeLimits::default())?;
    let native_selection = native_tree.root().clone();
    let selected = d.namespaces[0]
        .source(&key("main"))?
        .ok_or("missing current source")?
        .current
        .ok_or("missing current selection")?;
    if selected.selection.root != native_selection {
        return Err("native selection differs from adopted source".into());
    }
    let oid_entries = objects
        .iter()
        .map(|o| Ok((LogicalPath::parse(o.sha256.as_str())?, o.clone())))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut nodes = MemoryNodes::default();
    let (tree, _) = Tree::build(&oid_entries, &mut nodes, TreeLimits::default())?;
    let authority = tree.root().clone();
    let dir = d.root.join("remote-upload");
    fs::create_dir(&dir)?;
    let node_dir = dir.join("authority");
    fs::create_dir(&node_dir)?;
    for (id, bytes) in nodes.objects() {
        fs::write(node_dir.join(id.as_str()), bytes)?;
    }
    let source = dir.join("source");
    fs::create_dir(&source)?;
    let working = d
        .config
        .providers
        .iter()
        .position(|p| p.assignments[0].place.as_str() == "working")
        .ok_or("missing working provider")?;
    let mut staged = 0;
    for object in &objects {
        let mut bytes = Vec::new();
        d.stores[working]
            .read_object(object)?
            .take(65)
            .read_to_end(&mut bytes)?;
        if Object::from_bytes(&bytes) != *object {
            return Err("upload input export integrity".into());
        }
        let path = Layout::GitLfs.object_path(&source, &object.sha256);
        fs::create_dir_all(path.parent().ok_or("upload source parent")?)?;
        fs::write(path, &bytes)?;
        staged += bytes.len() as u64;
    }
    if staged != 54 {
        return Err("unexpected upload fixture staging".into());
    }
    let witness = LocalStore::create(&dir.join("witness"), key("sample-upload-witness"))?;
    let mut ledger = CapacityLedger::create(
        &dir.join("ledger"),
        key("sample-upload-capacity"),
        key("sample-upload-epoch"),
        0,
    )?;
    LocalCapacityProbe::bind(&witness, &mut ledger)?.observe(
        &witness,
        &mut ledger,
        key("upload-probe"),
        key("sample"),
    )?;
    let job = ledger
        .reserve(key("upload-job"), key("sample"), 64 * 1024 * 1024)?
        .reference()
        .clone();
    let minimum = ledger.checkpoint();
    drop(ledger);
    let receiver = remote_endpoint::Endpoint::new(&d.root, &objects)?;
    let empty = receiver.snapshot()?;
    let context = Digest::blob(&identity::canonical_json(
        &json!({"source":d.namespaces[0].checkpoint(),"authority":authority}),
    )?);
    let mut profile = json!({"schema":"tig.lfs-upload.design-1","source_root":source,"source_identity":context,"source_generation":1,"source_protection":if cfg!(windows){RequiredProtection::PreventMutation}else{RequiredProtection::CooperativeImmutable},"authority_root":authority,"authority_nodes":node_dir,"authority_checkpoint":null,"max_authority_entries":16,"witness_root":dir.join("witness"),"witness":Place::local(&witness,1)?,"ledger_root":dir.join("ledger"),"minimum":minimum,"capacity":minimum.identity(),"job":job,"owner":"sample","context":context,"operation_scope":"public-teaching-upload","expected_remote":"sample","ref_name":"refs/heads/main","remote":{"instance":empty["store"],"generation":1,"batch_url":format!("{}/repo/objects/batch",receiver.origin),"headers":{},"scopes":[{"origin":receiver.origin,"prefix":"/actions/","headers":[]}],"loopback_fixture":true},"budget":{"object_bytes":64,"total_source_bytes":256,"objects":4,"http_requests":16,"transfer_bytes":65536,"millis":10000},"allow_upload":false});
    let disabled = dir.join("disabled.json");
    save(&disabled, &profile)?;
    profile["allow_upload"] = true.into();
    let enabled = dir.join("enabled.json");
    save(&enabled, &profile)?;
    let mut plans = Vec::new();
    let mut paths = Vec::new();
    let mut failures = Vec::new();
    let mut peaks = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        let obj = dir.join(format!("object-{i}.json"));
        save(&obj, object)?;
        let raw = good("plan", &disabled, &obj)?;
        let p = git_asset::upload::Plan::from_json(&identity::canonical_json(&raw)?)?;
        if p.object != *object {
            return Err("wrong upload plan object".into());
        }
        let path = dir.join(format!("plan-{i}.json"));
        fs::write(&path, p.to_json()?)?;
        peaks.push(good("peak", &disabled, &path)?);
        let (ok, v) = call("apply", &disabled, &path)?;
        if ok || v["code"] != "upload_disabled" {
            return Err("disabled upload did not refuse".into());
        }
        failures.push(v);
        plans.push(raw);
        paths.push(path);
    }
    let unauthorized = dir.join("unauthorized.json");
    save(&unauthorized, &d.objects["original"])?;
    let (ok, not_admitted) = call("plan", &disabled, &unauthorized)?;
    if ok
        || not_admitted["code"] != "authority"
        || receiver.snapshot()?["requests"] != 0
        || CapacityLedger::open(&dir.join("ledger"), &minimum)?.checkpoint() != minimum
    {
        return Err("disabled or unauthorized upload had effects".into());
    }
    let local_before = local(d, head)?;
    let common = json!({"schema":"tig.teaching-remote-publication-observation.design-1","source_commit":head,"source_checkpoint":d.namespaces[0].checkpoint(),"providers":d.config.providers.iter().map(|p|&p.place.instance).collect::<Vec<_>>(),"authority":authority,"witness_store":witness.identity(),"native_selection":native_selection,"source_bindings":entries,"selected":objects,"plans":plans,"peaks":peaks,"fixture_input_staging_bytes":staged,"local_before":local_before,"seed_disabled":true,"retained_custody":false,"removal_authority":false,"synthetic_endpoint":true,"independent_hardware_backup_proven":false});
    let checkpoint = || -> Result<Value> {
        Ok(serde_json::to_value(
            CapacityLedger::open(&dir.join("ledger"), &minimum)?.checkpoint(),
        )?)
    };
    let mut capture = Capture {
        d,
        head,
        common,
        reports: Vec::new(),
    };
    let initial = checkpoint()?;
    capture.record(
        "remote-upload-disabled",
        "upload_disabled",
        &empty,
        &receiver.snapshot()?,
        (initial.clone(), checkpoint()?),
        json!({"apply":failures,"unauthorized_plan":not_admitted}),
    )?;
    let before = receiver.snapshot()?;
    let w = checkpoint()?;
    let mut receipts = Vec::new();
    let mut statuses = Vec::new();
    for path in &paths {
        receipts.push(good("apply", &enabled, path)?);
        statuses.push(good("status", &enabled, path)?);
    }
    let published = receiver.snapshot()?;
    if published["put_count"] != 2
        || published["put_payload_bytes"] != 54
        || receipts
            .iter()
            .any(|v| v["retained_custody"] != false || v["removal_authority"] != false)
        || statuses
            .iter()
            .any(|v| v["verified_present"] != true || v["historical_completion"] != true)
    {
        return Err("remote publication incomplete".into());
    }
    capture.record(
        "remote-payload-published",
        "payload_published",
        &before,
        &published,
        (w, checkpoint()?),
        json!({"apply":receipts,"status":statuses}),
    )?;
    receiver.corrupt(true)?;
    let before = receiver.snapshot()?;
    let w = checkpoint()?;
    let mut failures = Vec::new();
    for path in &paths {
        let (ok, v) = call("apply", &enabled, path)?;
        if ok || v["code"] != "integrity" {
            return Err("corrupt readback did not hold".into());
        }
        failures.push(v);
    }
    let held = receiver.snapshot()?;
    if !same_receiver(&before, &held) || w != checkpoint()? {
        return Err("held readback changed receiver or witness".into());
    }
    capture.record(
        "remote-readback-held",
        "readback_held",
        &before,
        &held,
        (w, checkpoint()?),
        json!({"apply":failures}),
    )?;
    receiver.corrupt(false)?;
    let before = receiver.snapshot()?;
    let w = checkpoint()?;
    let mut receipts = Vec::new();
    let mut statuses = Vec::new();
    for path in &paths {
        receipts.push(good("apply", &enabled, path)?);
        statuses.push(good("status", &enabled, path)?);
    }
    let restored = receiver.snapshot()?;
    if !same_receiver(&before, &restored)
        || w != checkpoint()?
        || statuses
            .iter()
            .any(|v| v["verified_present"] != true || v["historical_completion"] != true)
    {
        return Err("remote recheck changed publication".into());
    }
    capture.record(
        "remote-publication-rechecked",
        "publication_rechecked",
        &before,
        &restored,
        (w, checkpoint()?),
        json!({"apply":receipts,"status":statuses}),
    )?;
    let reports = capture.reports;
    let finished = receiver.finish()?;
    if finished != restored {
        return Err("receiver changed at shutdown".into());
    }
    Ok(
        json!({"reports":reports,"receiver_at_shutdown":finished,"fixture_input_staging_bytes":staged}),
    )
}
