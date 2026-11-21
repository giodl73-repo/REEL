//! Fixed metadata-only UI bridge; no general agent-query or byte-read endpoint.
use super::*;

impl Catalogue {
    pub(crate) fn impact_view(&mut self, q: &BTreeMap<String, String>) -> Result<Value> {
        let get = |key: &str| q.get(key).map(String::as_str).unwrap_or("");
        let project = get("project");
        let id = get("id");
        let offset: usize = if get("offset").is_empty() {
            0
        } else {
            get("offset").parse().map_err(|_| "invalid_request")?
        };
        let names: Vec<_> = get("kinds").split(',').collect();
        let kinds: BTreeSet<graph::EdgeKind> =
            serde_json::from_value(json!(names)).map_err(|_| "invalid_request")?;
        if q.keys().any(|k| {
            !["project", "id", "snapshot", "kinds", "offset", "basis"].contains(&k.as_str())
        }) || project == "all"
            || id.is_empty()
            || id.len() > 256
            || id.chars().any(char::is_control)
            || kinds.is_empty()
            || kinds.len() != names.len()
            || offset > 1_000_000
            || (offset > 0 && get("basis").is_empty())
        {
            return Err("invalid_request".into());
        }
        self.projects
            .get(project)
            .ok_or("unknown_project")?
            .check_snapshot(get("snapshot"))?;
        let (_, basis) = self.query_basis(&BTreeSet::from([project]))?;
        if !get("basis").is_empty()
            && git_asset_core::Digest::parse(get("basis")).map_err(|_| "invalid_request")? != basis
        {
            return Err("stale_basis".into());
        }
        let mut data = self.impact_query(project, id, &kinds, offset)?;
        let complete = data["complete_declared_coverage"] == true;
        let total = data["total_known"].as_u64().ok_or("graph_refused")?;
        let items = data["items"].as_array_mut().ok_or("graph_refused")?;
        items.truncate(25);
        let p = self.projects.get(project).ok_or("unknown_project")?;
        for row in items.iter_mut() {
            let reference: graph::Reference =
                serde_json::from_value(row["asset"].clone()).map_err(|_| "graph_refused")?;
            let selected = p
                .snapshot
                .resolve_id(&reference.id)
                .is_ok_and(|a| a.reference() == &reference);
            row["navigation"] = if selected {
                json!(p.cards.iter().find(|c| c.id == reference.id))
            } else {
                Value::Null
            };
        }
        let returned = items.len();
        let next = offset.checked_add(returned).ok_or("invalid_request")?;
        Ok(
            json!({"schema":"tig.catalogue-impact.experimental-1","project":project,"commit":p.commit,"basis_sha256":basis,
            "status":if complete {"complete"} else {"partial"},
            "coverage":{"complete_declared_scope":complete,"complete_project_scope":false,"payloads_opened":0},
            "paging":{"offset":offset,"returned":returned,"total_known":total,"next_offset":if (next as u64)<total {Some(next)} else {None}},
            "data":data,"authorizes_eviction":false}),
        )
    }
}
