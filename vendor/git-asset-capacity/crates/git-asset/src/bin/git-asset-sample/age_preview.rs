//! Retained age facts for the same sample occurrences; no payload authority.
use super::*;
use git_asset_store::source_claims::age;
struct Clock {
    policy: Digest,
    time: u64,
}
impl final_provider::Clock for Clock {
    fn now(&self, policy: &Digest) -> final_provider::Result<u64> {
        if policy != &self.policy {
            return Err(final_provider::Error::Clock);
        }
        Ok(self.time)
    }
}
pub(super) fn run(
    root: &Path,
    observer: &mut Namespace,
    metadata: &mut MetadataStore,
) -> Result<Object> {
    let observed = observer.checkpoint();
    let main = observer.source(&key("main"))?.ok_or("main missing")?;
    if main.predecessors.len() < 3 {
        return Err("age lesson requires three exact predecessors".into());
    }
    let requests = main
        .predecessors
        .iter()
        .take(3)
        .map(|v| age::Request {
            namespace: observer.identity().clone(),
            source: key("main"),
            predecessor_revision: v.revision,
        })
        .collect::<Vec<_>>();
    let mut registry = Namespace::create(
        &root.join("age-registry"),
        key("sample-age"),
        key("sample-age-epoch"),
        policy(),
    )?;
    registry.apply(
        key("enroll-age-registry"),
        Command::Enroll {
            source: key("observations"),
            owner: key("sample"),
            class: Kind::Retention,
        },
        metadata,
        &limits(),
    )?;
    let enrollment = age::Enrollment {
        authority: Digest::blob(b"public sample admitted age observer"),
        epoch: Digest::blob(b"public sample age epoch"),
        metadata: metadata.checkpoint().identity().clone(),
        registry: registry.identity().clone(),
        registry_policy: hash(registry.policy())?,
        source: key("observations"),
        owner: key("sample"),
        clock: Digest::blob(b"public sample simulated age clock"),
        observers: vec![age::Observer {
            namespace: observer.identity().clone(),
            policy: hash(observer.policy())?,
            owners: vec![key("sample")],
            classes: [Kind::Branch, Kind::Draft].into_iter().collect(),
        }],
    };
    let bounds = age::Limits::default();
    let minimum = registry.checkpoint();
    age::Capture {
        enrollment: &enrollment,
        registry: &mut registry,
        minimum: &minimum,
        metadata,
        limits: &bounds,
    }
    .initialize()?;
    let mut captures = Vec::new();
    for (request, time) in requests.iter().zip([100, 950]) {
        let minimum = registry.checkpoint();
        captures.push(
            age::Capture {
                enrollment: &enrollment,
                registry: &mut registry,
                minimum: &minimum,
                metadata,
                limits: &bounds,
            }
            .capture(
                request,
                observer,
                &observed,
                &Clock {
                    policy: enrollment.clock.clone(),
                    time,
                },
            )?,
        );
    }
    metadata.finish()?;
    let registry_minimum = registry.checkpoint();
    drop(registry);
    let mut registry = Namespace::open(&root.join("age-registry"), &registry_minimum)?;
    let mut reader = git_asset_store::metadata::MetadataReader::open(
        &root.join("metadata"),
        &metadata.checkpoint(),
    )?;
    let mut inspection = age::Inspection {
        enrollment: &enrollment,
        registry: &mut registry,
        minimum: &registry_minimum,
        metadata: enrollment.metadata.clone(),
        nodes: &mut reader,
        limits: &bounds,
    };
    let mut assessments = Vec::new();
    for request in &requests {
        assessments.push(inspection.assess(
            request,
            observer,
            &observed,
            &Clock {
                policy: enrollment.clock.clone(),
                time: 1000,
            },
            500,
        )?);
    }
    if !assessments[0].old_enough
        || assessments[0].age_lower_bound_millis != Some(900)
        || assessments[1].old_enough
        || assessments[1].age_lower_bound_millis != Some(50)
        || assessments[2].old_enough
        || assessments[2].status.evidence.is_some()
        || observer.checkpoint() != observed
        || assessments
            .iter()
            .any(|a| a.authorizes_removal || a.authorizes_claim_release)
    {
        return Err("retained age lesson did not preserve old/recent/unknown distinctions".into());
    }
    save(
        &root.join("retained-age.json"),
        &json!({"schema":"tig.sample-retained-age.design-1","enrollment":enrollment,
            "captures":captures,"assessments":assessments,"occurrences":main.predecessors.iter().take(3).collect::<Vec<_>>(),"observer_before":observed,"observer_after":observer.checkpoint(),
        "simulated_clock":true,"samples":[100,950,1000],"required_age_millis":500,"registry_reopened":true,
        "authorizes_removal":false,"scope_limit":"retained age assessment; native category and age-aware final execution pending"}),
    )
}
