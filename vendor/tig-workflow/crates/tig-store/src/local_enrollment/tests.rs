use super::*;
use crate::{AckLevel, Request as Publication};
use std::{
    io::{Cursor, Read},
    process::Command,
};
use tig_core::Object;
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
pub(super) fn request(temp: &tempfile::TempDir) -> Request {
    Request {
        schema: INPUT_SCHEMA.into(),
        root: temp.path().join("M"),
        operation: key("initialize-M"),
        place: key("M"),
        profile: StoreProfile {
            max_object_bytes: 8 * 1024 * 1024,
            removable: false,
        },
        capacity: Capacity::New {
            domain: key("volume"),
            epoch: key("epoch"),
            floor: 0,
        },
    }
}
#[test]
fn replay_preserves_published_bytes_identity_and_advanced_authority() {
    let temp = tempfile::tempdir().unwrap();
    let input = request(&temp);
    let first = initialize(&input).unwrap();
    assert_eq!(first.capacity.minimum_checkpoint.sequence(), 0);
    let mut store = LocalStore::open(&first.destination.root).unwrap();
    let bytes = b"public initialized-store fixture";
    let publication = Publication {
        operation: key("ingest"),
        owner: key("fixture"),
        object: Object::from_bytes(bytes),
        required: AckLevel::ProcessRestart,
    };
    crate::publish(&mut store, &publication, &mut Cursor::new(bytes)).unwrap();
    let mut ledger =
        CapacityLedger::open(&first.capacity.root, &first.capacity.minimum_checkpoint).unwrap();
    ledger.invalidate(key("invalidate")).unwrap();
    let witness = ledger.checkpoint();
    drop(ledger);
    let replay = initialize(&input).unwrap();
    assert_eq!(first.intent, replay.intent);
    assert_eq!(first.destination.place, replay.destination.place);
    assert_eq!(replay.capacity.minimum_checkpoint, witness);
    let mut actual = Vec::new();
    store
        .read_object(&publication.object)
        .unwrap()
        .read_to_end(&mut actual)
        .unwrap();
    assert_eq!(actual, bytes);
}
#[test]
fn changed_input_and_foreign_roots_refuse_without_overwrite() {
    let temp = tempfile::tempdir().unwrap();
    let input = request(&temp);
    initialize(&input).unwrap();
    let record = fs::read(input.root.join("enrollment.json")).unwrap();
    for n in 0..4 {
        let mut other = input.clone();
        match n {
            0 => other.operation = key("other"),
            1 => other.place = key("other"),
            2 => other.profile.removable = true,
            _ => {
                other.capacity = Capacity::New {
                    domain: key("other"),
                    epoch: key("epoch"),
                    floor: 0,
                }
            }
        }
        assert_eq!(initialize(&other).unwrap_err(), Error::Conflict);
    }
    assert_eq!(
        record,
        fs::read(input.root.join("enrollment.json")).unwrap()
    );
    let mut foreign = input;
    foreign.root = temp.path().join("foreign");
    fs::create_dir(&foreign.root).unwrap();
    fs::write(foreign.root.join("keep"), b"unmanaged bytes").unwrap();
    assert_eq!(initialize(&foreign).unwrap_err(), Error::Conflict);
    assert_eq!(
        fs::read(foreign.root.join("keep")).unwrap(),
        b"unmanaged bytes"
    );
}
#[test]
fn shared_capacity_attaches_without_new_authority_and_accepts_new_witness() {
    let temp = tempfile::tempdir().unwrap();
    let input = request(&temp);
    let first = initialize(&input).unwrap();
    let mut second = input.clone();
    second.root = temp.path().join("D");
    second.place = key("D");
    second.capacity = Capacity::Existing {
        root: first.capacity.root.clone(),
        ledger: first.capacity.ledger.clone(),
        minimum_checkpoint: first.capacity.minimum_checkpoint.clone(),
    };
    let attached = initialize(&second).unwrap();
    assert_eq!(attached.capacity.ledger, first.capacity.ledger);
    assert!(!second.root.join("capacity").exists());
    let mut ledger =
        CapacityLedger::open(&first.capacity.root, &first.capacity.minimum_checkpoint).unwrap();
    ledger.invalidate(key("advance")).unwrap();
    let witness = ledger.checkpoint();
    drop(ledger);
    if let Capacity::Existing {
        minimum_checkpoint, ..
    } = &mut second.capacity
    {
        *minimum_checkpoint = witness.clone();
    }
    let replay = initialize(&second).unwrap();
    assert_eq!(replay.intent, attached.intent);
    assert_eq!(replay.capacity.minimum_checkpoint, witness);
    if let Capacity::Existing { ledger, .. } = &mut second.capacity {
        *ledger = Digest::blob(b"foreign");
    }
    assert_eq!(initialize(&second).unwrap_err(), Error::Conflict);
}
#[test]
fn process_cut_child() {
    let Ok(path) = std::env::var("TIG_INIT_CUT_INPUT") else {
        return;
    };
    let input: Request = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let cut = std::env::var("TIG_INIT_CUT_STAGE").unwrap();
    initialize_hook(&input, |stage| {
        if format!("{stage:?}") == cut {
            std::process::exit(73)
        }
        Ok(())
    })
    .unwrap();
    panic!("requested cut did not execute");
}
#[test]
fn exact_process_restart_recovers_five_publication_boundaries() {
    for stage in [
        Stage::IntentPinned,
        Stage::StoreReady,
        Stage::CapacityReady,
        Stage::RecordReady,
        Stage::Published,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let input = request(&temp);
        let path = temp.path().join("request.json");
        fs::write(&path, serde_json::to_vec(&input).unwrap()).unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "local_enrollment::tests::process_cut_child",
                "--nocapture",
            ])
            .env("TIG_INIT_CUT_INPUT", path)
            .env("TIG_INIT_CUT_STAGE", format!("{stage:?}"))
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(73),
            "{stage:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let enrolled = initialize(&input).unwrap();
        assert_eq!(enrolled.capacity.minimum_checkpoint.sequence(), 0);
        let replay = initialize(&input).unwrap();
        assert_eq!(enrolled.destination.place, replay.destination.place);
        assert_eq!(enrolled.capacity.ledger, replay.capacity.ledger);
    }
}
#[test]
fn uncertain_partial_store_and_foreign_stage_entries_are_preserved() {
    let temp = tempfile::tempdir().unwrap();
    let input = request(&temp);
    let context = Context::prepare(&input).unwrap();
    {
        let _fence = context.fence().unwrap();
        context.prepare_stage().unwrap();
    }
    fs::create_dir(context.stage.join("store")).unwrap();
    fs::write(context.stage.join("store/keep"), b"partial").unwrap();
    assert_eq!(
        initialize(&input).unwrap_err(),
        Error::PendingInitialization
    );
    assert_eq!(
        fs::read(context.stage.join("store/keep")).unwrap(),
        b"partial"
    );
    fs::write(context.stage.join("unknown"), b"foreign").unwrap();
    assert_eq!(initialize(&input).unwrap_err(), Error::Conflict);
    assert!(!input.root.exists());
}
#[test]
fn scoped_fence_blocks_overlap_but_not_other_store_initialization() {
    let temp = tempfile::tempdir().unwrap();
    let input = request(&temp);
    let context = Context::prepare(&input).unwrap();
    let fence = context.fence().unwrap();
    assert_eq!(initialize(&input).unwrap_err(), Error::Busy);
    let mut other = input.clone();
    other.root = temp.path().join("F");
    other.place = key("F");
    initialize(&other).unwrap();
    drop(fence);
    initialize(&input).unwrap();
}
#[test]
fn invalid_input_creates_no_target_and_used_unpublished_authority_is_not_reset() {
    let temp = tempfile::tempdir().unwrap();
    let mut input = request(&temp);
    input.profile.max_object_bytes = 0;
    assert_eq!(initialize(&input).unwrap_err(), Error::Invalid);
    assert!(!input.root.exists());
    input = request(&temp);
    assert_eq!(
        initialize_hook(&input, |s| if s == Stage::CapacityReady {
            Err(Error::Injected)
        } else {
            Ok(())
        })
        .unwrap_err(),
        Error::Injected
    );
    let context = Context::prepare(&input).unwrap();
    let mut ledger = match &input.capacity {
        Capacity::New {
            domain,
            epoch,
            floor,
        } => CapacityLedger::reopen_unpublished_empty(
            &context.stage.join("capacity"),
            domain,
            epoch,
            *floor,
        )
        .unwrap(),
        _ => unreachable!(),
    };
    let witness = ledger.checkpoint();
    ledger.invalidate(key("unexpected-work")).unwrap();
    drop(ledger);
    assert_eq!(
        initialize(&input).unwrap_err(),
        Error::PendingInitialization
    );
    assert_eq!(
        CapacityLedger::open(&context.stage.join("capacity"), &witness)
            .unwrap()
            .checkpoint()
            .sequence(),
        1
    );
}

#[test]
fn staged_record_survives_shared_authority_progress_without_rewriting_witness() {
    let temp = tempfile::tempdir().unwrap();
    let mut input = request(&temp);
    let first = initialize(&input).unwrap();
    input.root = temp.path().join("D");
    input.place = key("D");
    input.capacity = Capacity::Existing {
        root: first.capacity.root.clone(),
        ledger: first.capacity.ledger.clone(),
        minimum_checkpoint: first.capacity.minimum_checkpoint.clone(),
    };
    assert_eq!(
        initialize_hook(&input, |s| if s == Stage::RecordReady {
            Err(Error::Injected)
        } else {
            Ok(())
        })
        .unwrap_err(),
        Error::Injected
    );
    let context = Context::prepare(&input).unwrap();
    let original = fs::read(context.stage.join("enrollment.json")).unwrap();
    let mut ledger =
        CapacityLedger::open(&first.capacity.root, &first.capacity.minimum_checkpoint).unwrap();
    ledger.invalidate(key("advance-after-stage")).unwrap();
    let witness = ledger.checkpoint();
    drop(ledger);
    let enrolled = initialize(&input).unwrap();
    assert_eq!(enrolled.capacity.minimum_checkpoint, witness);
    assert_eq!(
        fs::read(input.root.join("enrollment.json")).unwrap(),
        original
    );
}

#[test]
fn unexpected_staged_payload_is_neither_adopted_nor_removed() {
    let temp = tempfile::tempdir().unwrap();
    let input = request(&temp);
    assert_eq!(
        initialize_hook(&input, |s| if s == Stage::StoreReady {
            Err(Error::Injected)
        } else {
            Ok(())
        })
        .unwrap_err(),
        Error::Injected
    );
    let context = Context::prepare(&input).unwrap();
    let path = context.stage.join("store/objects/keep");
    fs::write(&path, b"unacknowledged bytes").unwrap();
    assert_eq!(
        initialize(&input).unwrap_err(),
        Error::PendingInitialization
    );
    assert_eq!(fs::read(path).unwrap(), b"unacknowledged bytes");
    assert!(!input.root.exists());
}

#[test]
fn partial_intent_marker_stays_pending_and_changed_intent_cannot_rebind() {
    let temp = tempfile::tempdir().unwrap();
    let input = request(&temp);
    let context = Context::prepare(&input).unwrap();
    drop(context.fence().unwrap());
    let mut changed = input.clone();
    changed.operation = key("different");
    assert_eq!(initialize(&changed).unwrap_err(), Error::Conflict);
    let marker = temp.path().join(".M.tig-init.lock");
    fs::write(&marker, b"partial").unwrap();
    assert_eq!(
        initialize(&input).unwrap_err(),
        Error::PendingInitialization
    );
    assert_eq!(fs::read(marker).unwrap(), b"partial");
    assert!(!input.root.exists());
}
