//! Fixed native metadata comparison for the catalogue; no arbitrary query proxy.
use super::*;

impl Catalogue {
    pub(crate) fn compare_view(&self, q: &BTreeMap<String, String>) -> Result<Value> {
        let get = |key: &str| q.get(key).map(String::as_str).unwrap_or("");
        if q.keys().any(|k| {
            ![
                "left_project",
                "left_id",
                "left_snapshot",
                "right_project",
                "right_id",
                "right_snapshot",
                "basis",
            ]
            .contains(&k.as_str())
        }) {
            return Err("invalid_request".into());
        }
        let left = Asset {
            project: get("left_project").into(),
            id: get("left_id").into(),
        };
        let right = Asset {
            project: get("right_project").into(),
            id: get("right_id").into(),
        };
        for (asset, snapshot) in [
            (&left, get("left_snapshot")),
            (&right, get("right_snapshot")),
        ] {
            if asset.project == "all"
                || asset.id.is_empty()
                || asset.id.len() > 256
                || asset.id.chars().any(char::is_control)
            {
                return Err("invalid_request".into());
            }
            self.projects
                .get(&asset.project)
                .ok_or("unknown_project")?
                .check_snapshot(snapshot)?;
        }
        let ids = BTreeSet::from([left.project.as_str(), right.project.as_str()]);
        let (_, basis) = self.query_basis(&ids)?;
        if !get("basis").is_empty()
            && git_asset_core::Digest::parse(get("basis")).map_err(|_| "invalid_request")? != basis
        {
            return Err("stale_basis".into());
        }
        let data = self.compare_assets(&left, &right)?;
        let snapshots: BTreeMap<_, _> = self
            .projects
            .iter()
            .filter(|(id, _)| ids.contains(id.as_str()))
            .map(|(id, p)| (id, &p.commit))
            .collect();
        Ok(
            json!({"schema":"tig.catalogue-compare.experimental-1","status":"complete","snapshots":snapshots,"basis_sha256":basis,
            "coverage":{"complete_declared_scope":true,"complete_project_scope":false,"payloads_opened":0},
            "data":data,"authorizes_eviction":false}),
        )
    }
}
