//! A real Git return selects references; it does not manufacture deleted bytes.
use super::*;
pub(super) fn run(d: &mut Demo, first: &str, prior: &str) -> Result<(String, Object)> {
    let repo = d.root.join("project");
    let before = d.copy_snapshot()?;
    let source_before = d.namespaces[0].checkpoint();
    let first_record = git(
        &d.git,
        &repo,
        &["show", &format!("{first}:records/series.asset")],
    )?;
    git(
        &d.git,
        &repo,
        &[
            "restore",
            "--source",
            first,
            "--staged",
            "--worktree",
            "--",
            "records",
        ],
    )?;
    let returned = d.commit("Sample: select v1 references after final loss")?;
    let returned_record = git(
        &d.git,
        &repo,
        &["show", &format!("{returned}:records/series.asset")],
    )?;
    if first_record != returned_record || returned == first || returned == prior {
        return Err("Git return did not create an exact new source selection".into());
    }
    d.select("main", &returned, "return-after-final-loss")?;
    let source_selected = d.namespaces[0].checkpoint();
    let selected = d.namespaces[0]
        .source(&key("main"))?
        .ok_or("missing returned source")?;
    let view = d.repository.commit(GitId::parse(&returned)?)?;
    let snapshot = d.repository.native(
        &view,
        Scope::new([LogicalPath::parse("records")?])?,
        Scope::new([LogicalPath::parse("definitions")?])?,
        native::Limits::default(),
    )?;
    if selected
        .current
        .as_ref()
        .is_none_or(|s| &s.selection.context != snapshot.checkpoint())
    {
        return Err("returned source does not select the exact native snapshot".into());
    }
    let attempts_before = d.seed.attempts;
    d.seed.disabled = true;
    let (plan, report, status) = d.with_placement(|executor| {
        let plan = executor.plan(None)?;
        let report = executor.apply(&plan)?;
        let status = executor.status(&plan)?;
        Ok((plan, report, status))
    })?;
    if report.page_converged
        || report.storage_converged != Some(false)
        || !report.holds.contains(&placement::Error::FinalRetirement)
        || status.storage_converged != Some(false)
        || d.namespaces[0].checkpoint() != source_selected
    {
        return Err("Git return concealed final-loss storage hold".into());
    }
    let after = d.copy_snapshot()?;
    let stage = d.fixture["stages"]
        .as_array()
        .ok_or("missing stages")?
        .iter()
        .find(|s| s["id"] == "git-return-after-final-loss")
        .ok_or("missing return oracle")?;
    let mut observations = BTreeMap::new();
    for (name, object) in &d.objects {
        let actual = ["working", "history", "backup"]
            .into_iter()
            .filter(|role| after[*role].get(name).is_some())
            .collect::<BTreeSet<_>>();
        let expected = stage["expected_copies"][name]
            .as_array()
            .ok_or("missing return copies")?
            .iter()
            .map(|v| v.as_str().ok_or("invalid return role"))
            .collect::<std::result::Result<BTreeSet<_>, _>>()?;
        if actual != expected {
            return Err(
                format!("return/{name}: expected {expected:?}, observed {actual:?}").into(),
            );
        }
        observations.insert(
            name,
            json!({"object":object,"verified_copies":actual,"unknown_copies":[]}),
        );
    }
    let report = json!({"schema":"tig.teaching-git-return-observation.design-1","lesson":"git-return-after-final-loss",
        "source_commit":returned,"source_checkpoint":source_selected,"plan":plan,"receipt":report,"status":status,
        "observations":observations,"declared_fixture_domains":3,"physical_domain_coverage":"not-established; portable fixture directories",
        "independent_hardware_backup_proven":false,
        "git_return":{"first_commit":first,"prior_commit":prior,"returned_commit":returned,
            "record_path":"records/series.asset","first_record":Object::from_bytes(first_record.as_bytes()),
            "returned_record":Object::from_bytes(returned_record.as_bytes()),"source_before":source_before,
            "source_selected":source_selected,"selected_source":selected,"returned_snapshot":snapshot.checkpoint(),"seed_disabled":d.seed.disabled,
            "seed_attempts_before":attempts_before,"seed_attempts_after":d.seed.attempts,
            "lost_object":d.objects["original"],"before":before,"after":after,"authorizes_storage":false}});
    let evidence = save(&d.root.join("git-return.json"), &report)?;
    fs::copy(
        d.root.join("git-return.json"),
        Layout::Flat.object_path(&d.root.join("lesson-evidence"), &evidence.sha256),
    )?;
    d.reports.push(
        json!({"lesson":"git-return-after-final-loss","report":evidence,"file":"git-return.json",
        "title":"Git returned; payload still missing","event":stage["event"]}),
    );
    Ok((returned, evidence))
}
