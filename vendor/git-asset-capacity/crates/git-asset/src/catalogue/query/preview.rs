//! Candidate interpretation simulation, never enrollment mutation or authority.
use super::*;

impl Catalogue {
    pub(super) fn preview_admission(
        &mut self,
        project: &str,
        asset: &str,
        kinds: &BTreeSet<graph::EdgeKind>,
        profiles: &[graph::Profile],
        offset: usize,
    ) -> Result<Value> {
        let proposed = graph::profile_admission(profiles).map_err(|_| "invalid_proposal")?;
        let p = self.projects.get_mut(project).ok_or("unknown_project")?;
        let current = p.enrollment.graph_profiles.clone();
        let before_admission = graph::profile_admission(&current).ok();
        let (before, before_consumers) = if before_admission.is_some() {
            super::impact::resolved_impact(p, asset, kinds, &current, 0)?
        } else {
            (
                json!({"complete_declared_coverage":false,"unavailable":"invalid_profile_set","items":[],"total_known":0}),
                BTreeMap::new(),
            )
        };
        let (after, after_consumers) =
            super::impact::resolved_impact(p, asset, kinds, profiles, 0)?;
        let added: Vec<_> = after_consumers
            .keys()
            .filter(|r| !before_consumers.contains_key(*r))
            .collect();
        let removed: Vec<_> = before_consumers
            .keys()
            .filter(|r| !after_consumers.contains_key(*r))
            .collect();
        let changed: Vec<_> = after_consumers
            .iter()
            .filter(|(r, h)| before_consumers.get(*r).is_some_and(|old| old != *h))
            .map(|(r, _)| r)
            .collect();
        let shared = after_consumers
            .keys()
            .filter(|r| before_consumers.contains_key(*r))
            .count();
        let total = added.len() + removed.len() + changed.len();
        let items = added
            .iter()
            .map(|r| json!({"change":"newly_known","asset":r}))
            .chain(
                removed
                    .iter()
                    .map(|r| json!({"change":"no_longer_known","asset":r})),
            )
            .chain(changed.iter().map(|r|json!({"change":"witness_changed","asset":r,"before_witness_sha256":before_consumers.get(*r),"after_witness_sha256":after_consumers.get(*r)})))
            .skip(offset)
            .take(100)
            .collect::<Vec<_>>();
        let complete = before["complete_declared_coverage"] == true
            && after["complete_declared_coverage"] == true;
        Ok(
            json!({"project":project,"asset":asset,"requested_kinds":kinds,
            "mode":"simulate_complete_profile_set_replacement","baseline_admission_sha256":before_admission,
            "proposed_admission_sha256":proposed,"proposed_profiles":profiles.len(),"baseline":before,"candidate":after,
            "total":total,"items":items,"newly_known":added.len(),"no_longer_known":removed.len(),
            "still_known":shared,"witness_changed":changed.len(),"complete_comparison":complete,
            "impact_details_first_page_only":true,"enrollment_changed":false,"admission_authorized":false,
            "semantics":"Known reachability under two interpretations of one captured source. Partial coverage is not absence; disappearing paths never authorize retirement. Candidate profiles are simulated, not installed. Candidate and baseline witness details show their first 100 consumers; delta paging covers all known consumer changes."}),
        )
    }
}
