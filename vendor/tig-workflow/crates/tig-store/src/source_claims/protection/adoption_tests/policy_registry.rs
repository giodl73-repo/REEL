use super::*;
use crate::provider::profile::{Binding as ProviderBinding, Profile, Qualification as Q, Schema};
use crate::source_claims::policy_registry::{
    self as registry, PlaceScope, ProviderCatalogue, Registration, Registry, RegistryError,
    Update as PolicyUpdate, UpdateStage,
};
use std::{fs, process::Command as Process};
use tig_core::retirement::review::routine_policy::{Limits as PolicyLimits, Policy, PolicyConfig};
#[path = "policy_registry/execution.rs"]
mod execution;
#[path = "policy_registry/execution_crash.rs"]
mod execution_crash;

fn create(c: &mut Case) -> Registry {
    let head = coordinated::accepted_head::hold_current(&mut c.f.index, &mut c.source).unwrap();
    let binding =
        registry::Binding::from_head(&c.f.index, &head, [key("main"), key("other")].into())
            .unwrap();
    drop(head);
    Registry::create(
        &c.f.temp.path().join("registry"),
        key("policy-epoch"),
        binding,
    )
    .unwrap()
}
fn request(
    c: &mut Case,
    r: &Registry,
    slot: &str,
    version: u64,
    base: u64,
    op: &str,
) -> PolicyUpdate {
    let head = coordinated::accepted_head::hold_current(&mut c.f.index, &mut c.source).unwrap();
    let mut profiles = BTreeMap::new();
    for place in ["F", "M"] {
        let p = Profile {
            schema: Schema::Experimental1,
            binding: ProviderBinding {
                place: d(place),
                authority: d("fixture-config"),
                generation: 1,
            },
            adapter: key("fixture"),
            adapter_build: d("build"),
            provider: key("unqualified-fixture"),
            provider_build: d("provider-build"),
            read: Q::Unsupported,
            publish: Q::Unsupported,
            custody: Q::Unsupported,
            issue: Q::Unsupported,
            release: Q::Unsupported,
            remove: Q::Unsupported,
        };
        profiles.insert(p.fingerprint().unwrap(), p);
    }
    let places = [("F", "working-domain"), ("M", "backup-domain")]
        .map(|(place, domain)| {
            (
                d(place),
                PlaceScope {
                    capacity_domain: d("shared-test-capacity"),
                    failure_domain: key(domain),
                },
            )
        })
        .into();
    let catalogue = ProviderCatalogue::new(profiles, places).unwrap();
    let providers = c.f.nodes.put(&catalogue.to_json().unwrap()).unwrap();
    let policy = Policy::new(PolicyConfig {
        namespace: c.source.identity().clone(),
        owner: d("subject"),
        source: head.frame().source.clone(),
        authority: head.frame().authority.clone(),
        epoch: head.frame().epoch.clone(),
        version,
        placement_policy: d("policy"),
        provider_bindings: providers.sha256.clone(),
        removable: [d("F")].into(),
        required_survivors: [d("M")].into(),
        limits: PolicyLimits {
            maximum_objects: 1,
            maximum_actions: 2,
            maximum_object_bytes: 1024,
            maximum_total_object_bytes: 2048,
        },
    })
    .unwrap();
    drop(head);
    let policy = c.f.nodes.put(&policy.to_json().unwrap()).unwrap();
    PolicyUpdate {
        slot: key(slot),
        owner: r.binding().owner().clone(),
        operation: key(op),
        expected_revision: base,
        registration: Registration::new(policy, providers).unwrap(),
    }
}

#[test]
fn registration_reopens_and_raw_journal_or_index_cannot_bypass_the_profile() {
    let mut c = Case::new();
    let mut r = create(&mut c);
    let req = request(&mut c, &r, "main", 1, 0, "enable");
    r.update(&req, &mut c.f.nodes).unwrap();
    let root = c.f.temp.path().join("registry");
    let minimum = r.checkpoint();
    let mut raw = SourceJournal::open(&root, &minimum).unwrap();
    let rows = raw
        .control_sources(r.binding().owner(), "policy-slot-", 16)
        .unwrap();
    assert_eq!(rows.len(), 1);
    let before = raw.checkpoint();
    assert!(matches!(
        raw.apply(
            key("bypass"),
            Command::Cancel {
                source: rows[0].source.clone(),
                owner: req.owner.clone(),
                expected_revision: rows[0].revision
            }
        ),
        Err(claims::Error::Restricted)
    ));
    assert_eq!(raw.checkpoint(), before);
    assert!(Index::open(&root, &minimum).is_err());
    drop(raw);
    drop(r);
    let mut reopened = Registry::open(&root, &minimum).unwrap();
    let held = reopened.hold(&key("main"), None, &c.f.nodes).unwrap();
    assert_eq!(held.version().revision, 2);
    assert_eq!(held.policy().config().version, 1);
    assert_eq!(held.providers().len(), 2);
}

#[test]
fn multiple_readers_fence_revoke_while_another_slot_can_progress() {
    let mut c = Case::new();
    let mut r = create(&mut c);
    let req = request(&mut c, &r, "main", 1, 0, "enable");
    r.update(&req, &mut c.f.nodes).unwrap();
    let held = r.hold(&key("main"), None, &c.f.nodes).unwrap();
    let version = held.version().clone();
    let mut other = Registry::open(&c.f.temp.path().join("registry"), &r.checkpoint()).unwrap();
    let second = other
        .hold(&key("main"), Some(&version), &c.f.nodes)
        .unwrap();
    assert!(matches!(
        other.revoke(&key("main"), &req.owner, 2, key("stop")),
        Err(RegistryError::Journal(claims::Error::Busy))
    ));
    let independent = request(&mut c, &other, "other", 1, 0, "enable-other");
    other.update(&independent, &mut c.f.nodes).unwrap();
    drop(held);
    drop(second);
    other
        .revoke(&key("main"), &req.owner, 2, key("stop"))
        .unwrap();
    assert!(matches!(
        r.hold(&key("main"), Some(&version), &c.f.nodes),
        Err(RegistryError::Revoked)
    ));
    assert!(other.hold(&key("other"), None, &c.f.nodes).is_ok());
}

#[test]
fn exact_registration_replay_never_renews_a_revoked_or_replaced_grant() {
    let mut c = Case::new();
    let mut r = create(&mut c);
    let req = request(&mut c, &r, "main", 1, 0, "enable");
    r.update(&req, &mut c.f.nodes).unwrap();
    let initial = r.hold(&key("main"), None, &c.f.nodes).unwrap();
    let version = initial.version().clone();
    drop(initial);
    let newer = request(&mut c, &r, "main", 2, 2, "replace");
    r.update(&newer, &mut c.f.nodes).unwrap();
    assert!(matches!(
        r.hold(&key("main"), Some(&version), &c.f.nodes),
        Err(RegistryError::Stale)
    ));
    let before = r.checkpoint();
    let mut unavailable = nodes(&c.f.temp.path().join("unrelated-empty-metadata"));
    assert!(r.update(&req, &mut unavailable).unwrap().replayed);
    assert_eq!(r.checkpoint(), before);
    r.revoke(&key("main"), &req.owner, 4, key("stop")).unwrap();
    let before = r.checkpoint();
    assert!(r.update(&newer, &mut unavailable).unwrap().replayed);
    assert_eq!(r.checkpoint(), before);
    assert!(matches!(
        r.hold(&key("main"), None, &unavailable),
        Err(RegistryError::Revoked)
    ));
    let new_attempt = request(&mut c, &r, "main", 3, 5, "cannot-reenable");
    assert!(matches!(
        r.update(&new_attempt, &mut c.f.nodes),
        Err(RegistryError::Revoked)
    ));
}

#[test]
fn changed_payload_under_an_accepted_operation_conflicts_before_metadata_access() {
    let mut c = Case::new();
    let mut r = create(&mut c);
    let req = request(&mut c, &r, "main", 1, 0, "enable");
    r.update(&req, &mut c.f.nodes).unwrap();
    let mut changed = request(&mut c, &r, "main", 2, 0, "enable");
    let before = r.checkpoint();
    let mut empty = nodes(&c.f.temp.path().join("empty-for-conflict"));
    assert!(matches!(
        r.update(&changed, &mut empty),
        Err(RegistryError::Journal(claims::Error::Conflict))
    ));
    assert_eq!(r.checkpoint(), before);
    changed.operation = key("another-operation");
    assert!(matches!(
        r.update(&changed, &mut c.f.nodes),
        Err(RegistryError::Stale)
    ));
}

#[test]
fn unknown_slots_and_foreign_owner_do_not_expand_fences_or_admit_records() {
    let mut c = Case::new();
    let mut r = create(&mut c);
    let mut req = request(&mut c, &r, "main", 1, 0, "enable");
    let fences = c.f.temp.path().join("registry/fences");
    let before = fs::read_dir(&fences).unwrap().count();
    let checkpoint = r.checkpoint();
    for i in 0..100 {
        let slot = key(&format!("unregistered-{i}"));
        assert!(matches!(
            r.hold(&slot, None, &c.f.nodes),
            Err(RegistryError::Scope)
        ));
    }
    req.owner = key("foreign-owner");
    assert!(matches!(
        r.update(&req, &mut c.f.nodes),
        Err(RegistryError::Scope)
    ));
    assert_eq!(fs::read_dir(&fences).unwrap().count(), before);
    assert_eq!(r.checkpoint(), checkpoint);
}

#[test]
fn policy_scope_catalogue_and_version_drift_refuse_before_journal_admission() {
    let mut c = Case::new();
    let mut r = create(&mut c);
    let original = request(&mut c, &r, "main", 1, 0, "enable");
    let registration: serde_json::Value = serde_json::to_value(&original.registration).unwrap();
    let policy_ref: Object = serde_json::from_value(registration["policy"].clone()).unwrap();
    let bytes = c.f.nodes.get(&policy_ref).unwrap().unwrap();
    let original_policy = Policy::from_json(&bytes).unwrap();
    let providers: Object = serde_json::from_value(registration["providers"].clone()).unwrap();
    let before = r.checkpoint();
    for mode in 0..7 {
        let mut config = original_policy.config().clone();
        match mode {
            0 => config.namespace = d("foreign-namespace"),
            1 => config.owner = d("foreign-subject"),
            2 => config.source = d("foreign-source"),
            3 => config.epoch = d("foreign-epoch"),
            4 => config.placement_policy = d("unenrolled-placement-policy"),
            5 => {
                config.removable.insert(d("unenrolled-store"));
            }
            _ => config.provider_bindings = d("wrong-provider-catalogue"),
        }
        let policy = Policy::new(config).unwrap();
        let root = c.f.nodes.put(&policy.to_json().unwrap()).unwrap();
        let mut bad = original.clone();
        bad.registration = Registration::new(root, providers.clone()).unwrap();
        assert!(r.update(&bad, &mut c.f.nodes).is_err(), "mode {mode}");
        assert_eq!(r.checkpoint(), before);
    }
    r.update(&original, &mut c.f.nodes).unwrap();
    let mut config = original_policy.config().clone();
    config.limits.maximum_object_bytes += 1;
    let same_version = Policy::new(config).unwrap();
    let root = c.f.nodes.put(&same_version.to_json().unwrap()).unwrap();
    let mut changed = original.clone();
    changed.operation = key("different-policy-same-version");
    changed.expected_revision = 2;
    changed.registration = Registration::new(root, providers).unwrap();
    let before = r.checkpoint();
    assert!(matches!(
        r.update(&changed, &mut c.f.nodes),
        Err(RegistryError::Scope)
    ));
    assert_eq!(r.checkpoint(), before);
}

#[test]
fn trusted_newer_minimum_rejects_an_actual_registry_rollback() {
    let mut c = Case::new();
    let mut r = create(&mut c);
    let req = request(&mut c, &r, "main", 1, 0, "enable");
    r.update(&req, &mut c.f.nodes).unwrap();
    let old = r.checkpoint();
    let root = c.f.temp.path().join("registry");
    fs::copy(
        root.join("claims.sqlite"),
        c.f.temp.path().join("old-registry.sqlite"),
    )
    .unwrap();
    r.revoke(&key("main"), &req.owner, 2, key("stop")).unwrap();
    let newest = r.checkpoint();
    drop(r);
    fs::copy(
        c.f.temp.path().join("old-registry.sqlite"),
        root.join("claims.sqlite"),
    )
    .unwrap();
    assert!(matches!(
        Registry::open(&root, &newest),
        Err(RegistryError::Journal(claims::Error::Rollback))
    ));
    // A locally valid old prefix is not evidence of globally latest authority.
    // Keep the trusted minimum outside the restore being validated.
    assert!(Registry::open(&root, &old).is_ok());
}

#[test]
fn legacy_headers_keep_original_bytes_and_reject_registry_null_or_schema_mixing() {
    let root = tempfile::tempdir().unwrap();
    let plain = root.path().join("plain");
    let j = SourceJournal::create(&plain, key("legacy"), key("epoch")).unwrap();
    let cp = j.checkpoint();
    drop(j);
    let bytes = fs::read(plain.join("claims.json")).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["schema"], "tig.source-claims.design-1");
    assert!(value.get("registry").is_none());
    assert!(SourceJournal::open(&plain, &cp).is_ok());
    value["registry"] = serde_json::Value::Null;
    fs::write(
        plain.join("claims.json"),
        identity::canonical_json(&value).unwrap(),
    )
    .unwrap();
    assert!(SourceJournal::open(&plain, &cp).is_err());
    let mut c = Case::new();
    let r = create(&mut c);
    let cp = r.checkpoint();
    drop(r);
    let path = c.f.temp.path().join("registry/claims.json");
    let mut v: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(v["schema"], "tig.source-claims.design-7");
    v["schema"] = "tig.source-claims.design-1".into();
    fs::write(path, identity::canonical_json(&v).unwrap()).unwrap();
    assert!(Registry::open(&c.f.temp.path().join("registry"), &cp).is_err());
}

#[test]
fn registry_child_exit() {
    let Ok(root) = std::env::var("TIG_POLICY_REGISTRY_CASE_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let input: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("child-request.json")).unwrap()).unwrap();
    let mut r = Registry::open(
        &root.join("registry"),
        &serde_json::from_value(input["journal"].clone()).unwrap(),
    )
    .unwrap();
    let mut metadata = metadata::MetadataStore::open(
        &root.join("nodes"),
        &serde_json::from_value(input["metadata"].clone()).unwrap(),
    )
    .unwrap();
    let req: PolicyUpdate = serde_json::from_value(input["request"].clone()).unwrap();
    let cut = input["cut"].as_u64().unwrap();
    r.update_hook(&req, &mut metadata, |stage| {
        let n = match stage {
            UpdateStage::MetadataReady => 1,
            UpdateStage::Prepared => 2,
            UpdateStage::Adopted => 3,
        };
        if n == cut {
            std::process::exit(91)
        }
        Ok(())
    })
    .unwrap();
    panic!("expected actual process exit");
}

#[test]
fn actual_process_exits_recover_exact_registration_and_pending_policy_refuses_holds() {
    for cut in 1..=3 {
        let mut c = Case::new();
        let mut r = create(&mut c);
        let req = request(&mut c, &r, "main", 1, 0, "enable");
        let m = c.f.nodes.finish().unwrap();
        let j = r.checkpoint();
        let root = c.f.temp.path();
        fs::write(
            root.join("child-request.json"),
            serde_json::to_vec(&serde_json::json!({
                "journal":j,"metadata":m,"request":req,"cut":cut,
            }))
            .unwrap(),
        )
        .unwrap();
        let out=Process::new(std::env::current_exe().unwrap())
            .args(["--exact","source_claims::protection::tests::adoption_tests::policy_registry::registry_child_exit","--nocapture"])
            .env("TIG_POLICY_REGISTRY_CASE_ROOT",root).output().unwrap();
        assert_eq!(
            out.status.code(),
            Some(91),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        drop(r);
        r = Registry::open(&root.join("registry"), &j).unwrap();
        c.f.nodes = metadata::MetadataStore::open(&root.join("nodes"), &m).unwrap();
        match cut {
            1 => assert!(matches!(
                r.hold(&key("main"), None, &c.f.nodes),
                Err(RegistryError::Missing)
            )),
            2 => assert!(matches!(
                r.hold(&key("main"), None, &c.f.nodes),
                Err(RegistryError::Pending)
            )),
            _ => assert!(r.hold(&key("main"), None, &c.f.nodes).is_ok()),
        }
        r.update(&req, &mut c.f.nodes).unwrap();
        let version = r
            .hold(&key("main"), None, &c.f.nodes)
            .unwrap()
            .version()
            .clone();
        assert_eq!(version.revision, 2);
        let before = r.checkpoint();
        assert!(r.update(&req, &mut c.f.nodes).unwrap().replayed);
        assert_eq!(r.checkpoint(), before);
    }
}

#[test]
fn approved_domain_changes_bind_new_policy_version_and_old_holds_cannot_upgrade() {
    let mut c = Case::new();
    let mut r = create(&mut c);
    let req = request(&mut c, &r, "main", 1, 0, "enable");
    r.update(&req, &mut c.f.nodes).unwrap();
    let held = r.hold(&key("main"), None, &c.f.nodes).unwrap();
    let old = held.version().clone();
    let mut config = held.policy().config().clone();
    let mut places = held.catalogue().places().clone();
    places.get_mut(&d("M")).unwrap().failure_domain = key("new-approved-backup-domain");
    let changed = ProviderCatalogue::new(held.providers().clone(), places).unwrap();
    let bytes = changed.to_json().unwrap();
    assert_eq!(ProviderCatalogue::from_json(&bytes).unwrap(), changed);
    for field in ["places", "profiles"] {
        let mut v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        v.as_object_mut().unwrap().remove(field);
        assert!(ProviderCatalogue::from_json(&identity::canonical_json(&v).unwrap()).is_err());
    }
    let mut unknown: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    unknown["unknown"] = serde_json::Value::Null;
    assert!(ProviderCatalogue::from_json(&identity::canonical_json(&unknown).unwrap()).is_err());
    let providers = c.f.nodes.put(&bytes).unwrap();
    config.provider_bindings = providers.sha256.clone();
    let same = Policy::new(config.clone()).unwrap();
    let same_root = c.f.nodes.put(&same.to_json().unwrap()).unwrap();
    let mut update = PolicyUpdate {
        slot: key("main"),
        owner: req.owner.clone(),
        operation: key("change-domains"),
        expected_revision: 2,
        registration: Registration::new(same_root, providers.clone()).unwrap(),
    };
    drop(held);
    assert!(matches!(
        r.update(&update, &mut c.f.nodes),
        Err(RegistryError::Scope)
    ));
    config.version = 2;
    let policy = Policy::new(config).unwrap();
    update.registration = Registration::new(
        c.f.nodes.put(&policy.to_json().unwrap()).unwrap(),
        providers,
    )
    .unwrap();
    r.update(&update, &mut c.f.nodes).unwrap();
    assert!(matches!(
        r.hold(&key("main"), Some(&old), &c.f.nodes),
        Err(RegistryError::Stale)
    ));
    let current = r.hold(&key("main"), None, &c.f.nodes).unwrap();
    assert_eq!(
        current.catalogue().places()[&d("M")].failure_domain,
        key("new-approved-backup-domain")
    );
}
