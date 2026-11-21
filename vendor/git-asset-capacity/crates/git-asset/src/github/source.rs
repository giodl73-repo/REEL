//! Bounded metadata-only GitHub source; no local Git fetch or checkout.
use super::{http::Client, *};
use crate::{
    lifecycle::{self, Budget, CommittedSource, FlatCapture},
    pointer::Pointer,
};
use base64::engine::general_purpose::STANDARD;
use git_asset_core::{LogicalPath, Object};
use std::collections::BTreeMap;
pub struct Source<'a> {
    pub client: &'a Client,
    pub trust: &'a Trust,
}
fn mapped(error: Error) -> lifecycle::Error {
    match error {
        Error::Budget => lifecycle::Error::Budget,
        Error::Stale => lifecycle::Error::Stale,
        Error::Trust | Error::Denied => lifecycle::Error::Authority,
        _ => lifecycle::Error::Git,
    }
}
struct Work<'a> {
    source: &'a Source<'a>,
    budget: &'a Budget,
    start: Instant,
    requests: u32,
    bytes: usize,
    tree_entries: usize,
}
impl Work<'_> {
    fn get(&mut self, suffix: &str) -> Result<serde_json::Value> {
        self.get_capped(suffix, 16 * 1024 * 1024)
    }
    fn get_capped(&mut self, suffix: &str, cap: usize) -> Result<serde_json::Value> {
        if self.requests >= self.budget.nodes
            || self.start.elapsed() >= Duration::from_millis(self.budget.millis)
        {
            return Err(Error::Budget);
        }
        self.requests += 1;
        let left = (self.budget.node_bytes as usize)
            .checked_sub(self.bytes)
            .ok_or(Error::Budget)?
            .min(cap);
        let remaining = Duration::from_millis(self.budget.millis)
            .checked_sub(self.start.elapsed())
            .ok_or(Error::Budget)?;
        let (value, n) = self.source.client.get_for(
            &format!(
                "{}/repos/{}/git/{}",
                self.source.client.api(),
                self.source.trust.repository,
                suffix
            ),
            true,
            remaining,
            left,
        )?;
        self.bytes += n;
        Ok(value)
    }
    fn tree(&mut self, oid: &GitId) -> Result<Vec<serde_json::Value>> {
        let value = self.get(&format!("trees/{}", oid.as_str()))?;
        if value["sha"].as_str() != Some(oid.as_str())
            || value["truncated"].as_bool() != Some(false)
        {
            return Err(Error::Invalid);
        }
        let entries = value["tree"].as_array().ok_or(Error::Invalid)?;
        self.tree_entries = self
            .tree_entries
            .checked_add(entries.len())
            .ok_or(Error::Budget)?;
        if self.tree_entries > self.budget.nodes as usize {
            return Err(Error::Budget);
        }
        let mut names = BTreeSet::new();
        for entry in entries {
            let name = entry["path"].as_str().ok_or(Error::Invalid)?;
            if name.contains('/') || LogicalPath::parse(name).is_err() || !names.insert(name) {
                return Err(Error::Invalid);
            }
            GitId::parse(entry["sha"].as_str().ok_or(Error::Invalid)?)
                .map_err(|_| Error::Invalid)?;
        }
        Ok(entries.clone())
    }
    fn blob(&mut self, entry: &serde_json::Value) -> Result<(GitId, Object)> {
        if entry["type"].as_str() != Some("blob")
            || !matches!(entry["mode"].as_str(), Some("100644" | "100755"))
        {
            return Err(Error::Invalid);
        }
        let bytes = entry["size"].as_u64().ok_or(Error::Invalid)?;
        if bytes > 1024 {
            return Err(Error::Budget);
        }
        let oid = GitId::parse(entry["sha"].as_str().ok_or(Error::Invalid)?)
            .map_err(|_| Error::Invalid)?;
        let value = self.get_capped(&format!("blobs/{}", oid.as_str()), 8192)?;
        if value["sha"].as_str() != Some(oid.as_str())
            || value["size"].as_u64() != Some(bytes)
            || value["encoding"].as_str() != Some("base64")
        {
            return Err(Error::Invalid);
        }
        let content = value["content"].as_str().ok_or(Error::Invalid)?;
        let content = content
            .bytes()
            .filter(|b| *b != b'\n' && *b != b'\r')
            .collect::<Vec<_>>();
        let raw = STANDARD.decode(&content).map_err(|_| Error::Invalid)?;
        if raw.len() as u64 != bytes {
            return Err(Error::Invalid);
        }
        oid.verify_blob(&raw).map_err(|_| Error::Signature)?;
        let pointer = Pointer::parse(&raw).map_err(|_| Error::Invalid)?;
        Ok((oid, pointer.object().clone()))
    }
}
impl CommittedSource for Source<'_> {
    fn named_commit(&self, name: &str) -> lifecycle::Result<GitId> {
        if name != self.trust.ref_name {
            return Err(lifecycle::Error::Authority);
        }
        self.client.current_head(self.trust).map_err(mapped)
    }
    fn flat_capture(
        &self,
        commit: GitId,
        scope: &[LogicalPath],
        budget: &Budget,
    ) -> lifecycle::Result<FlatCapture> {
        self.capture(commit, scope, budget).map_err(mapped)
    }
}
impl Source<'_> {
    fn capture(
        &self,
        commit: GitId,
        scope: &[LogicalPath],
        budget: &Budget,
    ) -> Result<FlatCapture> {
        self.trust.id()?;
        if scope.is_empty()
            || scope.len() > 128
            || budget.entries == 0
            || budget.entries > 32768
            || budget.nodes == 0
            || budget.nodes > 500000
            || budget.node_bytes == 0
            || budget.node_bytes > 32 * 1024 * 1024
            || budget.millis == 0
            || budget.millis > 600000
        {
            return Err(Error::Budget);
        }
        let mut work = Work {
            source: self,
            budget,
            start: Instant::now(),
            requests: 0,
            bytes: 0,
            tree_entries: 0,
        };
        let info = work.get(&format!("commits/{}", commit.as_str()))?;
        if info["sha"].as_str() != Some(commit.as_str()) {
            return Err(Error::Trust);
        }
        let root = GitId::parse(info["tree"]["sha"].as_str().ok_or(Error::Invalid)?)
            .map_err(|_| Error::Invalid)?;
        let mut stack = vec![(String::new(), root)];
        let mut bindings = BTreeMap::new();
        let mut listing = BTreeMap::new();
        while let Some((prefix, tree)) = stack.pop() {
            for entry in work.tree(&tree)? {
                let name = entry["path"].as_str().ok_or(Error::Invalid)?;
                let logical = if prefix.is_empty() {
                    name.to_owned()
                } else {
                    format!("{prefix}/{name}")
                };
                if logical.len() > 4096 || logical.split('/').count() > 64 {
                    return Err(Error::Budget);
                }
                LogicalPath::parse(&logical).map_err(|_| Error::Invalid)?;
                let selected = scope.iter().any(|s| {
                    logical == s.as_str()
                        || logical
                            .strip_prefix(s.as_str())
                            .is_some_and(|s| s.starts_with('/'))
                });
                let ancestor = scope.iter().any(|s| {
                    s.as_str()
                        .strip_prefix(&logical)
                        .is_some_and(|s| s.starts_with('/'))
                });
                if !selected && !ancestor {
                    continue;
                }
                match entry["type"].as_str() {
                    Some("tree") if entry["mode"].as_str() == Some("040000") => stack.push((
                        logical,
                        GitId::parse(entry["sha"].as_str().ok_or(Error::Invalid)?)
                            .map_err(|_| Error::Invalid)?,
                    )),
                    Some("blob") if selected => {
                        if bindings.len() >= budget.entries as usize {
                            return Err(Error::Budget);
                        }
                        let (oid, object) = work.blob(&entry)?;
                        let logical = LogicalPath::parse(&logical).map_err(|_| Error::Invalid)?;
                        if bindings.insert(logical.clone(), object).is_some() {
                            return Err(Error::Invalid);
                        }
                        listing.insert(logical.as_str().to_owned(), oid.as_str().to_owned());
                    }
                    _ => return Err(Error::Invalid),
                }
            }
        }
        // Distinct adapter/listing contract; switching from local ls-tree needs
        // independent private adoption rather than pretending identical context.
        let listing = identity::canonical_json(
            &serde_json::to_value(&(
                "tig.github-flat-listing.design-1",
                &self.trust.repository_id,
                commit.as_str(),
                scope,
                listing,
            ))
            .map_err(|_| Error::Invalid)?,
        )
        .map_err(|_| Error::Invalid)?;
        if listing.len() as u64 > budget.node_bytes {
            return Err(Error::Budget);
        }
        let listing = Digest::blob(&listing);
        Ok(FlatCapture { listing, bindings })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };
    fn trust() -> Trust {
        Trust {
            schema: "tig.github-trust.design-1".into(),
            audience: "git-asset:fixture".into(),
            subject: "fixture".into(),
            repository: "fixture/repo".into(),
            repository_id: "42".into(),
            owner_id: "7".into(),
            ref_name: "refs/heads/main".into(),
            workflow_ref: "fixture/repo/.github/workflows/git-asset.yml@refs/heads/main".into(),
            reusable: None,
            events: vec!["push".into()],
            max_token_seconds: 600,
            clock_skew_seconds: 0,
            selection_enrollment: Digest::blob(b"selection"),
            placement_enrollment: Digest::blob(b"placement"),
        }
    }
    fn budget() -> Budget {
        Budget {
            entries: 32,
            nodes: 128,
            node_bytes: 1024 * 1024,
            millis: 10000,
        }
    }
    fn peer(responses: Vec<Value>) -> (Client, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let h = thread::spawn(move || {
            let mut requests = Vec::new();
            for value in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut b = [0; 1];
                while !bytes.ends_with(b"\r\n\r\n") {
                    assert!(bytes.len() < 16384);
                    stream.read_exact(&mut b).unwrap();
                    bytes.push(b[0]);
                }
                requests.push(String::from_utf8(bytes).unwrap());
                let body = value.to_string();
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            }
            requests
        });
        (Client::fixture_api(base), h)
    }
    fn responses() -> (GitId, Object, Vec<Value>) {
        use sha1::{Digest as _, Sha1};
        let commit = GitId::parse(&"a".repeat(40)).unwrap();
        let object = Object::from_bytes(b"synthetic payload");
        let pointer = Pointer::new(object.clone()).to_bytes();
        let mut hash = Sha1::new();
        hash.update(format!("blob {}\0", pointer.len()).as_bytes());
        hash.update(&pointer);
        let oid = hash
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let r = vec![
            json!({"sha":commit.as_str(),"tree":{"sha":"b".repeat(40)}}),
            json!({"sha":"b".repeat(40),"truncated":false,"tree":[{"path":"assets","mode":"040000","type":"tree","sha":"c".repeat(40)},{"path":"excluded.bin","type":"blob","mode":"100644","sha":"d".repeat(40),"size":999999999}]}),
            json!({"sha":"c".repeat(40),"truncated":false,"tree":[{"path":"one.bin","mode":"100644","type":"blob","sha":oid,"size":pointer.len()}]}),
            json!({"sha":oid,"encoding":"base64","size":pointer.len(),"content":STANDARD.encode(pointer)}),
        ];
        (commit, object, r)
    }
    #[test]
    fn bounded_explicit_pointer_scope_verifies_blob_and_never_opens_excluded_payload() {
        let (commit, object, responses) = responses();
        let (client, server) = peer(responses);
        let trust = trust();
        let source = Source {
            client: &client,
            trust: &trust,
        };
        let captured = source
            .flat_capture(commit, &[LogicalPath::parse("assets").unwrap()], &budget())
            .unwrap();
        assert_eq!(
            captured.bindings,
            [(LogicalPath::parse("assets/one.bin").unwrap(), object)]
                .into_iter()
                .collect()
        );
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 4);
        assert!(requests[3].starts_with("GET /repos/fixture/repo/git/blobs/"));
        assert!(!requests.iter().any(|s| s.contains(&"d".repeat(40))));
    }
    #[test]
    fn truncated_tree_corrupt_blob_and_declared_large_control_hold_without_empty_success() {
        for kind in 0..3 {
            let (commit, _, mut responses) = responses();
            match kind {
                0 => {
                    responses[1]["truncated"] = json!(true);
                    responses.truncate(2);
                }
                1 => {
                    responses[3]["content"] =
                        json!(
                            STANDARD.encode(vec![
                                b'x';
                                responses[3]["size"].as_u64().unwrap() as usize
                            ])
                        )
                }
                _ => {
                    responses[2]["tree"][0]["size"] = json!(1025);
                    responses.truncate(3);
                }
            }
            let (client, server) = peer(responses);
            let trust = trust();
            let source = Source {
                client: &client,
                trust: &trust,
            };
            assert!(
                source
                    .flat_capture(commit, &[LogicalPath::parse("assets").unwrap()], &budget())
                    .is_err()
            );
            server.join().unwrap();
        }
    }
    #[test]
    fn aggregate_request_budget_is_not_reset_for_subtrees_and_missing_scope_is_explicitly_empty() {
        let (commit, _, mut responses) = responses();
        responses.truncate(2);
        let (client, server) = peer(responses);
        let trust = trust();
        let source = Source {
            client: &client,
            trust: &trust,
        };
        let mut small = budget();
        small.nodes = 2;
        assert!(matches!(
            source.flat_capture(commit, &[LogicalPath::parse("assets").unwrap()], &small),
            Err(lifecycle::Error::Budget)
        ));
        server.join().unwrap();
        let (commit, _, mut responses) = self::responses();
        responses.truncate(2);
        let (client, server) = peer(responses);
        let source = Source {
            client: &client,
            trust: &trust,
        };
        assert!(
            source
                .flat_capture(commit, &[LogicalPath::parse("absent").unwrap()], &budget())
                .unwrap()
                .bindings
                .is_empty()
        );
        server.join().unwrap();
    }

    #[test]
    fn admitted_remote_source_drives_existing_atomic_selection_and_persisted_readback() {
        use crate::lifecycle::{Enrollment, Execution};
        use git_asset_core::tree::{MemoryNodes, Tree, TreeLimits};
        use git_asset_store::{
            Key,
            metadata::{Limits as MetadataLimits, MetadataStore},
            protection::Limits,
            source_claims::{
                Command, Kind, Places,
                admission::{Namespace, Policy},
            },
        };
        let key = |s: &str| Key::parse(s).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let places = Places {
            current: vec![key("working"), key("backup")],
            pending: vec![key("working"), key("backup")],
            predecessor: vec![key("history"), key("backup")],
        };
        let policy = Policy {
            prefer_active: true,
            classes: [Kind::Branch, Kind::Draft, Kind::Job, Kind::Retention]
                .into_iter()
                .map(|k| (k, places.clone()))
                .collect(),
        };
        let mut ns = Namespace::create(
            &temp.path().join("namespace"),
            key("fixture"),
            key("epoch"),
            policy,
        )
        .unwrap();
        let limits = Limits {
            millis: 10000,
            roots: 32,
            bindings: 1024,
            associations: 2048,
            reads: 10000,
            read_bytes: 2 * 1024 * 1024,
        };
        ns.apply(
            key("enroll"),
            Command::Enroll {
                source: key("main"),
                owner: key("owner"),
                class: Kind::Branch,
            },
            &MemoryNodes::default(),
            &limits,
        )
        .unwrap();
        let mut metadata = MetadataStore::create(
            &temp.path().join("metadata"),
            MetadataLimits {
                nodes: 1000,
                bytes: 1024 * 1024,
                database_bytes: 4 * 1024 * 1024,
                min_free_bytes: 65536,
            },
        )
        .unwrap();
        let (commit, object, frames) = responses();
        let current = vec![
            json!({"id":42,"owner":{"id":7},"full_name":"fixture/repo"}),
            json!({"ref":"refs/heads/main","object":{"type":"commit","sha":commit.as_str()}}),
        ];
        let mut replies = current.clone();
        replies.extend(frames.clone());
        replies.extend(current.clone());
        replies.extend(current.clone());
        replies.extend(frames);
        replies.extend(current.clone());
        replies.extend(current);
        let (client, server) = peer(replies);
        let trust = trust();
        let source = Source {
            client: &client,
            trust: &trust,
        };
        let enrollment = Enrollment {
            repository: Digest::blob(b"admitted Github source contract"),
            authority: Digest::blob(b"owner authority"),
            namespace: ns.identity().clone(),
            policy: hash(ns.policy()).unwrap(),
            source: key("main"),
            owner: key("owner"),
            class: Kind::Branch,
            ref_name: "refs/heads/main".into(),
            scope: vec![LogicalPath::parse("assets").unwrap()],
            budget: budget(),
        };
        let mut execution = Execution {
            enrollment: &enrollment,
            repository: &source,
            namespace: &mut ns,
            metadata: &mut metadata,
            allow_selection: true,
        };
        let plan = execution.plan(Some(&commit)).unwrap();
        let status = execution.apply(&plan).unwrap();
        assert!(status.desired_current);
        assert_eq!(status.observed_ref_current, Some(true));
        assert_eq!(status.storage_converged, None);
        let entries = Tree::from_root(plan.selection.root)
            .entries(
                &mut metadata.finished_reader().unwrap(),
                TreeLimits::default(),
            )
            .unwrap()
            .0;
        assert_eq!(
            entries,
            [(LogicalPath::parse("assets/one.bin").unwrap(), object)]
                .into_iter()
                .collect()
        );
        assert_eq!(server.join().unwrap().len(), 18);
    }

    #[test]
    fn real_signed_request_reconciles_three_stores_under_independent_private_enrollment() {
        signed_reconciliation_case("complete");
    }
    #[test]
    fn signed_request_expiry_after_selection_stops_new_pages() {
        signed_reconciliation_case("expired");
    }
    #[test]
    fn signed_request_observed_clock_rollback_stops_new_pages() {
        signed_reconciliation_case("clock");
    }
    #[test]
    fn signed_request_head_advance_after_copy_preserves_progress_without_convergence() {
        signed_reconciliation_case("advanced");
    }
    #[test]
    fn signed_request_source_outage_after_selection_preserves_acknowledgement() {
        signed_reconciliation_case("outage");
    }
    #[test]
    fn signed_request_completes_multiple_pages_only_under_one_fresh_basis() {
        signed_reconciliation_case("paged");
    }
    #[test]
    fn signed_request_page_budget_reports_partial_copies_and_continuation() {
        signed_reconciliation_case("pagebudget");
    }
    #[test]
    fn repeated_signed_request_reuses_original_selection_and_placements() {
        signed_reconciliation_case("retry");
    }
    #[test]
    fn settled_source_cannot_claim_convergence_merely_because_github_ref_is_unchanged() {
        signed_reconciliation_case("withdrawn");
    }
    fn signed_reconciliation_case(mode: &str) {
        use crate::github::{
            fixture::Fixture,
            placement::{Budget as RunBudget, Execution as Coordinator, Permissions},
        };
        use crate::lifecycle::Enrollment;
        use git_asset_core::tree::MemoryNodes;
        use git_asset_store::{
            Key,
            existing::ByteSource,
            ledger::CapacityLedger,
            local::LocalStore,
            local_capacity::LocalCapacityProbe,
            metadata::{Limits as MetadataLimits, MetadataStore},
            orchestration::Place,
            placement::{self, Authority, Config, Mapping, Provider, Role},
            protection::Limits,
            source_claims::{
                Command, Kind, Places,
                admission::{Namespace, Policy},
            },
            store_protection::{Binding, Member},
        };
        struct Seed(Vec<u8>);
        impl ByteSource for Seed {
            type Reader = std::io::Cursor<Vec<u8>>;
            fn open_verified(
                &mut self,
                object: &Object,
            ) -> git_asset_store::existing::Result<Self::Reader> {
                let bytes = if &Object::from_bytes(&self.0) == object {
                    self.0.clone()
                } else {
                    let other = b"another synthetic payload".to_vec();
                    assert_eq!(&Object::from_bytes(&other), object);
                    other
                };
                Ok(std::io::Cursor::new(bytes))
            }
        }
        let key = |s: &str| Key::parse(s).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let places = Places {
            current: vec![key("working"), key("backup")],
            pending: vec![key("working"), key("backup")],
            predecessor: vec![key("history"), key("backup")],
        };
        let policy = Policy {
            prefer_active: true,
            classes: [Kind::Branch, Kind::Draft, Kind::Job, Kind::Retention]
                .into_iter()
                .map(|k| (k, places.clone()))
                .collect(),
        };
        let mut ns = Namespace::create(
            &temp.path().join("namespace"),
            key("fixture"),
            key("epoch"),
            policy,
        )
        .unwrap();
        let limits = Limits {
            millis: 10000,
            roots: 32,
            bindings: 1024,
            associations: 2048,
            reads: 10000,
            read_bytes: 2 * 1024 * 1024,
        };
        ns.apply(
            key("enroll"),
            Command::Enroll {
                source: key("main"),
                owner: key("owner"),
                class: Kind::Branch,
            },
            &MemoryNodes::default(),
            &limits,
        )
        .unwrap();
        let mut metadata = MetadataStore::create(
            &temp.path().join("metadata"),
            MetadataLimits {
                nodes: 1000,
                bytes: 1024 * 1024,
                database_bytes: 4 * 1024 * 1024,
                min_free_bytes: 65536,
            },
        )
        .unwrap();
        let enrollment = Enrollment {
            repository: Digest::blob(b"admitted Github source contract"),
            authority: Digest::blob(b"owner authority"),
            namespace: ns.identity().clone(),
            policy: hash(ns.policy()).unwrap(),
            source: key("main"),
            owner: key("owner"),
            class: Kind::Branch,
            ref_name: "refs/heads/main".into(),
            scope: vec![LogicalPath::parse("assets").unwrap()],
            budget: budget(),
        };
        let mut stores = Vec::new();
        let mut ledgers = Vec::new();
        let mut jobs = Vec::new();
        let mut providers = Vec::new();
        for (name, role) in [
            ("working", Role::Cache),
            ("history", Role::Cache),
            ("backup", Role::Retained),
        ] {
            let root = temp.path().join(name);
            let store = if role == Role::Cache {
                LocalStore::create_managed(
                    &root,
                    key(name),
                    Binding::new(
                        1,
                        Digest::blob(b"trusted clients"),
                        vec![Member {
                            namespace: ns.identity().clone(),
                            places: vec![key(name)],
                        }],
                    )
                    .unwrap(),
                )
                .unwrap()
            } else {
                LocalStore::create(&root, key(name)).unwrap()
            };
            let mut ledger = CapacityLedger::create(
                &temp.path().join(format!("ledger-{name}")),
                key(name),
                key("epoch"),
                0,
            )
            .unwrap();
            LocalCapacityProbe::bind(&store, &mut ledger)
                .unwrap()
                .observe(&store, &mut ledger, key("probe"), key("sample"))
                .unwrap();
            let job = ledger
                .reserve(key("controller"), key("owner"), 256 * 1024 * 1024)
                .unwrap()
                .reference()
                .clone();
            providers.push(Provider {
                place: Place::local(&store, 1).unwrap(),
                role,
                failure_domain: key(name),
                capacity: ledger.identity().clone(),
                job: job.clone(),
                assignments: vec![Mapping {
                    namespace: ns.identity().clone(),
                    place: key(name),
                }],
            });
            stores.push(store);
            ledgers.push(ledger);
            jobs.push(job);
        }
        let config = Config {
            schema: "tig.placement-enrollment.design-1".into(),
            owner: key("owner"),
            authority: enrollment.authority.clone(),
            seed: Digest::blob(b"seed"),
            providers,
            namespaces: vec![Authority {
                namespace: ns.identity().clone(),
                policy: enrollment.policy.clone(),
            }],
            minimum_domains: 1,
            budget: placement::Budget {
                objects: 1,
                max_object_bytes: 1024,
                max_stream_bytes: 1024,
                max_peak_bytes: 256 * 1024 * 1024,
                limits,
            },
        };
        let mut auth = Fixture::new();
        auth.trust.selection_enrollment = enrollment.id().unwrap();
        auth.trust.placement_enrollment = config.id().unwrap();
        let verified = auth.verify(&auth.claims, 1001).unwrap();
        let (commit, object, mut frames) = responses();
        let paged = matches!(mode, "paged" | "pagebudget");
        let other = Object::from_bytes(b"another synthetic payload");
        if paged {
            use sha1::{Digest as _, Sha1};
            let pointer = Pointer::new(other.clone()).to_bytes();
            let mut hasher = Sha1::new();
            hasher.update(format!("blob {}\0", pointer.len()).as_bytes());
            hasher.update(&pointer);
            let oid = hasher
                .finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            frames[2]["tree"].as_array_mut().unwrap().push(json!({
                "path":"two.bin","mode":"100644","type":"blob","sha":oid,"size":pointer.len()
            }));
            frames.push(json!({"sha":oid,"encoding":"base64","size":pointer.len(),"content":STANDARD.encode(pointer)}));
        }
        assert_eq!(&commit, verified.commit());
        let current = vec![
            json!({"id":1234,"owner":{"id":5678},"full_name":auth.trust.repository}),
            json!({"ref":auth.trust.ref_name,"object":{"type":"commit","sha":commit.as_str()}}),
        ];
        let mut replies = current.clone();
        replies.extend(frames.clone());
        replies.extend(current.clone());
        replies.extend(current.clone());
        replies.extend(frames.clone());
        replies.extend(current.clone());
        replies.extend(current.clone());
        replies.extend(current.clone());
        if mode == "paged" {
            replies.extend(current.clone());
        }
        replies.extend(current);
        match mode {
            "expired" | "clock" | "outage" => replies.truncate(18),
            "advanced" => replies[21]["object"]["sha"] = json!("e".repeat(40)),
            "withdrawn" => replies.truncate(20),
            "pagebudget" => replies.truncate(22),
            _ => {}
        }
        let expected_requests = replies.len();
        let retry_replies = replies.clone();
        let (client, server) = peer(replies);
        let namespace_minimum = ns.checkpoint();
        let metadata_minimum = metadata.checkpoint();
        let namespace_root = temp.path().join("namespace");
        let metadata_root = temp.path().join("metadata");
        let admission_limits = config.budget.limits.clone();
        let mut namespaces = vec![ns];
        let mut locals = stores
            .iter()
            .zip(ledgers.iter_mut())
            .zip(jobs.iter())
            .map(|((store, ledger), job)| placement::Local { store, ledger, job })
            .collect::<Vec<_>>();
        let mut seed = Seed(b"synthetic payload".to_vec());
        let run_budget = RunBudget {
            pages: if mode == "pagebudget" { 1 } else { 2 },
            objects: 2,
            stream_bytes: 1024,
            millis: 30000,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut caller = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (incoming, _) = listener.accept().unwrap();
        let token = auth.token(&auth.claims);
        let body =
            json!({"schema":"tig.github-request.design-1","commit":commit.as_str()}).to_string();
        let response = thread::scope(|scope| {
            let handler = scope.spawn(|| {
                crate::github::intake::handle(
                    incoming,
                    &auth.trust,
                    &auth.keys,
                    || Ok(1001),
                    |proof, cursor| {
                        let report = Coordinator {
                            trust: &auth.trust,
                            request: &proof,
                            client: &client,
                            enrollment: &enrollment,
                            config: &config,
                            namespaces: &mut namespaces,
                            metadata: &mut metadata,
                            places: &mut locals,
                            seed: &mut seed,
                            seed_identity: &config.seed,
                            permissions: Permissions {
                                selection: true,
                                copy: true,
                                eviction: true,
                                reacquisition: true,
                                cancellation: true,
                            },
                            budget: &run_budget,
                        }
                        .run(cursor.as_ref(), {
                            let mut calls = 0;
                            move || {
                                calls += 1;
                                if mode == "withdrawn" && calls == 2 {
                                    let mut peer =
                                        Namespace::open(&namespace_root, &namespace_minimum)
                                            .unwrap();
                                    let meta =
                                        MetadataStore::open(&metadata_root, &metadata_minimum)
                                            .unwrap();
                                    let current = peer.source(&key("main")).unwrap().unwrap();
                                    peer.apply(
                                        key("fixture-withdrawal"),
                                        Command::Withdraw {
                                            source: key("main"),
                                            owner: key("owner"),
                                            expected_revision: current.revision,
                                        },
                                        &meta.finished_reader().unwrap(),
                                        &admission_limits,
                                    )
                                    .unwrap();
                                }
                                Ok(match (mode, calls) {
                                    ("expired", 2..) => 1300,
                                    ("clock", 2..) => 1000,
                                    _ => 1001,
                                })
                            }
                        })?;
                        serde_json::to_value(report).map_err(|_| Error::Invalid)
                    },
                )
            });
            write!(caller,"POST /v1/reconcile HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
            let mut response = String::new();
            caller.read_to_string(&mut response).unwrap();
            handler.join().unwrap().unwrap();
            response
        });
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(!response.contains(&token));
        let response: Value =
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(response["authenticated"], true);
        let report = &response["result"];
        assert_eq!(report["selection"]["desired_current"], mode != "withdrawn");
        let copied = matches!(
            mode,
            "complete" | "advanced" | "paged" | "pagebudget" | "retry"
        );
        let converged = matches!(mode, "complete" | "paged" | "retry");
        assert_eq!(report["storage_converged"], converged);
        let pages = if mode == "paged" {
            2
        } else {
            u32::from(copied)
        };
        assert_eq!(report["pages"], pages);
        assert_eq!(report["objects"], pages);
        assert_eq!(
            report["stream_bytes"],
            if mode == "paged" {
                (object.bytes + other.bytes) * 2
            } else if mode == "pagebudget" {
                // Plans sort objects by immutable identity rather than path.
                let first = if object < other { &object } else { &other };
                first.bytes * 2
            } else if copied {
                object.bytes * 2
            } else {
                0
            }
        );
        if converged {
            assert_eq!(report["holds"], json!([]));
        } else {
            let reason = match mode {
                "expired" => "expired",
                "clock" => "clock",
                "advanced" | "withdrawn" => "stale",
                "outage" => "unavailable",
                "pagebudget" => "budget",
                _ => unreachable!(),
            };
            assert_eq!(report["holds"], json!([{"admission":reason}]));
        }
        let first_copied = copied && (mode != "pagebudget" || object < other);
        assert_eq!(stores[0].read_object(&object).is_ok(), first_copied);
        assert_eq!(stores[2].read_object(&object).is_ok(), first_copied);
        if paged {
            let other_copied = mode == "paged" || other < object;
            assert_eq!(stores[0].read_object(&other).is_ok(), other_copied);
            assert_eq!(stores[2].read_object(&other).is_ok(), other_copied);
            assert_eq!(report["next"].is_null(), mode == "paged");
        }
        assert!(stores[1].read_object(&object).is_err());
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), expected_requests);
        assert!(requests.iter().all(|s| !s.contains(&token)));
        if mode == "retry" {
            // The first request may have completed while its response was lost.
            // A duplicate must still observe stores without issuing new writes.
            let revision = report["selection"]["source_revision"].clone();
            let mut replies = retry_replies;
            replies.truncate(20); // no-op adoption skips the post-write ref sample
            let (client, server) = peer(replies);
            let proof = auth.verify(&auth.claims, 1001).unwrap();
            let repeated = Coordinator {
                trust: &auth.trust,
                request: &proof,
                client: &client,
                enrollment: &enrollment,
                config: &config,
                namespaces: &mut namespaces,
                metadata: &mut metadata,
                places: &mut locals,
                seed: &mut seed,
                seed_identity: &config.seed,
                permissions: Permissions {
                    selection: true,
                    copy: true,
                    eviction: true,
                    reacquisition: true,
                    cancellation: true,
                },
                budget: &run_budget,
            }
            .run(None, || Ok(1001))
            .unwrap();
            assert_eq!(repeated.storage_converged, Some(true));
            assert_eq!(repeated.stream_bytes, 0);
            assert_eq!(json!(repeated.selection.source_revision), revision);
            assert_eq!(server.join().unwrap().len(), 20);
        }
    }
}
