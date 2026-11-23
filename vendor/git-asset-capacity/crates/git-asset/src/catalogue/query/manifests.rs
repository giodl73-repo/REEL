//! Explicit named JSON reads, never scans or automatic semantic graph edges.
use super::*;
use crate::catalogue::storage::nested;

pub(super) struct Matches {
    pub refs: Vec<nested::Association>,
    pub summary: Value,
    pub content: git_asset_core::Digest,
    pub complete: bool,
}

pub(super) fn interpretation(p: &Project) -> Value {
    let storage=p.enrollment.storage.as_ref().map(|s|json!({"nested":s.nested_content,
        "places":s.places.iter().map(|p|json!({"index":p.place_index,"read_allowed":p.live_probe})).collect::<Vec<_>>()}));
    let source=p.profile.byte_source.as_ref().map(|s|json!({"places":s.places.iter().map(|p|json!({"root":p.root,"layout":p.layout})).collect::<Vec<_>>(),
        "required":s.required,"limits":{"object_bytes":s.limits.object_bytes,"total_bytes":s.limits.total_bytes,"requests":s.limits.requests,"verify_millis":s.limits.verify_millis,"live_readers":s.limits.live_readers}}));
    json!({"storage":storage,"source":source})
}

impl Catalogue {
    pub(super) fn manifest_references(
        &self,
        project: &str,
        collection: &str,
        work: &str,
        sha: &str,
        expected: Option<&git_asset_core::Digest>,
        offset: usize,
    ) -> Result<Value> {
        let matched = self.manifest_matches(project, collection, work, sha, expected)?;
        let total = matched.refs.len();
        let items=matched.refs.iter().skip(offset).take(100).map(|r|json!({"project":project,"collection":collection,"work":work,"binding":"manifest","reference":r})).collect::<Vec<_>>();
        let summary = matched.summary;
        Ok(
            json!({"project":project,"commit":self.projects[project].commit,"collection":collection,"work":work,"sha256":sha,
            "items":items,"total":total,"content_sha256":matched.content,"extraction":summary,"complete_extraction":matched.complete,
            "metadata_read_attempts":summary["read_attempts"],"metadata_documents_decoded":summary["documents_read"],
            "semantics":"Direct byte associations in this explicitly requested admitted manifest scope. Containing documents and root bindings are provenance, not inferred production edges. Unrequested scopes, raw LFS pointers and semantic consumers remain unknown.","authorizes_eviction":false}),
        )
    }

    pub(super) fn manifest_matches(
        &self,
        project: &str,
        collection: &str,
        work: &str,
        sha: &str,
        expected: Option<&git_asset_core::Digest>,
    ) -> Result<Matches> {
        let digest = git_asset_core::Digest::parse(sha).map_err(|_| "invalid_request")?;
        let p = self.projects.get(project).ok_or("unknown_project")?;
        if !p
            .works
            .iter()
            .any(|w| w.collection == collection && w.id == work)
        {
            return Err("unknown_work".into());
        }
        let available = p
            .enrollment
            .storage
            .as_ref()
            .is_some_and(|s| s.nested_content.is_some())
            && p.profile.byte_source.is_some();
        let (refs, summary) = if available {
            let request = self
                .nested_request(project, &p.commit, collection, work)
                .map_err(|_| "manifest_scope_refused")?
                .agent_bounded()
                .map_err(|_| "manifest_budget_exceeded")?;
            let loaded = nested::execute(&request).map_err(|e| match e.as_str() {
                "Nested metadata budget exceeded; previous view preserved" => {
                    "manifest_budget_exceeded"
                }
                "Conflicting nested object sizes; previous view preserved" => {
                    "conflicting_identity"
                }
                _ => "manifest_read_refused",
            })?;
            let rows: Vec<_> = loaded
                .associations()
                .iter()
                .filter(|r| r.object.sha256 == digest)
                .cloned()
                .collect();
            (rows, loaded.read_summary().clone())
        } else {
            (
                Vec::new(),
                json!({"roots":0,"successful_roots":0,"failures":[{"reason":"manifest adapters or byte sources are not enrolled"}],"read_attempts":0,"documents_read":0,"metadata_bytes":0}),
            )
        };
        let content=Object::from_bytes(&serde_json::to_vec(&json!({"project":project,"commit":p.commit,"collection":collection,"work":work,"extraction":summary})).map_err(|_|"query_refused")?).sha256;
        if expected.is_some_and(|e| e != &content) {
            return Err("stale_content".into());
        }
        let complete = available
            && summary["failures"].as_array().is_some_and(|v| v.is_empty())
            && summary["successful_roots"] == summary["roots"];
        Ok(Matches {
            refs,
            summary,
            content,
            complete,
        })
    }
}
