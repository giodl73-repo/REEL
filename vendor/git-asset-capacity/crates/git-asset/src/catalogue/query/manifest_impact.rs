//! Evidence associations followed by admitted semantic edges, never inferred use.
use super::*;

impl Catalogue {
    pub(super) fn manifest_impact_query(&mut self, request: &Request) -> Result<Value> {
        let Operation::ManifestImpact {
            project,
            collection,
            work,
            sha256,
            kinds,
            expected_content,
        } = &request.query
        else {
            return Err("invalid_request".into());
        };
        let matched =
            self.manifest_matches(project, collection, work, sha256, expected_content.as_ref())?;
        let p = self.projects.get_mut(project).ok_or("unknown_project")?;
        let mut targets = BTreeSet::new();
        for id in matched
            .refs
            .iter()
            .map(|r| r.asset.as_str())
            .collect::<BTreeSet<_>>()
        {
            targets.insert(
                p.snapshot
                    .resolve_id(id)
                    .map_err(|_| "unknown_asset")?
                    .reference()
                    .clone(),
            );
        }
        let profiles = p.enrollment.graph_profiles.clone();
        let (mut report, _) =
            impact::resolved_targets(p, targets, kinds, &profiles, request.offset)?;
        let native_complete = report["complete_declared_coverage"] == true;
        report["complete_native_coverage"] = json!(native_complete);
        report["complete_manifest_extraction"] = json!(matched.complete);
        report["complete_declared_coverage"] = json!(matched.complete && native_complete);
        report["content_sha256"] = json!(matched.content);
        report["manifest"] = json!({"project":project,"collection":collection,"work":work,"sha256":sha256,
            "matching_associations":matched.refs.len(),"extraction":matched.summary,
            "provenance_query":"manifest_references with the same scope, SHA and content token"});
        report["metadata_read_attempts"] = matched.summary["read_attempts"].clone();
        report["metadata_documents_decoded"] = matched.summary["documents_read"].clone();
        report["conditional_on"] = json!("A manifest-bearing selected record changes");
        report["semantics"] = json!(
            "Targets are selected records with matching declared byte associations, including retained bindings. Items are potential native consumers of those records through admitted edge kinds. Association membership is evidence, not a production edge or proof of current byte use, stale renders or required regeneration. Targets are listed separately and excluded from consumer counts; one deterministic shortest path is shown per consumer. Unrequested scopes and unadmitted consumers remain unknown."
        );
        report["authorizes_eviction"] = json!(false);
        Ok(report)
    }
}
