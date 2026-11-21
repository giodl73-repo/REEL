use super::*;
use git_asset_core::tree::NodeStore;
use serde_json::{Value, json};
use std::{
    path::Path,
    process::{Command, Output},
};

fn call(
    cli: &std::ffi::OsStr,
    root: &Path,
    profile: &Value,
    operation: &str,
    input: &Value,
) -> Output {
    std::fs::write(
        root.join("paged-profile.json"),
        serde_json::to_vec(profile).unwrap(),
    )
    .unwrap();
    std::fs::write(
        root.join("paged-input.json"),
        git_asset_core::identity::canonical_json(input).unwrap(),
    )
    .unwrap();
    Command::new(cli)
        .args(["paged", operation])
        .arg(root.join("paged-profile.json"))
        .arg(root.join("paged-input.json"))
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
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["code"], expected, "{v}");
}
fn profile(
    f: &Fixture,
    e: &control::Enrollment,
    dir: &Path,
    clock: &crate::clock::Enrollment,
    minimum: &crate::ledger::Checkpoint,
) -> Value {
    let mut p = inspection_profile(f);
    p["schema"] = json!("tig.paged-operator-profile.design-1");
    p["enrollment"] = json!(e);
    p["admitted_context"] = json!(e.context);
    p["clock"] = json!({"root":dir.join("clock"),"minimum":minimum,"enrollment":clock});
    p["permissions"] = json!({"stage":true,"apply":true,"cancel":true});
    p["run_limits"] = json!({"pages":16,"actions":1});
    p
}
#[test]
#[ignore = "requires explicitly fresh git-asset-storage executable"]
fn explicit_cli_paged_operator_qualification() {
    let cli = std::env::var_os("GIT_ASSET_PAGED_CLI").expect("explicit fresh CLI artifact");
    let describe = value(
        Command::new(&cli)
            .args(["paged", "describe"])
            .output()
            .unwrap(),
    );
    assert_eq!(describe["permissions_default_enabled"], false);
    assert_eq!(describe["cancel_requires_clock"], false);
    for cancel in [false, true] {
        let (mut f, old, clock_dir, clock, minimum) = super::super::operator::native();
        let (e, decision, _) = compile_with_window(&mut f, 15, old.request().window.clone());
        let mut p = profile(&f, &e, clock_dir.path(), &clock, &minimum);
        let original = serde_json::to_value(&decision).unwrap();
        let mut disabled = p.clone();
        disabled.as_object_mut().unwrap().remove("permissions");
        denied(
            call(&cli, f.temp.path(), &disabled, "copies", &json!({})),
            "staging_disabled",
        );
        denied(
            call(&cli, f.temp.path(), &disabled, "apply", &original),
            "execution_disabled",
        );
        let header: Value = serde_json::from_slice(
            &f.metadata
                .get(&decision.request().manifest)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        let request = json!({"schema":"tig.paged-copy-request.design-1","base":null,"copies":f.request().copies});
        let copy_response = value(call(&cli, f.temp.path(), &p, "copies", &request));
        let plan_request = json!({"schema":"tig.paged-plan-request.design-1","operation":"paged-cli","selection":header["request"],"copies":copy_response["copies"],"window":decision.request().window});
        let staged = value(call(&cli, f.temp.path(), &p, "plan", &plan_request));
        let decision: control::Decision =
            serde_json::from_value(staged["decision"].clone()).unwrap();
        let portable = staged["decision"].clone();
        p["metadata"]["minimum"] = staged["minimum"].clone();
        let files = inspection_files(&f.temp.path().join("witness"));
        let mut wrong = p.clone();
        wrong["claim_release"] = json!(decision.id().unwrap());
        wrong["final_loss"] = json!(d("other-decision"));
        wrong["metadata"]["root"] = json!(f.temp.path().join("missing"));
        denied(
            call(&cli, f.temp.path(), &wrong, "apply", &portable),
            "unapproved_decision",
        );
        assert_eq!(files, inspection_files(&f.temp.path().join("witness")));
        assert!(!f.temp.path().join("missing").exists());
        p["claim_release"] = json!(decision.id().unwrap());
        p["final_loss"] = p["claim_release"].clone();
        let mut offline = p.clone();
        offline["clock"] = Value::Null;
        offline["witness"]["root"] = json!(f.temp.path().join("missing"));
        offline["stores"][0]["root"] = json!(f.temp.path().join("missing"));
        for ns in offline["namespaces"].as_array_mut().unwrap() {
            ns["root"] = json!(f.temp.path().join("missing"));
        }
        let scope = value(call(&cli, f.temp.path(), &offline, "subjects", &portable));
        assert_eq!(scope["total"], 4);
        assert_eq!(scope["historical_only"], true);
        assert_eq!(scope["authorizes_removal"], false);
        for (id, kind) in [
            (d("different-decision"), "subjects"),
            (decision.id().unwrap(), "actions"),
        ] {
            let cursor = json!({"schema":"tig.paged-scope-cursor.design-1","decision":id,"kind":kind,"start":0});
            std::fs::write(
                f.temp.path().join("cursor.json"),
                serde_json::to_vec(&cursor).unwrap(),
            )
            .unwrap();
            denied(
                Command::new(&cli)
                    .args(["paged", "subjects"])
                    .arg(f.temp.path().join("paged-profile.json"))
                    .arg(f.temp.path().join("paged-input.json"))
                    .arg(f.temp.path().join("cursor.json"))
                    .output()
                    .unwrap(),
                "invalid_cursor",
            );
        }
        let observed = value(call(&cli, f.temp.path(), &p, "status", &portable));
        assert_eq!(observed["phase"], "not_started");
        assert_eq!(observed["total_pages"], 20);
        let peak = value(call(&cli, f.temp.path(), &p, "peak", &portable));
        assert!(peak["witness_peak_bytes"].as_u64().unwrap() > 0);
        let first = value(call(&cli, f.temp.path(), &p, "apply", &portable));
        assert_eq!(first["phase"], "intent");
        assert_eq!(first["completed_pages"], 16);
        if cancel {
            offline["witness"] = p["witness"].clone();
            offline["metadata"]["root"] = json!(f.temp.path().join("missing"));
            let stopped = value(call(&cli, f.temp.path(), &offline, "cancel", &portable));
            assert_eq!(stopped["phase"], "cancelled");
            assert_eq!(stopped["cleanup_unknown"], true);
            assert_eq!(stopped["completed_pages"], Value::Null);
            assert!(!f.temp.path().join("missing").exists());
            let resumed = value(call(&cli, f.temp.path(), &p, "resume", &portable));
            assert_eq!(resumed["phase"], "cancelled");
            for name in ["a", "b", "c"] {
                assert!(
                    f.store
                        .object_present(&f.publications[name].object)
                        .unwrap()
                );
            }
        } else {
            let mut result = first;
            for _ in 0..5 {
                result = value(call(&cli, f.temp.path(), &p, "resume", &portable));
                if result["phase"] == "completed" {
                    break;
                }
            }
            assert_eq!(result["phase"], "completed", "{result}");
            assert_eq!(result["completed_pages"], 20);
            assert_eq!(result["historical_deleted_actions"], 3);
            for name in ["a", "b", "c"] {
                assert!(
                    !f.store
                        .object_present(&f.publications[name].object)
                        .unwrap()
                );
            }
        }
        for name in ["shared", "keep"] {
            assert!(
                f.store
                    .object_present(&f.publications[name].object)
                    .unwrap()
            );
        }
    }
}

#[test]
#[ignore = "requires explicitly fresh git-asset-storage executable"]
fn explicit_cli_aged_paged_operator_qualification() {
    use crate::{clock::LocalClock, source_claims::age};
    let cli = std::env::var_os("GIT_ASSET_PAGED_CLI").expect("explicit fresh CLI artifact");
    let (mut f, old, clock_dir, clock, minimum) = super::super::operator::native();
    let (mut e, manual, _) = compile_with_window(&mut f, 15, old.request().window.clone());
    let ages = super::aged::AgeFixture::new(&mut f, &mut e);
    let ledger = CapacityLedger::open(&clock_dir.path().join("clock"), &minimum).unwrap();
    let port = LocalClock::new(ledger, clock.clone()).unwrap();
    let inventory = selection::inspect(
        &e.selection,
        &manual.request().manifest,
        &mut f.metadata.finished_reader().unwrap(),
        &selection::Limits::default(),
    )
    .unwrap();
    let mut registry = Namespace::open(&f.temp.path().join("ages"), &ages.minimum).unwrap();
    for cohort in inventory.cohorts() {
        let at = f
            .namespaces
            .iter()
            .position(|n| n.identity() == &cohort.origin.namespace)
            .unwrap();
        let minimum = f.namespaces[at].checkpoint();
        age::Capture {
            enrollment: &e.age.as_ref().unwrap().enrollment,
            registry: &mut registry,
            minimum: &ages.minimum,
            metadata: &mut f.metadata,
            limits: &age::Limits::default(),
        }
        .capture(
            &age::Request {
                namespace: cohort.origin.namespace.clone(),
                source: cohort.origin.source.clone(),
                predecessor_revision: cohort.origin.predecessor_revision,
            },
            &mut f.namespaces[at],
            &minimum,
            &port,
        )
        .unwrap();
    }
    // This clock admits a 5 s sample plus 1 s drift uncertainty. Wait beyond
    // that upper bound and the policy's 50 ms interval, rather than treating the
    // sample's wall timestamp as an exact observation time.
    std::thread::sleep(std::time::Duration::from_secs(7));
    let mut p = profile(&f, &e, clock_dir.path(), &clock, &minimum);
    p["age_registry"] = json!({"root":f.temp.path().join("ages"),"minimum":ages.minimum});
    p["run_limits"] = json!({"pages":16,"actions":16});
    let header: Value =
        serde_json::from_slice(&f.metadata.get(&manual.request().manifest).unwrap().unwrap())
            .unwrap();
    // Freeze the execution window at planning, after the observation interval.
    // It remains unchanged on every approved apply/resume call.
    let now = port.sample(&e.clock).unwrap();
    let window = provider::Window {
        not_before: now.earliest - 1000,
        expires: now.latest + 45_000,
    };
    let input = json!({"schema":"tig.paged-plan-request.design-1","operation":"paged-aged-cli","selection":header["request"],"copies":manual.request().copies,"window":window});
    let staged = value(call(&cli, f.temp.path(), &p, "plan", &input));
    p["metadata"]["minimum"] = staged["minimum"].clone();
    p["claim_release"] = staged["decision_id"].clone();
    p["final_loss"] = staged["decision_id"].clone();
    let decision = staged["decision"].clone();
    assert!(decision["age"].is_object());
    let mut missing = p.clone();
    missing["age_registry"] = Value::Null;
    denied(
        call(&cli, f.temp.path(), &missing, "apply", &decision),
        "registry_required",
    );
    let first = value(call(&cli, f.temp.path(), &p, "apply", &decision));
    assert_eq!(first["completed_pages"], 16);
    let mut readonly = p.clone();
    readonly["clock"] = Value::Null;
    let status = value(call(&cli, f.temp.path(), &readonly, "status", &decision));
    assert_eq!(status["completed_pages"], 16);
    assert_eq!(status["age_inheritance_unknown"], false);
    let done = value(call(&cli, f.temp.path(), &p, "resume", &decision));
    assert_eq!(done["phase"], "completed", "{done}");
    assert_eq!(done["historical_deleted_actions"], 3);
    assert_eq!(done["age_inheritance_unknown"], false);
    for name in ["a", "b", "c"] {
        assert!(
            !f.store
                .object_present(&f.publications[name].object)
                .unwrap()
        );
    }
    for name in ["shared", "keep"] {
        assert!(
            f.store
                .object_present(&f.publications[name].object)
                .unwrap()
        );
    }
}
