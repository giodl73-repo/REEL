use super::*;
use crate::{clock, ledger::Checkpoint};
use serde_json::{Value, json};
use std::io::Read;

fn assert_bytes(f: &Fixture, name: &str) {
    assert_object(&f.store, &f.publications[name].object, name.as_bytes());
}
fn assert_object(store: &LocalStore, object: &Object, expected: &[u8]) {
    let mut reader = store.read_object(object).unwrap();
    let mut actual = Vec::new();
    reader.read_to_end(&mut actual).unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn cancellation_without_source_metadata_or_time_preserves_settled_and_held_history() {
    for stop in [
        Stage::Intent,
        Stage::Settled(1),
        Stage::Removed(0),
        Stage::Completed,
    ] {
        let mut f = Fixture::new();
        let p = f.plan();
        assert!(
            f.run(&p, |e| e.apply_hook(&p, &p.authorize().unwrap(), |stage| {
                if stage == stop {
                    Err(Error::Injected)
                } else {
                    Ok(())
                }
            }))
            .is_err()
        );
        let metadata = f.metadata.checkpoint().identity().clone();
        drop(f.metadata);
        std::fs::rename(
            f.temp.path().join("metadata"),
            f.temp.path().join("offline"),
        )
        .unwrap();
        let sources = f
            .namespaces
            .iter()
            .map(Namespace::checkpoint)
            .collect::<Vec<_>>();
        let jobs = vec![f.job.clone(); p.actions.len()];
        let mut places = [Local {
            store: &f.store,
            ledger: &mut f.ledger,
            job: &f.job,
        }];
        let mut witness = Local {
            store: &f.witness,
            ledger: &mut f.witness_ledger,
            job: &f.witness_job,
        };
        let mut cancel = Cancellation {
            inspection: Inspection {
                enrollment: &f.enrollment,
                roster: &f.roster,
                namespaces: &mut f.namespaces,
                metadata,
                places: &mut places,
                action_jobs: &jobs,
                witness: &mut witness,
                admitted_context: &f.enrollment.context,
            },
        };
        let result = cancel.cancel(&p, &p.authorize().unwrap()).unwrap();
        assert_eq!(
            result.phase,
            if stop == Stage::Completed {
                Phase::Completed
            } else {
                Phase::Cancelled
            }
        );
        assert!(result.holds.is_empty(), "{stop:?}: {:?}", result.holds);
        assert_eq!(
            sources,
            f.namespaces
                .iter()
                .map(Namespace::checkpoint)
                .collect::<Vec<_>>()
        );
        for name in ["shared", "keep"] {
            assert_object(&f.store, &f.publications[name].object, name.as_bytes());
        }
        assert!(!f.temp.path().join("metadata").exists());
    }
}

pub(super) fn native() -> (
    Fixture,
    Plan,
    tempfile::TempDir,
    clock::Enrollment,
    Checkpoint,
) {
    let dir = tempfile::tempdir().unwrap();
    let ledger = CapacityLedger::create_clock(
        &dir.path().join("clock"),
        key("clock"),
        key("epoch"),
        0,
        clock::Policy {
            authority: d("native-clock"),
            epoch: d("clock-epoch"),
            boundary: d("test-os"),
            max_sample_millis: 5000,
            max_drift_millis: 1000,
            max_session_millis: 60_000,
        },
    )
    .unwrap();
    let enrollment = ledger.clock_enrollment().unwrap();
    let id = enrollment.id().unwrap();
    let mut f = Fixture::with_clock(id.clone(), 60_000);
    let port = clock::LocalClock::new(ledger, enrollment.clone()).unwrap();
    let now = port.sample(&id).unwrap();
    f.clock.0.set(now.earliest);
    let mut request = f.request();
    request.window = provider::Window {
        not_before: now.earliest - 1000,
        expires: now.latest + 45_000,
    };
    let p = plan(
        &f.enrollment,
        request,
        &f.roster,
        &mut f.namespaces,
        &f.metadata.finished_reader().unwrap(),
        &[&f.store],
    )
    .unwrap();
    let minimum = port.status().unwrap().checkpoint;
    (f, p, dir, enrollment, minimum)
}
fn profile(
    f: &Fixture,
    p: &Plan,
    root: &std::path::Path,
    enrollment: &clock::Enrollment,
    minimum: &Checkpoint,
) -> Value {
    json!({"schema":"tig.bulk-execution-profile.design-1", "inspection":inspection_profile(f),
        "admitted_plan":p.id().unwrap(),"permissions":{"apply":true,"cancel":true},
        "clock":{"root":root.join("clock"),"minimum":minimum,"enrollment":enrollment}})
}
fn call(
    cli: &std::ffi::OsStr,
    f: &Fixture,
    p: &Plan,
    profile: &Value,
    operation: &str,
) -> std::process::Output {
    let dir = f.temp.path();
    std::fs::write(
        dir.join("execution.json"),
        serde_json::to_vec(profile).unwrap(),
    )
    .unwrap();
    std::fs::write(dir.join("approved.json"), p.to_json().unwrap()).unwrap();
    std::process::Command::new(cli)
        .args(["bulk", "execute", operation])
        .arg(dir.join("execution.json"))
        .arg(dir.join("approved.json"))
        .output()
        .unwrap()
}
#[track_caller]
fn value(output: std::process::Output) -> Value {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    serde_json::from_slice(&output.stdout).unwrap()
}
fn denied(output: std::process::Output, expected: &str, dir: &std::path::Path) {
    assert!(!output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["code"], expected);
    let private = serde_json::to_string(&dir.to_string_lossy()).unwrap();
    assert!(
        !String::from_utf8(output.stdout)
            .unwrap()
            .contains(&private[1..private.len() - 1])
    );
}
fn checkpoints(f: &Fixture, root: &std::path::Path, minimum: &Checkpoint) -> Vec<Value> {
    let p = inspection_profile(f);
    let mut result = Vec::new();
    for n in p["namespaces"].as_array().unwrap() {
        let floor = serde_json::from_value(n["minimum"].clone()).unwrap();
        result.push(
            serde_json::to_value(
                Namespace::open(std::path::Path::new(n["root"].as_str().unwrap()), &floor)
                    .unwrap()
                    .checkpoint(),
            )
            .unwrap(),
        );
    }
    for store in [&p["stores"][0], &p["witness"]] {
        let floor = serde_json::from_value(store["minimum"].clone()).unwrap();
        result.push(
            serde_json::to_value(
                CapacityLedger::open(
                    std::path::Path::new(store["ledger_root"].as_str().unwrap()),
                    &floor,
                )
                .unwrap()
                .checkpoint(),
            )
            .unwrap(),
        );
    }
    result.push(
        serde_json::to_value(
            CapacityLedger::open(&root.join("clock"), minimum)
                .unwrap()
                .checkpoint(),
        )
        .unwrap(),
    );
    result
}

#[test]
#[ignore = "requires explicitly fresh git-asset-storage executable"]
fn explicit_cli_bulk_execution_qualification() {
    let cli = std::env::var_os("GIT_ASSET_BULK_CLI").expect("explicit fresh CLI artifact");
    let describe = value(
        std::process::Command::new(&cli)
            .args(["bulk", "execute", "describe"])
            .output()
            .unwrap(),
    );
    assert_eq!(describe["permissions_default_enabled"], false);
    assert_eq!(describe["cancel_requires_clock"], false);
    assert_eq!(describe["cancel_requires_metadata_writer"], false);
    // Preflight denials must leave actual reopened source/capacity/clock journals
    // and provider records unchanged, not merely original handles' cached views.
    let (f, p, clock_root, enrollment, minimum) = native();
    let admitted = profile(&f, &p, clock_root.path(), &enrollment, &minimum);
    let before = checkpoints(&f, clock_root.path(), &minimum);
    let files = (
        inspection_files(&f.temp.path().join("M")),
        inspection_files(&f.temp.path().join("witness")),
    );
    let mut cases = Vec::new();
    let mut wrong = admitted.clone();
    wrong["permissions"] = json!({});
    cases.push((wrong, "execution_disabled"));
    let mut wrong = admitted.clone();
    wrong.as_object_mut().unwrap().remove("permissions");
    cases.push((wrong, "execution_disabled"));
    let mut wrong = admitted.clone();
    wrong["admitted_plan"] = json!(d("unapproved"));
    cases.push((wrong, "unapproved_plan"));
    let mut wrong = admitted.clone();
    wrong["clock"] = Value::Null;
    cases.push((wrong, "clock_required"));
    let mut wrong = admitted.clone();
    wrong["clock"]["enrollment"]["policy"]["authority"] = json!(d("other-clock"));
    cases.push((wrong, "invalid_clock"));
    let mut wrong = admitted.clone();
    wrong["clock"]["root"] = json!(clock_root.path().join("missing"));
    cases.push((wrong, "clock_unavailable"));
    let mut wrong = admitted.clone();
    wrong["inspection"]["metadata"]["root"] = json!(f.temp.path().join("missing"));
    cases.push((wrong, "metadata_unavailable"));
    let mut wrong = admitted.clone();
    wrong["inspection"]["admitted_context"] = json!(d("wrong"));
    cases.push((wrong, "invalid_profile"));
    let mut wrong = admitted.clone();
    wrong["extra"] = json!(true);
    cases.push((wrong, "invalid_document"));
    for (wrong, expected) in cases {
        denied(call(&cli, &f, &p, &wrong, "apply"), expected, f.temp.path());
    }
    let mut wrong = admitted.clone();
    wrong["permissions"] = json!({"apply":true});
    denied(
        call(&cli, &f, &p, &wrong, "cancel"),
        "execution_disabled",
        f.temp.path(),
    );
    wrong["permissions"] = json!({"cancel":true});
    denied(
        call(&cli, &f, &p, &wrong, "resume"),
        "execution_disabled",
        f.temp.path(),
    );
    let mut wrong = admitted.clone();
    wrong["inspection"]["stores"][0]["root"] = json!(f.temp.path().join("witness"));
    denied(
        call(&cli, &f, &p, &wrong, "apply"),
        "wrong_store",
        f.temp.path(),
    );
    assert_eq!(before, checkpoints(&f, clock_root.path(), &minimum));
    assert_eq!(
        files,
        (
            inspection_files(&f.temp.path().join("M")),
            inspection_files(&f.temp.path().join("witness"))
        )
    );
    assert!(!clock_root.path().join("missing").exists());
    assert!(!f.temp.path().join("missing").exists());
    f.clock.0.set(0); // Execution must use its enrolled native clock.
    let report = value(call(&cli, &f, &p, &admitted, "apply"));
    assert_eq!(report["phase"], "completed");
    assert_eq!(report["historical_deleted_copy_bytes"], 3);
    assert_eq!(report["excluded"].as_array().unwrap().len(), 1);
    for name in ["a", "b", "c"] {
        assert!(
            !f.store
                .object_present(&f.publications[name].object)
                .unwrap()
        );
    }
    for name in ["shared", "keep"] {
        assert_bytes(&f, name);
    }
    let after = checkpoints(&f, clock_root.path(), &minimum);
    assert_eq!(
        value(call(&cli, &f, &p, &admitted, "resume"))["phase"],
        "completed"
    );
    assert_eq!(after, checkpoints(&f, clock_root.path(), &minimum));
    // Interrupted owned source work resumes through the actual subprocess.
    let (mut f2, p2, dir2, e2, min2) = native();
    let admitted2 = profile(&f2, &p2, dir2.path(), &e2, &min2);
    assert!(
        f2.run(&p2, |e| e.apply_hook(
            &p2,
            &p2.authorize().unwrap(),
            |stage| {
                if stage == Stage::Settled(1) {
                    Err(Error::Injected)
                } else {
                    Ok(())
                }
            }
        ))
        .is_err()
    );
    assert_eq!(
        value(call(&cli, &f2, &p2, &admitted2, "resume"))["phase"],
        "completed"
    );
    // Cancellation is exact and independent of fresh time/source metadata.
    for stop in [Stage::Intent, Stage::Settled(1), Stage::Removed(0)] {
        let (mut f, p, dir, enrollment, minimum) = native();
        let mut admitted = profile(&f, &p, dir.path(), &enrollment, &minimum);
        assert!(
            f.run(&p, |e| e.apply_hook(&p, &p.authorize().unwrap(), |stage| {
                if stage == stop {
                    Err(Error::Injected)
                } else {
                    Ok(())
                }
            }))
            .is_err()
        );
        admitted["clock"] = Value::Null;
        admitted["inspection"]["metadata"]["root"] = json!(f.temp.path().join("missing"));
        let clock_before = CapacityLedger::open(&dir.path().join("clock"), &minimum)
            .unwrap()
            .checkpoint();
        let sources = f
            .namespaces
            .iter()
            .map(Namespace::checkpoint)
            .collect::<Vec<_>>();
        let r = value(call(&cli, &f, &p, &admitted, "cancel"));
        assert_eq!(r["phase"], "cancelled");
        assert!(r["holds"].as_array().unwrap().is_empty(), "{r}");
        let current = sources
            .iter()
            .enumerate()
            .map(|(i, c)| {
                Namespace::open(&f.temp.path().join(format!("ns-{i}")), c)
                    .unwrap()
                    .checkpoint()
            })
            .collect::<Vec<_>>();
        assert_eq!(sources, current);
        assert_eq!(
            clock_before,
            CapacityLedger::open(&dir.path().join("clock"), &minimum)
                .unwrap()
                .checkpoint()
        );
        for name in ["shared", "keep"] {
            assert_bytes(&f, name);
        }
        assert!(!f.temp.path().join("missing").exists());
    }
    // New use refuses source work before settlement, or holds provider actions
    // after settlement; preserve the controller's distinct error/report outcomes.
    for stop in [Stage::Intent, Stage::Settled(1)] {
        let (mut f3, p3, dir3, e3, min3) = native();
        let admitted3 = profile(&f3, &p3, dir3.path(), &e3, &min3);
        assert!(
            f3.run(&p3, |e| e.apply_hook(
                &p3,
                &p3.authorize().unwrap(),
                |stage| {
                    if stage == stop {
                        Err(Error::Injected)
                    } else {
                        Ok(())
                    }
                }
            ))
            .is_err()
        );
        transient_use(&mut f3);
        let output = call(&cli, &f3, &p3, &admitted3, "resume");
        if stop == Stage::Intent {
            denied(output, "stale_basis", f3.temp.path());
        } else {
            let r = value(output);
            assert_ne!(r["phase"], "completed");
            assert!(!r["holds"].as_array().unwrap().is_empty());
        }
        for name in ["a", "b", "c", "shared", "keep"] {
            assert_bytes(&f3, name);
        }
    }
    // An expired native window cannot settle source history, but separately
    // admitted cancellation remains possible without a clock or metadata root.
    let (mut f4, original, dir4, e4, min4) = native();
    let mut request = original.request().clone();
    request.window = provider::Window {
        not_before: 1,
        expires: 2,
    };
    let p4 = plan(
        &f4.enrollment,
        request,
        &f4.roster,
        &mut f4.namespaces,
        &f4.metadata.finished_reader().unwrap(),
        &[&f4.store],
    )
    .unwrap();
    let mut admitted4 = profile(&f4, &p4, dir4.path(), &e4, &min4);
    let before = checkpoints(&f4, dir4.path(), &min4);
    denied(
        call(&cli, &f4, &p4, &admitted4, "apply"),
        "provider_held",
        f4.temp.path(),
    );
    let after = checkpoints(&f4, dir4.path(), &min4);
    assert_eq!(&before[..before.len() - 1], &after[..after.len() - 1]);
    admitted4["clock"] = Value::Null;
    admitted4["inspection"]["metadata"]["root"] = json!(f4.temp.path().join("missing"));
    assert_eq!(
        value(call(&cli, &f4, &p4, &admitted4, "cancel"))["phase"],
        "cancelled"
    );
    for name in ["a", "b", "c", "shared", "keep"] {
        assert_bytes(&f4, name);
    }
}
