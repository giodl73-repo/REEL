use super::profile::{Profile, Selection};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use tig_core::{Claim, Digest, Placement, Plan, View, identity::canonical_json};
use tig_git::source::{Extraction, GitId, Limits, Repository, Scope};

fn placement(p: &Placement) -> Value {
    json!({"object":p.object,"place":p.place})
}
fn resolve(repository: &Repository, selection: &Selection) -> Result<Extraction, &'static str> {
    let id = GitId::parse(&selection.commit).map_err(|_| "invalid commit")?;
    let snapshot = repository
        .commit(id)
        .map_err(|_| "cannot read exact commit")?;
    repository
        .extract(
            &snapshot,
            Scope::new(selection.scope.clone()).map_err(|_| "invalid scope")?,
        )
        .map_err(|_| "exact selection failed or exceeded budget")
}
fn source_report(selection: &Selection, extracted: &Extraction) -> Value {
    json!({"id":selection.id,"commit":selection.commit,
        "scope":extracted.scope().prefixes().collect::<Vec<_>>(),"places":selection.places,
        "selection_digest":extracted.selection_digest().ok(),"complete":extracted.complete(),
        "bindings":extracted.bindings().iter().map(|b|json!({"path":b.path,"object":b.object})).collect::<Vec<_>>(),
        "issues":extracted.issues().iter().map(|i|json!({"path":i.path,"stage":i.stage,"kind":format!("{:?}",i.kind)})).collect::<Vec<_>>() })
}
fn input_digest(sources: &[Value], supplied: &[Value]) -> Result<Digest, &'static str> {
    let descriptors: Vec<_> = sources
        .iter()
        .map(|s| {
            json!({"id":s["id"],
        "commit":s["commit"],"scope":s["scope"],"places":s["places"],
        "selection_digest":s["selection_digest"],"complete":s["complete"]})
        })
        .collect();
    Ok(Digest::blob(
        &canonical_json(&json!([
            "tig.placement-inspection-input.experimental-1",
            descriptors,
            supplied
        ]))
        .map_err(|_| "inspection identity budget exceeded")?,
    ))
}
pub fn inspect(mut profile: Profile) -> Result<(Vec<u8>, bool), &'static str> {
    let repository = Repository::new(
        profile.git_executable,
        profile.repository,
        Limits {
            listing_bytes: 2 * 1024 * 1024,
            entries: 8192,
            pointer_blobs: 512,
            command_millis: 1000,
        },
    )
    .map_err(|_| "cannot inspect explicit repository")?;
    profile.selections.sort_by(|a, b| a.id.cmp(&b.id));
    let mut sources = Vec::new();
    let mut claims = Vec::new();
    let mut bindings = 0usize;
    let mut complete = true;
    for mut selection in profile.selections {
        selection.places.sort();
        let extracted = resolve(&repository, &selection)?;
        bindings = bindings
            .checked_add(extracted.bindings().len())
            .filter(|n| *n <= 2048)
            .ok_or("aggregate binding budget exceeded")?;
        complete &= extracted.complete();
        let placements = extracted
            .bindings()
            .iter()
            .flat_map(|b| {
                selection.places.iter().map(move |place| Placement {
                    object: b.object.clone(),
                    place: place.clone(),
                })
            })
            .collect();
        claims.push(Claim {
            id: selection.id.clone(),
            placements,
        });
        sources.push(source_report(&selection, &extracted));
    }
    let supplied: BTreeSet<_> = profile
        .supplied_copies
        .into_iter()
        .map(|p| Placement {
            object: p.object,
            place: p.place,
        })
        .collect();
    // Supplied copies have no provider proof, clock or current global coverage.
    let plan = View::new(0, false, claims)
        .and_then(|v| v.plan(&supplied))
        .map_err(|_| "conflicting identities or placement budget exceeded")?;
    render(sources, supplied, plan, complete)
}
fn render(
    sources: Vec<Value>,
    supplied: BTreeSet<Placement>,
    plan: Plan,
    complete: bool,
) -> Result<(Vec<u8>, bool), &'static str> {
    let supplied: Vec<_> = supplied.iter().map(placement).collect();
    if plan
        .required_bytes_by_place
        .values()
        .any(|n| *n > 9_007_199_254_740_991)
    {
        return Err("logical byte count exceeds interoperable integer budget");
    }
    let input_digest = input_digest(&sources, &supplied)?;
    let output = json!({"schema":"tig.placement-inspection.experimental-1",
        "input_digest":input_digest,"source_metadata_complete":complete,"sources":sources,
        "coverage":{"protection":"unknown","custody":"unknown","copies":"caller-supplied-unverified"},
        "supplied_copies":supplied,
        "required_without_supplied_copy":plan.ensure.iter().map(placement).collect::<Vec<_>>(),
        "desired_with_supplied_copy":plan.retained.iter().map(placement).collect::<Vec<_>>(),
        "held_surplus":plan.held.keys().map(|p|json!({"copy":placement(p),"reason":"current-protection-and-custody-unverified"})).collect::<Vec<_>>(),
        "required_logical_bytes_by_place":plan.required_bytes_by_place,
        "physical_capacity":"unknown","execution_ready":false,"removal_authorized":false});
    let output = serde_json::to_vec(&output).map_err(|_| "inspection output budget exceeded")?;
    if output.len() > 4 * 1024 * 1024 {
        return Err("inspection output budget exceeded");
    }
    Ok((output, complete))
}
