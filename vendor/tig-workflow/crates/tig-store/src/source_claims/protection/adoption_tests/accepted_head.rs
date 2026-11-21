use super::*;
use crate::source_claims::coordinated::accepted_head::hold_current;
use tig_core::retirement::review::routine_policy::{Run, RunConfig};

fn run(head: &coordinated::accepted_head::HeldHead) -> Run {
    Run::new(RunConfig {
        namespace: head.journal().clone(),
        frame: head.frame().clone(),
        catalogue: d("old-catalogue"),
        provider_bindings: d("admitted-bindings"),
        admission: d("not-standing-approval"),
        native_sources: Default::default(),
    })
    .unwrap()
}

#[test]
fn held_head_excludes_coordinated_writers_and_allows_unrelated_control_progress() {
    let mut c = Case::new();
    let head = hold_current(&mut c.f.index, &mut c.source).unwrap();
    let request = run(&head);
    head.require_run(&request).unwrap();
    assert_eq!(head.owner(), &key("source-owner"));
    assert_eq!(head.source(), &key("main"));
    let batch = c.batch("held-merge", 10);
    assert!(matches!(
        adoption::stage(&mut c.f.index, &mut c.source, &batch, None, &mut c.f.nodes),
        Err(Error::Journal(claims::Error::Busy))
    ));
    let current = c.source.source(&key("main")).unwrap().unwrap();
    assert!(matches!(
        c.source.apply(
            key("blocked-cancel"),
            Command::Cancel {
                source: key("main"),
                owner: key("source-owner"),
                expected_revision: current.revision,
            }
        ),
        Err(claims::Error::Busy)
    ));
    c.source
        .apply(
            key("unrelated-control"),
            Command::Prepare {
                source: key("unrelated"),
                owner: key("another-owner"),
                expected_revision: 0,
                selection: head.selection().clone(),
            },
        )
        .unwrap();
    head.require_run(&request).unwrap();
    drop(head);
    let reference = c.stage(&batch);
    c.run(&reference, |_| Ok(())).unwrap();
    let next = hold_current(&mut c.f.index, &mut c.source).unwrap();
    assert!(next.require_run(&request).is_err());
}

#[test]
fn accepted_generation_ignores_history_retirement_but_not_repeated_selection() {
    let mut c = Case::new();
    let initial = hold_current(&mut c.f.index, &mut c.source).unwrap();
    let initial_frame = initial.frame().clone();
    let initial_selection = initial.selection().clone();
    drop(initial);
    let batch = c.batch("new-generation", 10);
    let work = c.stage(&batch);
    c.run(&work, |_| Ok(())).unwrap();
    let selected = hold_current(&mut c.f.index, &mut c.source).unwrap();
    let selected_frame = selected.frame().clone();
    drop(selected);
    let state = c.source.source(&key("main")).unwrap().unwrap();
    assert_eq!(
        selected_frame.generation,
        state.current.as_ref().unwrap().revision
    );
    c.source
        .apply(
            key("retire-old-source-root"),
            Command::Retire {
                source: key("main"),
                owner: key("source-owner"),
                expected_revision: state.revision,
                predecessor_revision: state.predecessors[0].revision,
            },
        )
        .unwrap();
    let same = hold_current(&mut c.f.index, &mut c.source).unwrap();
    assert_eq!(same.frame(), &selected_frame);
    drop(same);
    let mut revert = c.batch("return-original-selection", 0);
    revert
        .updates
        .iter_mut()
        .find(|u| u.actor == key("main"))
        .unwrap()
        .selection = initial_selection;
    let work = c.stage(&revert);
    c.run(&work, |_| Ok(())).unwrap();
    let returned = hold_current(&mut c.f.index, &mut c.source).unwrap();
    assert_eq!(returned.frame().selection, initial_frame.selection);
    assert_ne!(returned.frame().generation, initial_frame.generation);
    assert_ne!(returned.frame(), &initial_frame);
}

#[test]
fn interrupted_adoption_and_foreign_enrollment_supply_no_head_lease() {
    let mut c = Case::uninitialized();
    assert!(hold_current(&mut c.f.index, &mut c.source).is_err());
    adoption::enrollment::initialize(&mut c.f.index, &mut c.source, &mut c.f.nodes).unwrap();
    let batch = c.batch("interrupted-head", 10);
    let work = c.stage(&batch);
    assert!(matches!(
        c.run(&work, |cut| if cut == Cut::SourceAdopted {
            Err(Error::Injected)
        } else {
            Ok(())
        }),
        Err(Error::Injected)
    ));
    // The logical new source exists, but its protection/control handoff is pending.
    assert!(matches!(
        hold_current(&mut c.f.index, &mut c.source),
        Err(claims::Error::Pending)
    ));
    c.run(&work, |_| Ok(())).unwrap();
    let head = hold_current(&mut c.f.index, &mut c.source).unwrap();
    let mut changed = run(&head).config().clone();
    changed.namespace = d("foreign-namespace");
    assert!(head.require_run(&Run::new(changed).unwrap()).is_err());
    drop(head);
    let mut other = competing_source(&c, "foreign-source-head");
    assert!(hold_current(&mut c.f.index, &mut other).is_err());
}
