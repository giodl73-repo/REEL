//! Explicit SDK client flow; no raw annex setkey for any handoff control.
use super::*;
use reel_workflow_git_asset::ingress::Control;
pub(super) struct Frozen {
    control: Control,
    raw: Vec<u8>,
    expected: Object,
    request: publication::Request,
    admission: publication::Admission,
}
pub(super) fn freeze(
    root: &Path,
    binding: &Binding,
    profile: &publication::Profile,
    admission: &publication::Admission,
    nodes: &mut impl NodeStore,
    entries: Vec<(&str, Artifact, &[u8])>,
) -> Vec<Frozen> {
    assert!(!entries.is_empty() && entries.len() <= 32);
    let provenance = nodes
        .put(b"original synthetic client ingress provenance")
        .unwrap();
    entries
        .into_iter()
        .map(|(role, artifact, bytes)| {
            let control = Control::new(artifact, bytes).unwrap();
            let mut request = publication::Request {
                operation: key(&format!("ingress-{role}")),
                source: key(&format!("ingress-{role}")),
                owner: key("owner"),
                expected_revision: 1,
                candidate: publication::Candidate {
                    logical_id: control.artifact().logical_id.clone(),
                    object: binding::object(control.artifact()).unwrap(),
                    provenance: provenance.clone(),
                },
            };
            // A control blob cannot consume a reserved composite seed or the input job.
            for reserved in ["controls", "returns", "retained", "attempt", "creative"] {
                let source = std::mem::replace(&mut request.source, key(reserved));
                assert!(
                    matches!(control.plan(binding,profile,request.clone(),admission.clone(),nodes),
                Err(e) if e.kind==ErrorKind::Denied)
                );
                request.source = source;
            }
            let mut missing = request.clone(); missing.source=key("missing-ingress");
            assert!(matches!(control.plan(binding,profile,missing,admission.clone(),nodes),Err(e) if e.kind==ErrorKind::Missing));
            let mut foreign = request.clone(); foreign.owner=key("foreign-owner");
            assert!(matches!(control.plan(binding,profile,foreign,admission.clone(),nodes),Err(e) if e.kind==ErrorKind::Denied));
            let mut stale = request.clone(); stale.expected_revision += 1;
            assert!(matches!(control.plan(binding,profile,stale,admission.clone(),nodes),Err(e) if e.kind==ErrorKind::Stale));
            let mut other = admission.clone(); other.namespace=git_asset_core::Digest::blob(b"foreign namespace");
            assert!(matches!(control.plan(binding,profile,request.clone(),other,nodes),Err(e) if e.kind==ErrorKind::Denied));
            let plan = control
                .plan(binding, profile, request.clone(), admission.clone(), nodes)
                .unwrap();
            let raw = control.encode_plan(&plan).unwrap();
            let expected = Object::from_bytes(&raw);
            // Persist the ORIGINAL SDK plan before native visibility. This directory
            // is owned synthetic caller control; the pin is not third-party authority.
            use std::io::Write;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(root.join(format!("original-ingress-{role}.json")))
                .unwrap();
            file.write_all(&raw).unwrap();
            file.sync_all().unwrap();
            Frozen {
                control,
                raw,
                expected,
                request,
                admission:admission.clone(),
            }
        })
        .collect()
}
pub(super) fn publish(
    binding: &Binding,
    profile: &publication::Profile,
    frozen: &[Frozen],
    publisher: &mut Publisher,
    resources: &mut Resources<'_>,
) -> Vec<publication::Ready> {
    frozen
        .iter()
        .map(|f| {
            let plan = Control::restore_plan(f.control.artifact(), &f.raw, &f.expected).unwrap();
            let mut source = f.control.clone();
            let ready = publisher.apply(&plan, resources, &mut source).unwrap();
            // A published source cannot be prepared again as a fresh ingress.
            // Read-only nodes also prove this refusal precedes metadata freeze.
            assert!(matches!(f.control.plan(binding,profile,f.request.clone(),f.admission.clone(),resources.metadata),
                Err(e) if e.kind==ErrorKind::Stale));
            let resumed = publisher.resume(&plan, resources).unwrap();
            assert_eq!(ready.receipt(), resumed.receipt());
            assert_eq!(
                ready.receipt().candidate.object,
                binding::object(f.control.artifact()).unwrap()
            );
            drop(source);
            drop(resumed);
            ready
        })
        .collect()
}
