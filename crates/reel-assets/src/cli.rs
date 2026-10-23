use crate::{
    Result, admission, config, ingest, load, object, registry, resolve, selected, verify, versions,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

fn run(root: &Path, args: &[String]) -> Result<Value> {
    let command = args.first().ok_or("Missing command")?.as_str();
    let arg = |n: usize| -> Result<&str> {
        args.get(n)
            .map(String::as_str)
            .ok_or_else(|| "Missing argument".into())
    };
    match command {
        "collector-shadow" => {
            if args.len()!=2 {return Err("Usage: collector-shadow <private-job.json>".into());}
            let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
            crate::inventory_evidence::sidecar::file_store::collector::command::run(root,Path::new(arg(1)?),now)
        },
        "inventory-index-init" | "inventory-index-build" | "inventory-index-recover" | "inventory-index-shadow" => {
            let expected=if command=="inventory-index-shadow" {3}else{2};
            if args.len()!=expected {return Err("Usage: inventory-index-init|inventory-index-build|inventory-index-recover <profile.json> | inventory-index-shadow <profile.json> <exact-subjects.json>".into());}
            crate::inventory_evidence::sidecar::file_store::managed::command::run(root,command,Path::new(arg(1)?),args.get(2).map(Path::new))
        },
        "schema-catalogue" => {
            if args.len()!=2 {return Err("Usage: schema-catalogue <new-output.json>".into());}
            let value=crate::references::catalogue()?;
            crate::tiering::write_plan(Path::new(arg(1)?),&value)?;
            Ok(json!({"schema":"reel.asset-schema-export.v1","contracts":value["contracts"].as_array().unwrap().len(),"output":arg(1)?}))
        },
        "reference-inspect" | "reference-check" => {
            if args.len()!=3 {return Err("Usage: reference-inspect|reference-check <repository-relative-profile.json> <new-private-report.json>".into());}
            let report=crate::references::inspect(root,arg(1)?)?;
            crate::tiering::write_plan(Path::new(arg(2)?),&serde_json::to_value(&report)?)?;
            if command=="reference-check" && !report.findings.is_empty() {return Err(format!("Reference check has {} findings; private report written to {}",report.findings.len(),arg(2)?).into());}
            Ok(json!({"schema":report.schema,"nodes":report.nodes.len(),"edges":report.edges.len(),"findings":report.findings.len(),"report_sha256":report.report_sha256,"private_report":arg(2)?}))
        },
        "reference-query" | "reference-review" => {
            if args.len()!=3 {return Err("Usage: reference-query <private-report.json> <node-id> | reference-review <private-report.json> <new-private.html>".into());}
            let report:crate::references::Report=serde_json::from_slice(&crate::tiering::read_bounded(Path::new(arg(1)?),128*1024*1024)?)?;
            if command=="reference-query" {crate::references::query(&report,arg(2)?)}
            else {crate::references::review(&report,Path::new(arg(2)?))}
        },
        "manager-review" => {
            if args.len()!=3 && !(args.len()==4 && arg(3)?=="--refresh-inventory") { return Err("Usage: manager-review <repository-relative-project.json> <new-review-name> [--refresh-inventory]".into()); }
            let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
            crate::manager_review::run_with_refresh(root,arg(1)?,arg(2)?,now,args.len()==4)
        },
        "tier-review" | "tier-request" => {
            let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
            if command=="tier-review" && args.len()==3 {
                crate::tier_review::render(root,Path::new(arg(1)?),Path::new(arg(2)?),now)
            } else if command=="tier-request" && args.len()==4 {
                crate::tier_review::request(root,Path::new(arg(1)?),Path::new(arg(2)?),Path::new(arg(3)?),now)
            } else {Err("Usage: tier-review <consumer-scope-request.json> <new-private.html> | tier-request <consumer-scope-request.json> <block-choice.json> <new-request.json>".into())}
        },
        "consumer-protections" => {
            if args.len()!=3 {return Err("Usage: consumer-protections <request.json> <new-private-export.json>".into());}
            let mut export=crate::demotion::export_protections(root,Path::new(arg(1)?))?;
            crate::tiering::write_plan(Path::new(arg(2)?),&export)?;
            let count=export["protected_hashes"].as_object().unwrap().len();
            export.as_object_mut().unwrap().remove("protected_hashes");export["protected_objects"]=json!(count);Ok(export)
        },
        "tier-demote" => {
            if args.len()!=3 && !(args.len()==4 && arg(3)?=="--apply") { return Err("Usage: tier-demote <request.json> <absolute-owned-journal> [--apply]".into()); }
            let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
            crate::demotion::run(root,Path::new(arg(1)?),Path::new(arg(2)?),args.len()==4,now)
        },
        "tier-plan-check" => {
            if args.len()!=3 { return Err("Usage: tier-plan-check <policy.json> <plan.json>".into()); }
            let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
            crate::tiering::check_plan(root,Path::new(arg(1)?),Path::new(arg(2)?),now)
        },
        "tier-plan" => {
            if args.len()!=4 { return Err("Usage: tier-plan <policy.json> <baseline.json> <new-private-plan.json>".into()); }
            let destination=Path::new(arg(3)?);
            if destination.exists() { return Err("Plan output already exists".into()); }
            let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
            let mut plan=crate::tiering::plan(root,Path::new(arg(1)?),Path::new(arg(2)?),now)?;
            crate::tiering::write_plan(destination,&plan)?;
            plan.as_object_mut().unwrap().remove("objects");plan["private_plan_path"]=json!(destination);Ok(plan)
        },
        "cache-report" => {
            if args.get(1).is_some_and(|p|Path::new(p).exists()) {return Err("Report output already exists; choose a new private report path".into());}
            let report=crate::accounting::report_with_progress(root,|done,total|eprintln!("Cache metadata: {done}/{total} objects"))?;
            if let Some(destination)=args.get(1) {
                let path=Path::new(destination);
                let parent=path.parent().filter(|p|!p.as_os_str().is_empty()).unwrap_or(Path::new("."));
                let temp=crate::Temporary(crate::unique(parent,"cache-report"));
                let file=std::fs::OpenOptions::new().write(true).create_new(true).open(&temp.0)?;
                let mut buffered=std::io::BufWriter::with_capacity(256*1024,file);
                serde_json::to_writer(&mut buffered,&report)?;buffered.flush()?;buffered.get_ref().sync_all()?;drop(buffered);
                std::fs::hard_link(&temp.0,path)?;
                let mut summary=report;summary.as_object_mut().unwrap().remove("objects");summary["private_report_path"]=json!(path);Ok(summary)
            } else {let mut summary=report;summary.as_object_mut().unwrap().remove("objects");Ok(summary)}
        },
        "cache-observe" => {
            if args.len()!=3 {return Err("Usage: cache-observe <exact-hashes.json> <new-private-report.json>".into());}
            let hashes:BTreeSet<String>=serde_json::from_slice(&crate::tiering::read_bounded(Path::new(arg(1)?),64*1024*1024)?)?;
            let report=crate::accounting::observe_named(root,&hashes)?;
            crate::tiering::write_plan(Path::new(arg(2)?),&report)?;
            Ok(json!({"schema":"reel.asset-named-observation.v1","objects":hashes.len(),"archive_probed":false,"payload_bytes_copied":0,"private_report":arg(2)?}))
        },
        "artifact-check" | "artifact-verify" => crate::artifacts::check(root,Path::new(arg(1)?),args.get(2).map(String::as_str),command=="artifact-verify"),
        "artifact-locate" => crate::artifacts::locate(root,Path::new(arg(1)?),args.get(3).map(String::as_str),arg(2)?),
        "golden-list" => crate::goldens::list(root,arg(1)?),
        "golden-check" | "golden-verify" => crate::goldens::check(root,arg(1)?,arg(2)?,command=="golden-verify"),
        "golden-restore" => crate::goldens::restore(root,arg(1)?,arg(2)?),
        "scene-check" | "scene-verify" => crate::scenes::check(root,Path::new(arg(1)?),command=="scene-verify"),
        "scene-locate" => crate::scenes::locate(root,Path::new(arg(1)?),arg(2)?),
        "ingest" => ingest(root,Path::new(arg(1)?)),
        "register" => registry::register(root,Path::new(arg(1)?)),
        "compact-objects" => registry::compact(root),
        "compact-versions" => versions::compact(root),
        "compact-goldens" => crate::goldens::compact(root),
        "run-job" => crate::jobs::run(root,Path::new(arg(1)?)),
        "discard-job-scratch" => crate::jobs::discard(root,arg(1)?,arg(2)?,args.get(3).is_some_and(|a|a=="--discard-incomplete")),
        "verify-active" => {
            let oid=arg(1)?;let index=selected(root)?;let size=*index.get(oid).ok_or("Unregistered hash")?;
            let cfg=config(root)?;
            for (tier,cache) in cfg.cache_roots.iter().enumerate() {
                let path=object(cache,oid);
                if path.exists() {
                    verify(&path,oid,size)?;
                    return Ok(json!({"sha256":oid,"bytes":size,"path":path,"active_tier":tier,"verified":true,"payloads_copied":0}));
                }
            }
            Err("Named object absent from active caches; archive not probed".into())
        },
        "resolve" => {
            let oid=arg(1)?;let index=selected(root)?;let size=*index.get(oid).ok_or("Unregistered hash")?;
            let cfg=config(root)?;let path=resolve(root,&cfg,oid,size)?;
            Ok(json!({"sha256":oid,"bytes":size,"path":path,"verified":true}))
        },
        "locate" => {
            let oid=arg(1)?;let index=selected(root)?;let size=*index.get(oid).ok_or("Unregistered hash")?;
            let cfg=config(root)?;
            for cache in &cfg.cache_roots {
                let path=object(cache,oid);
                if path.exists() {
                    if path.metadata()?.len()!=size { return Err("Wrong object size".into()); }
                    return Ok(json!({"sha256":oid,"bytes":size,"path":path,"sha256_verified":false}));
                }
            }
            Err("Named object absent from active caches; use explicit resolve/fetch for archive recovery".into())
        },
        "fetch" | "fetch-existing" => {
            let oid=arg(1)?;let index=selected(root)?;let size=*index.get(oid).ok_or("Unregistered hash")?;
            let cfg=config(root)?;
            if size>cfg.capacity.max_object_bytes { return Err("Object exceeds configured operation budget".into()); }
            let existing=command=="fetch-existing";
            let reservation=if existing {
                let mut source=None;
                for cache in &cfg.cache_roots {
                    let path=object(cache,oid);
                    match std::fs::symlink_metadata(&path) {
                        Ok(_)=>{verify(&path,oid,size)?;source=Some(cache);break;},
                        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>(),
                        Err(e)=>return Err(e.into()),
                    }
                }
                // A same-volume disposable hard link requires no payload copy.
                admission::reserve_on(root,&cfg,source.ok_or("Named object absent from active caches; archive not probed")?,0)?
            } else { admission::reserve(root,&cfg,size)? };
            let storage=reservation.cache_root.join("lfs/music-videos");
            let mut child=Command::new("git").current_dir(root)
                .args(["-c",&format!("lfs.storage={}",storage.display()),"lfs","smudge","--",&format!("assets/objects/sha256/{}/{oid}",&oid[..2])])
                .env("CAMINOS_TRANSFER_CACHE",&reservation.cache_root).env("GIT_LFS_SKIP_SMUDGE","0")
                .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn()?;
            child.stdin.take().ok_or("Missing LFS stdin")?.write_all(registry::pointer(oid,size)?.as_bytes())?;
            let output=child.wait_with_output()?;
            if !output.status.success() { return Err(format!("Named LFS fetch failed: {}",String::from_utf8_lossy(&output.stderr)).into()); }
            let path=storage.join("objects").join(&oid[..2]).join(&oid[2..4]).join(oid);verify(&path,oid,size)?;
            Ok(json!({"sha256":oid,"bytes":size,"lfs_path":path,"cache_root":reservation.cache_root,"volume_id":reservation.volume_id,"verified":true,"daemon_started":false,"reused_active_object":existing,"payload_bytes_copied":if existing {Some(0u64)} else {None}}))
        },
        "version-manage" => {
            if args.len()!=6 && !(args.len()==7 && arg(6)?=="--apply") {return Err("Usage: version-manage <logical-id> <old-version> <new-version> <consumer-scope.json> <new-owned-directory> [--apply]".into());}
            crate::version_diff::manage(root,arg(1)?,arg(2)?,arg(3)?,Path::new(arg(4)?),Path::new(arg(5)?),args.len()==7)
        },
        "version-backup" => {
            if args.len()!=3 && !(args.len()==4 && arg(3)?=="--apply") {return Err("Usage: version-backup <request.json> <new-private-receipt.json> [--apply]".into());}
            crate::version_diff::backup(root,Path::new(arg(1)?),Path::new(arg(2)?),args.len()==4)
        },
        "tier-backup" => {
            if args.len()!=3 && !(args.len()==4 && arg(3)?=="--apply") {return Err("Usage: tier-backup <request.json> <new-private-receipt.json> [--apply]".into());}
            crate::version_diff::backup_tier(root,Path::new(arg(1)?),Path::new(arg(2)?),args.len()==4)
        },
        "version-plan" => {
            if args.len()!=7 {return Err("Usage: version-plan <old-logical-id> <old-version> <new-logical-id> <new-version> <consumer-scope.json> <new-owned-directory>".into());}
            let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
            crate::version_diff::plan_transition(root,(arg(1)?,arg(2)?),(arg(3)?,arg(4)?),Path::new(arg(5)?),Path::new(arg(6)?),now)
        },
        "version-management-check" => {
            if args.len()!=4 {return Err("Usage: version-management-check <request.json> <apply-result.json> <new-private-receipt.json>".into());}
            crate::version_diff::management_check(root,Path::new(arg(1)?),Path::new(arg(2)?),Path::new(arg(3)?))
        },
        "version-eviction-request" => {
            if args.len()!=7 {return Err("Usage: version-eviction-request <old-logical-id> <old-version> <new-logical-id> <new-version> <consumer-scope.json> <new-request.json>".into());}
            let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
            crate::version_diff::eviction_request(root,(arg(1)?,arg(2)?),(arg(3)?,arg(4)?),Path::new(arg(5)?),Path::new(arg(6)?),now)
        },
        "version-diff" => {
            if args.len()!=6 {return Err("Usage: version-diff <old-logical-id> <old-version> <new-logical-id> <new-version> <new-private-report.json>".into());}
            let report=crate::version_diff::compare_project(root,&versions::read(root)?,&selected(root)?,(arg(1)?,arg(2)?),(arg(3)?,arg(4)?))?;
            crate::tiering::write_plan(Path::new(arg(5)?),&report)?;
            Ok(json!({"schema":report["schema"],"added":report["added"].as_array().unwrap().len(),"retained":report["retained"].as_array().unwrap().len(),"dropped":report["dropped"].as_array().unwrap().len(),"eviction_candidates":report["eviction_candidate_sha256s"].as_array().unwrap().len(),"candidate_logical_bytes":report["candidate_logical_bytes"],"removal_authorized":false,"private_report_path":arg(5)?}))
        },
        "register-version" => {
            let version: versions::Version=serde_json::from_value(load(Path::new(arg(1)?))?)?;
            if root.join("asset-project.json").exists() && crate::accounting::project(root)?.require_primary_input_contracts && version.dependency_contract.is_none() {return Err("Project requires an explicit primary-input/output/reference contract".into());}
            let index=selected(root)?;
            if !index.contains_key(&version.manifest_sha256) || version.payloads.iter().any(|h|!index.contains_key(h)) { return Err("Version manifest and payload hashes must be registered assets".into()); }
            versions::transact(root,|s|{s.register(version)?;Ok(json!({"generation":s.generation}))})
        },
        "unselect" => versions::transact(root,|s|{s.unselect(arg(1)?,arg(2)?)?;Ok(json!({"generation":s.generation,"bytes_removed":0,"immutable_version_retained":true}))}),
        "select" => versions::transact(root,|s|{
            if root.join("asset-project.json").exists() && crate::accounting::project(root)?.require_primary_input_contracts && s.versions.get(&format!("{}@{}",arg(1)?,arg(2)?)).is_none_or(|v|v.dependency_contract.is_none()) {return Err("Project requires classified primary-input dependencies before selection".into());}
            s.select(arg(1)?,arg(2)?)?;Ok(json!({"generation":s.generation}))
        }),
        "pin" => versions::transact(root,|s|{
            let key=format!("{}@{}",arg(1)?,arg(2)?);
            if !s.versions.contains_key(&key) {return Err("Cannot pin an unknown version".into());}
            if s.pins.insert(key) {s.generation+=1;}
            Ok(json!({"generation":s.generation,"bytes_removed":0}))
        }),
        "deprecate" => versions::transact(root,|s|{s.deprecate(arg(1)?,arg(2)?)?;Ok(json!({"generation":s.generation,"bytes_removed":0}))}),
        "lease" => versions::transact(root,|s|{s.lease(arg(1)?,args.iter().skip(2).cloned().collect::<BTreeSet<_>>())?;Ok(json!({"generation":s.generation}))}),
        "release-lease" => versions::transact(root,|s|{s.release(arg(1)?)?;Ok(json!({"generation":s.generation}))}),
        "retirement-plan" => versions::transact(root,|s|s.retirement_plan(arg(1)?,arg(2)?)),
        "check-plan" => {
            let plan=load(Path::new(arg(1)?))?;
            versions::transact(root,|s|{s.validate_plan(&plan)?;Ok(json!({"generation":s.generation,"fresh":true,"removal_authorized":false}))})
        },
        "release-reservation" => {admission::release_stale(root,&config(root)?,arg(1)?)?;Ok(json!({"released":arg(1)?}))},
        _=>Err("Commands include: collector-shadow, inventory-index-init, inventory-index-build, inventory-index-shadow, inventory-index-recover, ingest, register, verify-active, locate, resolve, fetch, register-version, select, deprecate, lease, release-lease, retirement-plan, check-plan, release-reservation".into()),
    }
}
pub fn main() {
    let result = (|| -> Result<Value> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args.len() < 3 || args[0] != "--root" {
            return Err("Usage: reel-assets --root <repository> <command> [args]".into());
        }
        run(&PathBuf::from(&args[1]).canonicalize()?, &args[2..])
    })();
    match result {
        Ok(value) => println!("{value}"),
        Err(error) => {
            eprintln!("reel-assets: {error}");
            std::process::exit(1);
        }
    }
}
