//! Owner-visible diagnostics from exact used definitions and admitted profiles.
use super::*;
use git_asset_model::native::Definition;

impl Catalogue {
    pub(super) fn admission_query(
        &mut self,
        project: &str,
        kinds: &BTreeSet<graph::EdgeKind>,
        filter: &str,
        offset: usize,
    ) -> Result<Value> {
        let mut items = Vec::new();
        let mut total = 0usize;
        let mut covered = 0usize;
        let mut invalid = 0usize;
        let mut metadata_gaps = 0usize;
        let mut admissions = BTreeMap::new();
        for (id, p) in self
            .projects
            .iter_mut()
            .filter(|(id, _)| project == "all" || id.as_str() == project)
        {
            let admission = graph::profile_admission(&p.enrollment.graph_profiles).ok();
            if admission.is_none() {
                invalid += 1;
            }
            admissions.insert(id.clone(),json!({"valid":admission.is_some(),"sha256":admission,"profiles":p.enrollment.graph_profiles.len()}));
            let mut definitions =
                BTreeMap::<graph::Reference, (usize, Vec<graph::Reference>)>::new();
            for asset in p.snapshot.selected_records() {
                if !filter.is_empty() && asset.definition().id != filter {
                    continue;
                }
                let entry = definitions.entry(asset.definition().clone()).or_default();
                entry.0 += 1;
                if entry.1.len() < 3 {
                    entry.1.push(asset.reference().clone());
                }
            }
            for (definition, (records, examples)) in definitions {
                let profile = p.enrollment.graph_profiles.iter().find(|g| {
                    g.definition_id == definition.id && g.definition == definition.object
                });
                let uncovered: BTreeSet<_> = kinds
                    .iter()
                    .filter(|k| profile.is_none_or(|p| !p.covered.contains(k)))
                    .copied()
                    .collect();
                let admitted = profile.is_some() && admission.is_some();
                if admitted && uncovered.is_empty() {
                    covered += 1;
                }
                if total >= offset && items.len() < 100 {
                    let lookup = graph::Source::lookup(&mut p.snapshot, &examples[0], 3 * 65536);
                    let contract=match lookup.data {
                        graph::Data::Record{definition:raw,..} if lookup.checkpoint==*p.snapshot.checkpoint()&&Object::from_bytes(&raw)==definition.object=>{
                            Definition::parse(&raw).ok().map(|parsed|json!({"authored_schema":parsed.value().authored_schema,
                                "properties":parsed.value().authoring.properties,"required":parsed.value().authoring.required}))
                        },_=>None
                    };
                    if contract.is_none() {
                        metadata_gaps += 1;
                    }
                    items.push(json!({"project":id,"definition":definition,"registered_records":records,"examples":examples,
                        "admitted":admitted,"covered_kinds":profile.map(|p|&p.covered),"uncovered_kinds":uncovered,
                        "class":profile.map(|p|p.class),"rules":profile.map(|p|&p.rules),"payloads":profile.map(|p|&p.payloads),
                        "contract":contract,"next_action":if admission.is_none(){"fix_invalid_profile_set"}else if !admitted{"admit_exact_definition_profile"}else if !uncovered.is_empty(){"review_requested_kind_coverage"}else{"evaluate_instance_edges_with_impact"}}));
                }
                total += 1;
            }
        }
        Ok(
            json!({"requested_kinds":kinds,"profile_sets":admissions,"total":total,"items":items,"definitions_with_requested_coverage":covered,
            "invalid_profile_sets":invalid,"contract_metadata_gaps_on_page":metadata_gaps,
            "complete_requested_admission":covered==total&&invalid==0&&metadata_gaps==0,"instance_edges_evaluated":false,
            "semantics":"Declared admission and contract discovery only. Property names do not infer relationships; profiles remain owner-controlled. Defaults, schema examples and raw instance values are omitted."}),
        )
    }
}
