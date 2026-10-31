use reel::{
    media_runtime::ByteIdentity,
    tig_job_runner::{self, Profile},
    tig_scene_production::jobs::{Coordination, Identity, JobPlan},
};
use reel_assets::{Config, PathPolicy};
use serde_json::{Value, json};
use std::{fs, path::Path};
use tig_core::{
    LogicalPath,
    tree::{NodeStore, Tree, TreeLimits},
};
use tig_store::{
    Key,
    metadata::{Checkpoint, MetadataReader, MetadataStore},
    source_claims::SourceJournal,
};
#[path = "tig_runner_recovery.rs"]
mod recovery;
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}

pub fn exercise(
    plan: &JobPlan<'_>,
    journal: &mut SourceJournal,
    root: &Path,
    metadata: (&Path, &Checkpoint),
) {
    let project = root.join("runner-project");
    fs::create_dir(&project).unwrap();
    let cache = root.join("runner-active");
    fs::create_dir_all(cache.join("objects/sha256")).unwrap();
    let control = root.join("runner-control");
    fs::create_dir(&control).unwrap();
    let coordination = Coordination {
        root: control.clone(),
    };
    let mut cfg = Config {
        path_policy: PathPolicy::ConfiguredLocalRoots,
        cache_roots: vec![cache.clone()],
        coordination_root: Some(control),
        ..Default::default()
    };
    cfg.capacity.minimum_free_bytes = 0;
    cfg.capacity.minimum_free_basis_points = 0;
    let ffmpeg = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|p| p.join("ffmpeg.exe"))
        .find(|p| p.is_file())
        .unwrap()
        .canonicalize()
        .unwrap();
    let tool = ByteIdentity::observe_file(&ffmpeg).unwrap();
    let mut writer = MetadataStore::open(metadata.0, metadata.1).unwrap();
    let frozen = plan.freeze(&mut writer, TreeLimits::default()).unwrap();
    let witness = writer.finish().unwrap();
    drop(writer);
    let mut reader = MetadataReader::open(metadata.0, &witness).unwrap();
    let resources = plan
        .readback(frozen.selection(), &mut reader, TreeLimits::default())
        .unwrap();
    for mode in ["wrong-role", "success", "overbudget"] {
        let identity = Identity {
            source: key(&format!("runner-{mode}")),
            owner: key("runner-owner"),
        };
        let protected = plan
            .readback(frozen.selection(), &mut reader, TreeLimits::default())
            .unwrap()
            .protect(
                journal,
                &coordination,
                identity.clone(),
                0,
                &key("runner-start"),
            )
            .unwrap();
        drop(protected); // Durable roots survive; the runner takes its same source fence.
        let args = if mode == "overbudget" {
            vec![
                "-v",
                "error",
                "-nostdin",
                "-n",
                "-re",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=48000:cl=stereo",
                "-t",
                "10",
                "-c:a",
                "pcm_s16le",
                "scratch.wav",
            ]
        } else {
            vec![
                "-v",
                "error",
                "-nostdin",
                "-n",
                "-f",
                "lavfi",
                "-i",
                "color=red:s=16x16:r=1",
                "-frames:v",
                "1",
                "-c:v",
                "ffv1",
                "out.mkv",
            ]
        };
        let mut manifest = json!({"budget_bytes":if mode=="overbudget"{256*1024}else{8*1024*1024},"scratch_bytes":64*1024,
            "inputs":{},"tools":{},"installed_tools":{"encoder":{"path":ffmpeg,"sha256":tool.sha256,"bytes":tool.bytes}},
            "steps":[{"tool":"encoder","args":args,"timeout_seconds":10}],
            "outputs":{"video":{"path":"out.mkv","max_bytes":if mode=="overbudget"{1024}else{1024*1024}}}});
        if mode == "wrong-role" {
            manifest["budget_bytes"] = json!(tool.bytes + 8 * 1024 * 1024);
            manifest["inputs"] =
                json!({"not-primary.bin":{"sha256":tool.sha256,"bytes":tool.bytes}});
        }
        let path = root.join(format!("runner-{mode}.json"));
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let mut profile = Profile {
            config: &cfg,
            coordination: &coordination,
            identity: identity.clone(),
            expected_revision: 2,
            operation: key(&format!("runner-retain-{mode}")),
            metadata_root: metadata.0,
            metadata_minimum: &witness,
            tree_limits: TreeLimits::default(),
        };
        if mode == "success" {
            let before = journal.checkpoint();
            profile.identity.owner = key("foreign-runner-owner");
            assert!(
                tig_job_runner::run(&project, &path, resources.resources(), journal, &profile)
                    .is_err()
            );
            profile.identity.owner = identity.owner.clone();
            profile.expected_revision = 1;
            assert!(
                tig_job_runner::run(&project, &path, resources.resources(), journal, &profile)
                    .is_err()
            );
            profile.expected_revision = 2;
            let held = plan
                .readback(frozen.selection(), &mut reader, TreeLimits::default())
                .unwrap()
                .resume(journal, &coordination, identity.clone(), 2)
                .unwrap();
            assert!(
                tig_job_runner::run(&project, &path, resources.resources(), journal, &profile)
                    .is_err()
            );
            drop(held);
            assert_eq!(journal.checkpoint(), before);
            assert!(!cache.join("staging/caminos").exists());
        }
        let result = tig_job_runner::run(&project, &path, resources.resources(), journal, &profile);
        if mode == "wrong-role" {
            let error = result.unwrap_err().to_string();
            assert!(error.contains("admitted Tig role"), "{error}");
            assert!(!cache.join("staging/caminos").exists());
            assert_eq!(
                journal.source(&identity.source).unwrap().unwrap().revision,
                2
            );
            continue;
        }
        if mode == "success" {
            let result = result.unwrap();
            assert_eq!(result["workspace_retained"], false);
            let output = &result["outputs"]["video"];
            assert!(
                fs::metadata(output["path"].as_str().unwrap())
                    .unwrap()
                    .len()
                    > 0
            );
            assert!(result["retirement_evidence"]["sha256"].as_str().is_some());
        } else {
            let error = result.unwrap_err().to_string();
            assert!(error.contains("workspace budget"), "{mode}: {error}");
        }
        let state = journal.source(&identity.source).unwrap().unwrap();
        assert_eq!(state.revision, 4);
        assert_eq!(state.predecessors.len(), 1);
        let selection = state.current.unwrap().selection;
        let mut latest = MetadataReader::open(metadata.0, &witness).unwrap();
        let (header, _) = Tree::from_root(selection.root.clone())
            .get(
                &LogicalPath::parse("retention/identity").unwrap(),
                &mut latest,
                TreeLimits::default(),
            )
            .unwrap();
        let header: Value =
            serde_json::from_slice(&latest.get(&header.unwrap()).unwrap().unwrap()).unwrap();
        assert_eq!(header["succeeded"], mode == "success");
        assert!(header["production_claim"].is_null());
        recovery::exercise(
            resources.resources(),
            journal,
            &root.join("job-owner"),
            &profile,
            &selection,
            mode == "success",
        );
        assert!(
            fs::read_dir(cache.join("staging/caminos"))
                .unwrap()
                .next()
                .is_none()
        );
    }
    assert!(!project.join("catalog.json").exists());
    assert!(!project.join("registry").exists());
    assert!(!journal.obligations(100).unwrap().complete_claim_coverage());
}
