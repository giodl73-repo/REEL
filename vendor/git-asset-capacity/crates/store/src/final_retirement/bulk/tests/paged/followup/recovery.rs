use super::*;
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize)]
struct Saved {
    enrollment: control::Enrollment,
    decision: fresh::Decision,
    roster: claims::Roster,
    namespaces: Vec<source_claims::Checkpoint>,
    metadata: crate::metadata::Checkpoint,
    ledger: crate::ledger::Checkpoint,
    witness: crate::ledger::Checkpoint,
    job: JournalRef,
    witness_job: JournalRef,
    store: Digest,
}
#[test]
fn followup_child() {
    let Some(root) = std::env::var_os("GIT_ASSET_FOLLOWUP_CHILD") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let s: Saved =
        serde_json::from_slice(&std::fs::read(root.join("followup-saved.json")).unwrap()).unwrap();
    let witness = LocalStore::open(&root.join("witness")).unwrap();
    let mut witness_ledger =
        CapacityLedger::open(&root.join("witness-ledger"), &s.witness).unwrap();
    let jobs = [control::StoreJob {
        store: s.store.clone(),
        job: s.job.clone(),
    }];
    let l = selection::Limits::default();
    let stop = std::env::var("GIT_ASSET_FOLLOWUP_STOP").unwrap();
    let hook = |stage| {
        let reached = match stop.as_str() {
            "intent" => stage == control::Stage::Intent,
            "released" => matches!(
                stage,
                control::Stage::Provider(0, provider::Stage::Released)
            ),
            "removed" => stage == control::Stage::Removed(0),
            "completed" => stage == control::Stage::Completed,
            "cancelled" => stage == control::Stage::Cancelled,
            _ => false,
        };
        if reached {
            std::process::exit(91);
        }
        Ok(())
    };
    let mut witness = Local {
        store: &witness,
        ledger: &mut witness_ledger,
        job: &s.witness_job,
    };
    let auth = s.decision.authorize_final_loss().unwrap();
    if stop == "cancelled" {
        fresh::Cancellation {
            inspection: fresh::Inspection {
                enrollment: &s.enrollment,
                roster: &s.roster,
                metadata: None,
                places: &mut [],
                jobs: &jobs,
                witness: &mut witness,
                admitted_context: &s.enrollment.context,
                limits: &l,
            },
        }
        .cancel_hook(&s.decision, &auth, hook)
        .unwrap();
    } else {
        let mut namespaces = s
            .namespaces
            .iter()
            .enumerate()
            .map(|(i, min)| Namespace::open(&root.join(format!("ns-{i}")), min).unwrap())
            .collect::<Vec<_>>();
        let mut reader =
            crate::metadata::MetadataReader::open(&root.join("metadata"), &s.metadata).unwrap();
        let store = LocalStore::open(&root.join("M")).unwrap();
        let mut ledger = CapacityLedger::open(&root.join("ledger"), &s.ledger).unwrap();
        let mut places = [Local {
            store: &store,
            ledger: &mut ledger,
            job: &s.job,
        }];
        let clock = Clock(Cell::new(700), s.enrollment.clock.clone());
        fresh::Execution {
            enrollment: &s.enrollment,
            roster: &s.roster,
            namespaces: &mut namespaces,
            metadata: &mut reader,
            places: &mut places,
            jobs: &jobs,
            witness,
            clock: &clock,
            admitted_context: &s.enrollment.context,
            limits: &l,
            run_limits: fresh::RunLimits::default(),
        }
        .apply_hook(&s.decision, &auth, None, hook)
        .unwrap();
    }
    panic!("requested abrupt boundary was not reached");
}
#[test]
fn actual_child_exits_preserve_followup_intent_release_completion_and_stop() {
    for stop in ["intent", "released", "removed", "completed", "cancelled"] {
        let (mut f, e, _, l, next) = recovered();
        let before = source_checkpoints(&f);
        let saved = Saved {
            enrollment: e.clone(),
            decision: next.clone(),
            roster: f.roster.clone(),
            namespaces: before.clone(),
            metadata: f.metadata.checkpoint(),
            ledger: f.ledger.checkpoint(),
            witness: f.witness_ledger.checkpoint(),
            job: f.job.clone(),
            witness_job: f.witness_job.clone(),
            store: f.store.identity().clone(),
        };
        std::fs::write(
            f.temp.path().join("followup-saved.json"),
            serde_json::to_vec(&saved).unwrap(),
        )
        .unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "final_retirement::bulk::tests::paged::followup::recovery::followup_child",
                "--nocapture",
            ])
            .env("GIT_ASSET_FOLLOWUP_CHILD", f.temp.path())
            .env("GIT_ASSET_FOLLOWUP_STOP", stop)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(91), "{stop}: {output:?}");
        if matches!(stop, "completed" | "cancelled") {
            f.clock.0.set(0);
        }
        let report = execute(&mut f, &e, &l, |x| {
            x.apply(&next, &next.authorize_final_loss().unwrap())
        })
        .unwrap();
        assert_eq!(
            report.phase,
            if stop == "cancelled" {
                control::Phase::Cancelled
            } else {
                control::Phase::Completed
            },
            "{stop}: {:?}",
            report.holds
        );
        assert_eq!(
            report.historical_deleted_actions,
            Some(if stop == "cancelled" { 0 } else { 3 })
        );
        assert_eq!(source_checkpoints(&f), before);
        for name in ["a", "b", "c"] {
            assert_eq!(
                f.store
                    .object_present(&f.publications[name].object)
                    .unwrap(),
                stop == "cancelled"
            );
        }
        for name in ["shared", "keep"] {
            assert!(f.store.read_object(&f.publications[name].object).is_ok());
        }
    }
}
