#![allow(dead_code)]
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::TempDir;
use tig_core::{
    LogicalPath, Object,
    tree::{MemoryNodes, Tree, TreeLimits},
};
use tig_git::cache::{Admission, Layout, Place, Profile};
use tig_store::{Key, ledger::CapacityLedger};
pub fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
pub fn git_executable() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|p| p.join(format!("git{}", std::env::consts::EXE_SUFFIX)))
        .find(|p| p.is_absolute() && p.is_file())
        .unwrap()
}
pub struct Fixture {
    pub temp: TempDir,
    pub profile: Profile,
    pub objects: Vec<Object>,
}
impl Fixture {
    pub fn new(payloads: &[&[u8]]) -> Self {
        let temp = tempfile::Builder::new()
            .prefix("tig lfs fixture ")
            .tempdir()
            .unwrap();
        let cache = temp.path().join("cache");
        let aliases = temp.path().join("handoffs");
        let nodes = temp.path().join("nodes");
        fs::create_dir(&cache).unwrap();
        fs::create_dir(&aliases).unwrap();
        fs::create_dir(&nodes).unwrap();
        let objects: Vec<_> = payloads.iter().map(|b| Object::from_bytes(b)).collect();
        let mut bindings = BTreeMap::new();
        for (bytes, object) in payloads.iter().zip(&objects) {
            let path = Layout::ReelCas.object_path(&cache, &object.sha256);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
            bindings.insert(
                LogicalPath::parse(object.sha256.as_str()).unwrap(),
                object.clone(),
            );
        }
        let mut memory = MemoryNodes::default();
        let (tree, _) = Tree::build(&bindings, &mut memory, TreeLimits::default()).unwrap();
        for (id, bytes) in memory.objects() {
            fs::write(nodes.join(id.as_str()), bytes).unwrap();
        }
        let ledger_root = temp.path().join("ledger");
        let mut ledger =
            CapacityLedger::create(&ledger_root, key("volume"), key("epoch"), 0).unwrap();
        let checkpoint = ledger.checkpoint();
        let p = ledger.begin_probe(key("probe")).unwrap();
        ledger
            .observe(key("sample"), p.reference(), 1024 * 1024 * 1024)
            .unwrap();
        let job = ledger
            .reserve(key("job"), key("owner"), 32 * 1024 * 1024)
            .unwrap()
            .reference()
            .clone();
        let profile = Profile {
            schema: "tig.lfs-download.design-1".into(),
            authority_root: tree.root().clone(),
            authority_nodes: nodes,
            authority_checkpoint: None,
            max_authority_entries: 500_000,
            places: vec![Place {
                root: cache,
                layout: Layout::ReelCas,
                enabled: true,
                handoff_directory: aliases,
                admission: Admission {
                    ledger_root,
                    checkpoint,
                    job,
                },
            }],
            max_object_bytes: 64 * 1024 * 1024 * 1024,
            max_requests: 128,
            metadata_window: 65_536,
            min_free_bytes: 65_536,
        };
        Self {
            temp,
            profile,
            objects,
        }
    }
    pub fn profile_file(&self, name: &str) -> PathBuf {
        let path = self.temp.path().join(name);
        fs::write(&path, self.profile.to_json().unwrap()).unwrap();
        path
    }
    pub fn object_path(&self, i: usize) -> PathBuf {
        let p = &self.profile.places[0];
        p.layout.object_path(&p.root, &self.objects[i].sha256)
    }
    pub fn aliases(&self) -> Vec<PathBuf> {
        fs::read_dir(&self.profile.places[0].handoff_directory)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect()
    }
}
pub fn command(dir: &Path, args: &[&str]) -> Command {
    let mut c = Command::new("git");
    c.current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", dir.join("no-global-config"))
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_LFS_SKIP_SMUDGE", "1");
    for var in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
    ] {
        c.env_remove(var);
    }
    c
}
pub fn git(dir: &Path, args: &[&str]) -> Output {
    let output = command(dir, args).output().unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
