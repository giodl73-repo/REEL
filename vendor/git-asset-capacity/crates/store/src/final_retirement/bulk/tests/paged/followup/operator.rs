use super::*;
use serde_json::{Value, json};
use std::{
    path::Path,
    process::{Command as Process, Output},
};

fn call(
    cli: &std::ffi::OsStr,
    root: &Path,
    profile: &Value,
    op: &str,
    input: &Value,
    followup: bool,
) -> Output {
    std::fs::write(
        root.join("fresh-profile.json"),
        serde_json::to_vec(profile).unwrap(),
    )
    .unwrap();
    std::fs::write(
        root.join("fresh-input.json"),
        git_asset_core::identity::canonical_json(input).unwrap(),
    )
    .unwrap();
    let mut c = Process::new(cli);
    c.arg("paged");
    if followup {
        c.arg("followup");
    }
    c.arg(op)
        .arg(root.join("fresh-profile.json"))
        .arg(root.join("fresh-input.json"))
        .output()
        .unwrap()
}
#[track_caller]
fn value(output: Output) -> Value {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    assert!(output.stdout.len() <= 65536);
    serde_json::from_slice(&output.stdout).unwrap()
}
fn denied(output: Output, expected: &str) {
    assert!(!output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["code"],
        expected
    );
}
#[test]
#[ignore = "requires explicitly fresh git-asset-storage executable"]
fn explicit_cli_held_copy_followup_qualification() {
    use crate::clock::LocalClock;
    let cli = std::env::var_os("GIT_ASSET_FOLLOWUP_CLI").expect("explicit fresh CLI artifact");
    let described = value(
        Process::new(&cli)
            .args(["paged", "followup", "describe"])
            .output()
            .unwrap(),
    );
    assert_eq!(described["permissions_default_enabled"], false);
    assert_eq!(described["metadata_writer_on_apply"], false);
    for cancel in [false, true] {
        let (mut f, old, clock_dir, clock, minimum) = super::super::super::operator::native();
        let (e, original, l) = compile_with_window(&mut f, 0, old.request().window.clone());
        complete(&mut f, &e, &original, &l);
        for (op, command) in [
            (
                "end-job",
                Command::Withdraw {
                    source: key("job"),
                    owner: key("other"),
                    expected_revision: 3,
                },
            ),
            (
                "retire-job",
                Command::Retire {
                    source: key("job"),
                    owner: key("other"),
                    expected_revision: 4,
                    predecessor_revision: 3,
                },
            ),
        ] {
            f.namespaces[1]
                .apply(key(op), command, &f.metadata, &limits())
                .unwrap();
        }
        let original_job = f.witness_job.clone();
        LocalCapacityProbe::bind(&f.witness, &mut f.witness_ledger)
            .unwrap()
            .observe(
                &f.witness,
                &mut f.witness_ledger,
                key("followup-probe"),
                key("followup-sample"),
            )
            .unwrap();
        f.witness_job = f
            .witness_ledger
            .reserve(key("new-followup-job"), key("owner"), 128 * 1024 * 1024)
            .unwrap()
            .reference()
            .clone();
        let mut operator = inspection_profile(&f);
        operator["schema"] = json!("tig.paged-operator-profile.design-1");
        operator["enrollment"] = json!(e);
        operator["admitted_context"] = json!(e.context);
        operator["permissions"] = json!({"stage":true});
        operator["claim_release"] = json!(original.id().unwrap());
        operator["final_loss"] = json!(original.id().unwrap());
        operator["clock"] =
            json!({"root":clock_dir.path().join("clock"),"minimum":minimum,"enrollment":clock});
        for member in operator["roster"]["members"].as_array_mut().unwrap() {
            let ns = f
                .namespaces
                .iter()
                .find(|n| json!(n.identity()) == member["namespace"])
                .unwrap();
            member["minimum"] = json!(ns.checkpoint());
        }
        operator["namespaces"]=json!(f.namespaces.iter().enumerate().map(|(i,n)| json!({"root":f.temp.path().join(format!("ns-{i}")),"minimum":n.checkpoint()})).collect::<Vec<_>>());
        let copied = value(call(
            &cli,
            f.temp.path(),
            &operator,
            "copies",
            &json!({"schema":"tig.paged-copy-request.design-1","base":null,
            "copies":[Copy { store:f.store.identity().clone(),capacity:f.ledger.identity().clone(),publication:f.publications["shared"].clone() }]}),
            false,
        ));
        operator["metadata"]["minimum"] = copied["minimum"].clone();
        let mut profile = json!({"schema":"tig.held-copy-followup-profile.design-1","operator":operator,"original_witness_job":original_job,
            "original_jobs":[control::StoreJob { store:f.store.identity().clone(),job:f.job.clone() }],
            "permissions":{"stage":true,"apply":true,"cancel":true},"final_loss":null});
        let port = LocalClock::new(
            CapacityLedger::open(&clock_dir.path().join("clock"), &minimum).unwrap(),
            clock,
        )
        .unwrap();
        let now = port.sample(&e.clock).unwrap();
        let input = json!({"schema":"tig.held-copy-followup-request.design-1","operation":"cli-followup","original":original,"copies":copied["copies"],
            "window":{"not_before":now.earliest-1000,"expires":now.latest+45000}});
        let mut disabled = profile.clone();
        disabled["permissions"] = json!({});
        denied(
            call(&cli, f.temp.path(), &disabled, "plan", &input, true),
            "staging_disabled",
        );
        let staged = value(call(&cli, f.temp.path(), &profile, "plan", &input, true));
        profile["operator"]["metadata"]["minimum"] = staged["minimum"].clone();
        let portable = staged["decision"].clone();
        let mut wrong = profile.clone();
        wrong["final_loss"] = json!(original.id().unwrap());
        wrong["operator"]["metadata"]["root"] = json!(f.temp.path().join("missing"));
        denied(
            call(&cli, f.temp.path(), &wrong, "apply", &portable, true),
            "unapproved_decision",
        );
        assert!(!f.temp.path().join("missing").exists());
        profile["final_loss"] = staged["decision_id"].clone();
        let before = source_checkpoints(&f);
        let mut readonly = profile.clone();
        readonly["operator"]["clock"] = Value::Null;
        let status = value(call(
            &cli,
            f.temp.path(),
            &readonly,
            "status",
            &portable,
            true,
        ));
        assert_eq!(status["phase"], "not_started");
        assert_eq!(
            value(call(
                &cli,
                f.temp.path(),
                &readonly,
                "actions",
                &portable,
                true
            ))["total"],
            1
        );
        if cancel {
            let decision = fresh::Decision::from_json(
                &git_asset_core::identity::canonical_json(&portable).unwrap(),
            )
            .unwrap();
            let mut reader = crate::metadata::MetadataReader::open(
                &f.temp.path().join("metadata"),
                &serde_json::from_value(staged["minimum"].clone()).unwrap(),
            )
            .unwrap();
            let jobs = [control::StoreJob {
                store: f.store.identity().clone(),
                job: f.job.clone(),
            }];
            let mut places = [Local {
                store: &f.store,
                ledger: &mut f.ledger,
                job: &f.job,
            }];
            let mut runtime = fresh::Execution {
                enrollment: &e,
                roster: &f.roster,
                namespaces: &mut f.namespaces,
                metadata: &mut reader,
                places: &mut places,
                jobs: &jobs,
                witness: Local {
                    store: &f.witness,
                    ledger: &mut f.witness_ledger,
                    job: &f.witness_job,
                },
                clock: &port,
                admitted_context: &e.context,
                limits: &l,
                run_limits: fresh::RunLimits::default(),
            };
            assert!(matches!(
                runtime.apply_hook(
                    &decision,
                    &decision.authorize_final_loss().unwrap(),
                    None,
                    |stage| {
                        if stage == control::Stage::Intent {
                            Err(control::Error::Injected)
                        } else {
                            Ok(())
                        }
                    }
                ),
                Err(control::Error::Injected)
            ));
            let mut offline = readonly.clone();
            offline["operator"]["metadata"]["root"] = json!(f.temp.path().join("missing"));
            offline["operator"]["stores"][0]["root"] = json!(f.temp.path().join("missing"));
            for ns in offline["operator"]["namespaces"].as_array_mut().unwrap() {
                ns["root"] = json!(f.temp.path().join("missing"));
            }
            let stopped = value(call(
                &cli,
                f.temp.path(),
                &offline,
                "cancel",
                &portable,
                true,
            ));
            assert_eq!(stopped["phase"], "cancelled");
            assert_eq!(stopped["historical_deleted_actions"], Value::Null);
            assert_eq!(stopped["cleanup_unknown"], true);
            let cleaned = value(call(
                &cli,
                f.temp.path(),
                &readonly,
                "cancel",
                &portable,
                true,
            ));
            assert_eq!(cleaned["phase"], "cancelled");
            assert_eq!(cleaned["pending_cancellations"], 0);
            assert!(
                f.store
                    .read_object(&f.publications["shared"].object)
                    .is_ok()
            );
        } else {
            let done = value(call(
                &cli,
                f.temp.path(),
                &profile,
                "apply",
                &portable,
                true,
            ));
            assert_eq!(done["phase"], "completed");
            assert_eq!(done["historical_deleted_actions"], 1);
            assert!(
                !f.store
                    .object_present(&f.publications["shared"].object)
                    .unwrap()
            );
        }
        assert_eq!(source_checkpoints(&f), before);
        assert!(f.store.read_object(&f.publications["keep"].object).is_ok());
    }
}
