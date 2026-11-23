use super::execution::*;
use super::*;
use crate::retention::routine_sources::Validation;
use crate::retirement_intent::{
    self as intent,
    execution_profile::Context,
    policy::{self, Proof},
};
use tig_core::retirement::review::{Review, decision::Decision};
#[test]
fn budget_child_exit() {
    let Ok(root) = std::env::var("TIG_POLICY_BUDGET_CASE_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let input: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("budget-child.json")).unwrap()).unwrap();
    let mut n = metadata::MetadataStore::open(
        &root.join("nodes"),
        &serde_json::from_value(input["metadata"].clone()).unwrap(),
    )
    .unwrap();
    let mut r = Registry::open(
        &root.join("accounted"),
        &serde_json::from_value(input["registry"].clone()).unwrap(),
    )
    .unwrap();
    let mut index = Index::open(
        &root.join("index"),
        &serde_json::from_value(input["index"].clone()).unwrap(),
    )
    .unwrap();
    let mut journal = SourceJournal::open(
        &root.join("source"),
        &serde_json::from_value(input["source"].clone()).unwrap(),
    )
    .unwrap();
    let context =
        Context::from_json(&identity::canonical_json(&input["context"]).unwrap()).unwrap();
    let review = Review::from_json(&identity::canonical_json(&input["review"]).unwrap()).unwrap();
    let decision = Decision::from_json(
        &identity::canonical_json(&input["decision"]).unwrap(),
        &review,
    )
    .unwrap();
    let proof = Proof::load(&n, &context, &review, &decision).unwrap();
    let held = r
        .hold(&key("main"), Some(&proof.grant().version), &n)
        .unwrap();
    let head = coordinated::accepted_head::hold_current(&mut index, &mut journal).unwrap();
    r.reserve_run_hook(
        &held,
        &head,
        &proof,
        &context,
        &review,
        &decision,
        &mut n,
        |stage| {
            let cut = match stage {
                UpdateStage::MetadataReady => 1,
                UpdateStage::Prepared => 2,
                UpdateStage::Adopted => 3,
            };
            if input["cut"] == cut {
                std::process::exit(92);
            }
            Ok(())
        },
    )
    .unwrap();
    panic!("expected actual process exit");
}
#[test]
fn actual_budget_process_exits_recover_without_double_spending() {
    for cut in 1..=3 {
        let (mut c, mut r) = enabled();
        let (observed, _, old_context) = stage(&mut c, &mut r);
        let old = Proof::load(
            &c.f.nodes,
            &old_context,
            observed.review(),
            observed.decision(),
        )
        .unwrap();
        let held = r.hold(&key("main"), None, &c.f.nodes).unwrap();
        let head = coordinated::accepted_head::hold_current(&mut c.f.index, &mut c.source).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_slice(&old_context.to_json().unwrap()).unwrap();
        value.as_object_mut().unwrap().remove("policy_proof");
        value["schema"] = serde_json::json!("tig.retirement-execution-context.experimental-1");
        value["logical_operation"] = serde_json::json!(d("second-decision"));
        let base = Context::from_json(&identity::canonical_json(&value).unwrap()).unwrap();
        let selected =
            Validation::new(held.policy(), old.run(), observed.review(), key("owner"), 0)
                .unwrap()
                .finish()
                .unwrap();
        let (context, decision) = policy::stage_proof(
            &mut c.f.nodes,
            &held,
            &head,
            &selected,
            old.run(),
            observed.review(),
            base,
        )
        .unwrap();
        drop(head);
        drop(held);
        let m = c.f.nodes.finish().unwrap();
        let j = r.checkpoint();
        let root = c.f.temp.path().to_owned();
        fs::write(root.join("budget-child.json"),serde_json::to_vec(&serde_json::json!({"cut":cut,"metadata":m,"registry":j,"index":c.f.index.checkpoint(),"source":c.source.checkpoint(),"context":serde_json::from_slice::<serde_json::Value>(&context.to_json().unwrap()).unwrap(),"review":serde_json::from_slice::<serde_json::Value>(&observed.review().to_json().unwrap()).unwrap(),"decision":serde_json::from_slice::<serde_json::Value>(&decision.to_json().unwrap()).unwrap()})).unwrap()).unwrap();
        let out=Process::new(std::env::current_exe().unwrap()).args(["--exact","source_claims::protection::tests::adoption_tests::policy_registry::execution_crash::budget_child_exit","--nocapture"]).env("TIG_POLICY_BUDGET_CASE_ROOT",&root).output().unwrap();
        assert_eq!(
            out.status.code(),
            Some(92),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        drop(r);
        r = Registry::open(&root.join("accounted"), &j).unwrap();
        c.f.nodes = metadata::MetadataStore::open(&root.join("nodes"), &m).unwrap();
        let proof = Proof::load(&c.f.nodes, &context, observed.review(), &decision).unwrap();
        let held = r
            .hold(&key("main"), Some(&proof.grant().version), &c.f.nodes)
            .unwrap();
        let head = coordinated::accepted_head::hold_current(&mut c.f.index, &mut c.source).unwrap();
        r.reserve_run(
            &held,
            &head,
            &proof,
            &context,
            observed.review(),
            &decision,
            &mut c.f.nodes,
        )
        .unwrap();
        r.check_run(
            &held,
            &proof,
            &context,
            observed.review(),
            &decision,
            &c.f.nodes,
        )
        .unwrap();
        let before = r.checkpoint();
        r.reserve_run(
            &held,
            &head,
            &proof,
            &context,
            observed.review(),
            &decision,
            &mut c.f.nodes,
        )
        .unwrap();
        assert_eq!(r.checkpoint(), before);
        drop(head);
        drop(held);
        assert_eq!(
            stage_variant(&mut c, &mut r, "third", "third-catalogue", 99).unwrap_err(),
            intent::Error::Budget
        );
    }
}
