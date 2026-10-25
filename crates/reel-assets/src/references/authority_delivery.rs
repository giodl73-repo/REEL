//! Explicit reference records, not rights decisions or remote-delivery verification.
use super::*;

pub(super) const AUTHORITY: &str = "reel.source-authority-record.v1";
pub(super) const DELIVERY: &str = "reel.delivery-record.v1";

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceAuthority {
    pub schema: String,
    pub authority_id: String,
    pub revisions: Vec<SourceRevision>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceRevision {
    pub revision_id: String,
    pub document: CacheDocument,
    pub scope_ids: Vec<String>,
    /// Exact opaque evidence. Its presence grants no approval or permission.
    pub evidence: Vec<CacheDocument>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeliveryRecord {
    pub schema: String,
    pub delivery_id: String,
    pub subject: versions::Dependency,
    pub audience: Audience,
    pub outputs: Vec<DeliveryOutput>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Audience {
    Private,
    Public,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeliveryOutput {
    pub role: String,
    pub language: String,
    pub asset: CacheDocument,
    pub evidence: Vec<CacheDocument>,
}

fn label(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 256 && !s.chars().any(char::is_control)
}
fn descriptor(d: &CacheDocument) -> Result<()> {
    if d.bytes == 0
        || !valid_oid(&d.sha256)
        || d.cache_uri != format!("cache://sha256/{}", d.sha256)
    {
        return Err("Reference requires nonempty bytes and an exact cache descriptor".into());
    }
    Ok(())
}
fn evidence(ds: &[CacheDocument]) -> Result<()> {
    if ds.len() > 128 {
        return Err("Evidence count exceeds 128".into());
    }
    let mut seen = BTreeSet::new();
    for d in ds {
        descriptor(d)?;
        if !seen.insert(&d.sha256) {
            return Err("Duplicate evidence object".into());
        }
    }
    Ok(())
}

impl Inspector<'_> {
    fn reference_asset(
        &mut self,
        id: &str,
        d: &CacheDocument,
        kind: &str,
        field: &str,
    ) -> Result<()> {
        self.asset(
            id,
            &Asset {
                sha256: d.sha256.clone(),
                bytes: d.bytes,
                cache_uri: d.cache_uri.clone(),
                extra: BTreeMap::new(),
            },
            kind,
            field,
        )?;
        Ok(())
    }

    pub(super) fn source_authority(&mut self, id: &str, value: Value) -> Result<()> {
        let r: SourceAuthority = serde_json::from_value(value)?;
        if r.schema != AUTHORITY
            || !label(&r.authority_id)
            || r.revisions.is_empty()
            || r.revisions.len() > 128
        {
            return Err("Invalid authority identity or revision count".into());
        }
        // Validate the whole record before adding relationships.
        let mut revisions = BTreeSet::new();
        let mut scopes = BTreeSet::new();
        for v in &r.revisions {
            if !label(&v.revision_id)
                || !revisions.insert(&v.revision_id)
                || v.scope_ids.is_empty()
                || v.scope_ids.len() > 10000
            {
                return Err("Invalid source revision identity or scopes".into());
            }
            descriptor(&v.document)?;
            evidence(&v.evidence)?;
            for scope in &v.scope_ids {
                if !label(scope) || !scopes.insert(scope.clone()) {
                    return Err(
                        "Source scopes must be bounded and uniquely assigned to a revision".into(),
                    );
                }
            }
            if scopes.len() > 10000 {
                return Err("Source scope count exceeds 10000".into());
            }
        }
        for v in &r.revisions {
            self.reference_asset(
                id,
                &v.document,
                "source-document",
                &format!("revisions/{}/document", v.revision_id),
            )?;
            for d in &v.evidence {
                self.reference_asset(
                    id,
                    d,
                    "authority-evidence",
                    &format!("revisions/{}/evidence", v.revision_id),
                )?;
            }
            if v.evidence.is_empty() {
                self.finding(
                    id,
                    "missing-authority-evidence",
                    format!(
                        "Revision {} has no decision/provenance evidence",
                        v.revision_id
                    ),
                );
            }
        }
        let n = self.report.nodes.get_mut(id).unwrap();
        n.details["declared_scope_ids"] = json!(scopes);
        n.details["authority_verified"] = json!(false);
        self.finding(id, "authority-semantics-unverified", "Exact source and opaque evidence references only; source content, authorship, rights, consent and approval have not been verified");
        Ok(())
    }

    pub(super) fn delivery_record(&mut self, id: &str, value: Value, depth: usize) -> Result<()> {
        let r: DeliveryRecord = serde_json::from_value(value)?;
        if r.schema != DELIVERY
            || !label(&r.delivery_id)
            || r.outputs.is_empty()
            || r.outputs.len() > 128
        {
            return Err("Invalid delivery identity or output count".into());
        }
        let mut outputs = BTreeSet::new();
        for o in &r.outputs {
            if !label(&o.role) || !label(&o.language) || !outputs.insert((&o.role, &o.language)) {
                return Err("Delivery role/language pairs must be bounded and unique".into());
            }
            descriptor(&o.asset)?;
            evidence(&o.evidence)?;
        }
        self.version(id, &r.subject, "delivers-version", depth)?;
        let subject = self
            .state
            .versions
            .get(&format!(
                "{}@{}",
                r.subject.logical_id, r.subject.version_id
            ))
            .filter(|v| v.manifest_sha256 == r.subject.manifest_sha256)
            .cloned();
        for o in &r.outputs {
            self.reference_asset(
                id,
                &o.asset,
                "delivers-asset",
                &format!("outputs/{}/{}", o.role, o.language),
            )?;
            if let Some(v) = &subject
                && !v.payloads.contains(&o.asset.sha256)
            {
                self.finding(
                    id,
                    "delivery-output-outside-version",
                    format!(
                        "Output {}/{} is not retained by the exact subject version",
                        o.role, o.language
                    ),
                );
            }
            for d in &o.evidence {
                self.reference_asset(
                    id,
                    d,
                    "delivery-evidence",
                    &format!("outputs/{}/{}/evidence", o.role, o.language),
                )?;
            }
            if o.evidence.is_empty() {
                self.finding(
                    id,
                    "missing-delivery-evidence",
                    format!("Output {}/{} has no receipt evidence", o.role, o.language),
                );
            }
        }
        let n = self.report.nodes.get_mut(id).unwrap();
        n.details["declared_audience"] = serde_json::to_value(r.audience)?;
        n.details["remote_delivery_verified"] = json!(false);
        n.details["release_authorized"] = json!(false);
        self.finding(id, "delivery-semantics-unverified", "Historical output/evidence references only; no destination access, remote readback, release permission, playback or interpretation of evidence");
        Ok(())
    }

    pub(super) fn check_authority_scopes(&mut self) {
        let edges: Vec<_> = self
            .report
            .edges
            .iter()
            .filter(|e| e.kind == "source-authority")
            .map(|e| (e.from.clone(), e.to.clone()))
            .collect();
        for (scene, authority) in edges {
            if self
                .report
                .nodes
                .get(&authority)
                .is_none_or(|n| n.status != "inspected-reference-projection")
            {
                continue;
            }
            let Some(value) = self.native_documents.get(&scene) else {
                continue;
            };
            let scopes = [
                "source_scope_ids",
                "canonical_cue_ids",
                "presentation_source_scope_ids",
            ]
            .into_iter()
            .flat_map(|k| value[k].as_array().into_iter().flatten())
            .filter_map(Value::as_str)
            .collect::<BTreeSet<_>>();
            let declared = self.report.nodes[&authority].details["declared_scope_ids"].as_array();
            let missing: Vec<_> = scopes
                .into_iter()
                .filter(|s| declared.is_none_or(|ds| !ds.contains(&json!(s))))
                .map(str::to_owned)
                .collect();
            if !missing.is_empty() {
                self.finding(
                    &scene,
                    "source-scope-outside-authority",
                    format!(
                        "Source authority does not declare scopes: {}",
                        missing.join(", ")
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{descriptor, fixture, profile, write};
    use super::*;

    fn records(f: &crate::tiering::tests::Fixture, media: &str) {
        let subject = json!({"logical_id":"episode/demo","version_id":"v1","manifest_sha256":f.state.versions["episode/demo@v1"].manifest_sha256});
        // Deliberately use a supported-looking JSON object as opaque evidence.
        let d = descriptor(media, 15);
        write(
            f,
            "authority.json",
            &json!({"schema":AUTHORITY,"authority_id":"source-one","revisions":[{"revision_id":"original","document":d,"scope_ids":["cue-one","display-one"],"evidence":[d]}]}),
        );
        write(
            f,
            "delivery.json",
            &json!({"schema":DELIVERY,"delivery_id":"private-film","subject":subject,"audience":"private","outputs":[{"role":"movie","language":"es","asset":d,"evidence":[d]}]}),
        );
        profile(f, json!(["authority.json", "delivery.json"]));
    }
    fn edit(f: &crate::tiering::tests::Fixture, name: &str, change: impl FnOnce(&mut Value)) {
        let mut v: Value = serde_json::from_slice(&fs::read(f.root().join(name)).unwrap()).unwrap();
        change(&mut v);
        write(f, name, &v);
    }
    fn report(f: &crate::tiering::tests::Fixture) -> Report {
        inspect(f.root(), "inspect.json").unwrap()
    }
    fn code(r: &Report, name: &str) -> bool {
        r.findings.iter().any(|f| f.code == name)
    }

    #[test]
    fn exact_subject_source_and_evidence_are_distinct_from_build_dependencies_and_approval() {
        let (f, media, _) = fixture();
        records(&f, &media);
        let r = report(&f);
        for kind in [
            "source-document",
            "authority-evidence",
            "delivers-version",
            "delivers-asset",
            "delivery-evidence",
        ] {
            assert!(r.edges.iter().any(|e| e.kind == kind), "{kind}");
        }
        assert!(
            !r.edges
                .iter()
                .any(|e| e.from == "repo:delivery.json" && e.kind == "requires-version")
        );
        assert!(
            code(&r, "authority-semantics-unverified") && code(&r, "delivery-semantics-unverified")
        );
        assert!(!code(&r, "delivery-output-outside-version"));
        assert_eq!(
            r.nodes["repo:delivery.json"].details["remote_delivery_verified"],
            false
        );
        assert_eq!(
            r.nodes["repo:delivery.json"].details["release_authorized"],
            false
        );
        assert_eq!(
            r.nodes["repo:authority.json"].details["authority_verified"],
            false
        );
        assert!(!f.root().join("offline-cold").exists());
    }
    #[test]
    fn delivery_checks_exact_version_hash_and_payload_membership() {
        let (mut f, media, _) = fixture();
        records(&f, &media);
        let other = f.put(b"different media", true);
        f.save();
        edit(&f, "delivery.json", |v| {
            v["outputs"][0]["asset"] = descriptor(&other, 15)
        });
        assert!(code(&report(&f), "delivery-output-outside-version"));
        edit(&f, "delivery.json", |v| {
            v["subject"]["manifest_sha256"] = json!("a".repeat(64))
        });
        assert!(code(&report(&f), "version-manifest-mismatch"));
        edit(&f, "delivery.json", |v| {
            v["subject"]["version_id"] = json!("missing")
        });
        assert!(code(&report(&f), "missing-version"));
    }
    #[test]
    fn strict_contracts_reject_ambiguous_scopes_outputs_and_unknown_claims() {
        let (f, media, _) = fixture();
        for (file, case) in [
            ("authority.json", 0),
            ("authority.json", 1),
            ("delivery.json", 2),
            ("delivery.json", 3),
            ("delivery.json", 4),
        ] {
            records(&f, &media);
            edit(&f, file, |v| match case {
                0 => v["revisions"][0]["scope_ids"] = json!(["cue-one", "cue-one"]),
                1 => v["revisions"][0]["document"]["bytes"] = json!(0),
                2 => {
                    let o = v["outputs"][0].clone();
                    v["outputs"].as_array_mut().unwrap().push(o);
                }
                3 => v["approved"] = json!(true),
                _ => v["audience"] = json!("anything"),
            });
            let r = report(&f);
            assert!(code(&r, "invalid-contract"), "case {case}");
            assert!(
                !r.edges.iter().any(|e| e.from == format!("repo:{file}")),
                "no partial relationships on malformed record"
            );
        }
    }
    #[test]
    fn missing_evidence_and_missing_media_remain_holds_even_for_public_declarations() {
        let (f, media, _) = fixture();
        records(&f, &media);
        edit(&f, "authority.json", |v| {
            v["revisions"][0]["evidence"] = json!([])
        });
        edit(&f, "delivery.json", |v| {
            v["outputs"][0]["evidence"] = json!([]);
            v["audience"] = json!("public");
        });
        fs::remove_file(object(&f.root().join("fast"), &media)).unwrap();
        fs::remove_file(object(&f.root().join("history"), &media)).unwrap();
        let r = report(&f);
        for c in [
            "missing-authority-evidence",
            "missing-delivery-evidence",
            "missing-active-copy",
            "delivery-semantics-unverified",
        ] {
            assert!(code(&r, c), "{c}");
        }
        assert_eq!(
            r.nodes["repo:delivery.json"].details["release_authorized"],
            false
        );
    }
    #[test]
    fn source_binding_requires_exact_identity_schema_and_declared_scopes() {
        let (f, media, _) = fixture();
        records(&f, &media);
        write(
            &f,
            "scene.json",
            &json!({"schema":"reel.scene-authoring.v2","scene_id":"one","episode_id":"demo","authoring_state":"imported-evidence","source_authority_id":"source-one","canonical_cue_ids":["cue-one"],"presentation_source_scope_ids":["display-one"],"languages":{}}),
        );
        write(
            &f,
            "inspect.json",
            &json!({"schema":PROFILE,"project_id":"synthetic","roots":["scene.json"],"documents":{"source-one":"authority.json"}}),
        );
        let r = report(&f);
        assert!(!code(&r, "source-scope-outside-authority"));
        assert!(r.edges.iter().any(|e| e.kind == "source-authority"));
        edit(&f, "scene.json", |v| {
            v["canonical_cue_ids"] = json!(["unknown"])
        });
        assert!(code(&report(&f), "source-scope-outside-authority"));
        edit(&f, "authority.json", |v| {
            v["authority_id"] = json!("another")
        });
        assert!(code(&report(&f), "wrong-logical-identity"));
        edit(&f, "authority.json", |v| v["schema"] = json!(DELIVERY));
        assert!(code(&report(&f), "wrong-target-schema"));
    }
    #[test]
    fn evidence_json_is_never_opened_or_interpreted_and_bounds_are_enforced() {
        let (mut f, media, _) = fixture();
        records(&f, &media);
        let bytes = br#"{"schema":"reel.delivery-record.v1","secret":"opaque evidence"}"#;
        let hash = f.put(bytes, true);
        f.save();
        edit(&f, "delivery.json", |v| {
            v["outputs"][0]["evidence"] = json!([descriptor(&hash, bytes.len() as u64)])
        });
        let r = report(&f);
        assert_eq!(r.nodes[&format!("cache://sha256/{hash}")].kind, "asset");
        assert!(
            !r.nodes
                .values()
                .any(|n| n.schema.as_deref() == Some("opaque evidence"))
        );
        edit(&f, "delivery.json", |v| {
            v["outputs"][0]["evidence"] = json!(vec![descriptor(&hash, bytes.len() as u64); 129])
        });
        assert!(code(&report(&f), "invalid-contract"));
        records(&f, &media);
        edit(&f, "inspect.json", |v| v["limits"] = json!({"edges":1}));
        assert!(inspect(f.root(), "inspect.json").is_err());
    }
}
