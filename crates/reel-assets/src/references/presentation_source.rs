//! Opt-in native selected source-text checks; never parse media or infer consent.
use super::*;
use reel_assembly::{scene_authoring as native, template_presentation as text};

impl Inspector<'_> {
    fn selected_source_text(
        &mut self,
        context: &str,
        key: &str,
        a: &native::AssetRef,
    ) -> Result<text::PresentationSourceText> {
        let id = self.asset(
            context,
            &Asset {
                sha256: a.sha256.clone(),
                bytes: a.bytes,
                cache_uri: a.cache_uri.clone(),
                extra: BTreeMap::new(),
            },
            "selected-source-text",
            key,
        )?;
        if self.registered.get(&a.sha256) != Some(&a.bytes) {
            return Err(
                "Selected source-text descriptor is not authorized with its exact byte count"
                    .into(),
            );
        }
        if !self.native_documents.contains_key(&id) {
            if !self.seen.contains(&id) && self.seen.len() >= self.project.limits.documents {
                return Err(Budget("Document count budget exceeded").into());
            }
            self.seen.insert(id.clone());
            let bytes = self.read_document(&id)?;
            let value = parse(&bytes)?;
            let source: text::PresentationSourceText = serde_json::from_value(value.clone())?;
            if !matches!(
                source.schema.as_str(),
                text::SOURCE_TEXT_SCHEMA | text::SCENE_SOURCE_TEXT_SCHEMA_V2
            ) {
                return Err("Selected source-text has the wrong native schema".into());
            }
            let n = self.report.nodes.get_mut(&id).unwrap();
            n.kind = "asset-document".into();
            n.schema = Some(source.schema);
            n.status = "inspected-reference-projection".into();
            n.details["metadata_sha256"] = json!(tiering::digest(&bytes));
            n.details["verification"] = json!(
                "Selected source-text metadata freshly hash/size verified; original source document and audio not read"
            );
            self.native_documents.insert(id.clone(), value);
        }
        Ok(serde_json::from_value(self.native_documents[&id].clone())?)
    }

    fn check_source_language(
        &mut self,
        id: &str,
        resolved: &native::ResolvedScene,
        scene: &native::Scene,
        kind: &str,
        language: &str,
    ) -> Result<Value> {
        let presentation = scene
            .presentation
            .as_ref()
            .ok_or("Scene has no editable presentation")?;
        let key = presentation.content["source_text_bindings"][language]
            .as_str()
            .ok_or("Language-local selected source-text binding is missing")?;
        let a = resolved
            .selected_inputs
            .get(key)
            .ok_or("Source-text binding is not a native selected input")?;
        let source = self.selected_source_text(id, key, a)?;
        let invocation =
            text::scene_invocation(scene, language, kind).map_err(|e| e.to_string())?;
        let scope = text::scene_presentation_scope(scene, kind).map_err(|e| e.to_string())?;
        text::verify_source_text(
            &invocation,
            &source,
            &scene.source_authority_id,
            &scope,
            &scene.presentation_source_scope_ids,
        )
        .map_err(|e| e.to_string())?;
        text::verify_native_line_ownership(&invocation, &scene.languages[language].cues)
            .map_err(|e| e.to_string())?;
        if let Some(states) = presentation.content.get("text_state_by_language")
            && states[language] != source.text_state
        {
            return Err(
                "Selected source-text state differs from authored language text state".into(),
            );
        }
        let authority = self
            .project
            .documents
            .get(&source.source_authority_id)
            .ok_or("Source authority document is not explicitly bound")?;
        let authority_id = authority.id()?;
        let n = self
            .report
            .nodes
            .get(&authority_id)
            .ok_or("Source authority document was not inspected")?;
        if n.status != "inspected-reference-projection"
            || n.schema.as_deref() != Some(authority_delivery::AUTHORITY)
        {
            return Err("Source authority document is held or has the wrong schema".into());
        }
        let authority: authority_delivery::SourceAuthority = serde_json::from_value(
            self.native_documents
                .get(&authority_id)
                .ok_or("Source authority metadata is unavailable")?
                .clone(),
        )?;
        if authority.authority_id != source.source_authority_id
            || !authority.revisions.iter().any(|r| {
                r.document.sha256 == source.source_document_sha256
                    && source
                        .source_scope_ids
                        .iter()
                        .all(|s| r.scope_ids.contains(s))
            })
        {
            return Err(
                "Source-text document hash/scopes do not match a declared authority revision"
                    .into(),
            );
        }
        // Only identities/counts/statuses enter the report, never title/byline/line text.
        Ok(
            json!({"valid":true,"source_asset":a.cache_uri,"source_document_sha256":source.source_document_sha256,
            "source_authority_node":authority_id,"text_state":source.text_state,"source_scope_count":scope.len(),
            "line_count":invocation.lines.len(),"wording_and_stanza_match":true,"native_line_ownership_valid":true,
            "original_source_content_verified":false,"audio_content_verified":false,"human_approval_inferred":false}),
        )
    }

    pub(super) fn check_presentation_sources(
        &mut self,
        id: &str,
        c: &SceneContext,
        resolved: &native::ResolvedScene,
    ) -> Result<()> {
        let result = (|| -> Result<(native::Scene, String)> {
            let s: native::Scene = self.native_input(&c.scene, native::SCENE_SCHEMA_V2)?;
            let scene = native::materialize_scene(&s).map_err(|e| e.to_string())?;
            let presentation = scene
                .presentation
                .as_ref()
                .ok_or("Source checks require an editable presentation")?;
            let templates: native::TemplateCatalog =
                self.native_input(&c.templates, native::CATALOG_SCHEMA)?;
            let kind = templates
                .templates
                .iter()
                .find(|t| t.template_id == presentation.template_id)
                .ok_or("Presentation template is missing")?
                .kind
                .clone();
            Ok((scene, kind))
        })();
        let mut checks = BTreeMap::new();
        let mut valid = true;
        match result {
            Err(e) => {
                valid = false;
                self.finding(id, "presentation-source-check-failed", e.to_string());
            }
            Ok((scene, kind)) => {
                for language in scene.languages.keys() {
                    match self.check_source_language(id, resolved, &scene, &kind, language) {
                        Ok(value) => {
                            checks.insert(language.clone(), value);
                        }
                        Err(e) => {
                            if e.is::<Budget>() {
                                return Err(e);
                            }
                            valid = false;
                            self.finding(
                                id,
                                "presentation-source-check-failed",
                                format!("{language}: {e}"),
                            );
                            checks.insert(language.clone(), json!({"valid":false}));
                        }
                    }
                }
            }
        }
        let n = self.report.nodes.get_mut(id).unwrap();
        n.details["presentation_source_checks"] = json!(checks);
        n.details["presentation_source_valid"] = json!(valid);
        n.details["presentation_source_check_scope"] = json!(
            "Native authored wording/stanza/display/cue relationships and declared authority document hash only; no original manuscript content, audio, alignment timing, template definition, consent or approval verification"
        );
        if !valid {
            n.status = "source-held".into();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiering::tests::Fixture;
    fn write(f: &Fixture, name: &str, value: &Value) {
        fs::write(f.root().join(name), serde_json::to_vec(value).unwrap()).unwrap();
    }
    fn edit(f: &Fixture, name: &str, change: impl FnOnce(&mut Value)) {
        let mut v = parse(&fs::read(f.root().join(name)).unwrap()).unwrap();
        change(&mut v);
        write(f, name, &v);
    }
    fn fixture(change: impl Fn(&str, &mut Value)) -> (Fixture, String) {
        let (f, mut profile, _, _) = super::super::scene_context::tests::fixture();
        let document = f.put(b"synthetic original source", true);
        let mut source_hash = String::new();
        for lang in ["es", "en"] {
            let line = json!({"text":format!("private synthetic wording {lang}"),"cue_id":"cue-1","semantic_trigger_id":"first-word","stanza_break_before":false});
            edit(&f, "scene.json", |v| {
                v["presentation"]["content"]["titles"][lang] = json!(format!("title {lang}"));
                v["presentation"]["content"]["lines_by_language"][lang] = json!([line]);
                v["presentation"]["content"]["source_text_bindings"][lang] =
                    json!(format!("source.{lang}"));
                v["presentation"]["content"]["text_state_by_language"][lang] =
                    json!("project-draft-review-held");
            });
            let mut source = json!({"schema":text::SOURCE_TEXT_SCHEMA,"source_authority_id":"synthetic-source","source_document_sha256":document,"source_scope_ids":["cue-1"],"language":lang,"text_state":"project-draft-review-held","title":format!("title {lang}"),"chapter_number":null,"lines":[{"text":format!("private synthetic wording {lang}"),"cue_id":"cue-1","stanza_break_before":false}]});
            change(lang, &mut source);
            let bytes = serde_json::to_vec(&source).unwrap();
            let sha = f.put(&bytes, true);
            if lang == "es" {
                source_hash = sha.clone();
            }
            edit(&f, "scene-bindings.json", |v| {
                v["assets"][format!("source.{lang}")] = json!({"sha256":sha,"bytes":bytes.len(),"cache_uri":format!("cache://sha256/{sha}"),"logical_id":format!("selected-source.{lang}"),"selection_state":"selected-private-production"})
            });
        }
        write(
            &f,
            "authority.json",
            &json!({"schema":authority_delivery::AUTHORITY,"authority_id":"synthetic-source","revisions":[{"revision_id":"original","document":{"sha256":document,"bytes":25,"cache_uri":format!("cache://sha256/{document}")},"scope_ids":["cue-1"],"evidence":[]}]}),
        );
        // Correct document byte size independently of the source-text metadata.
        edit(&f, "authority.json", |v| {
            v["revisions"][0]["document"]["bytes"] = json!(b"synthetic original source".len())
        });
        profile["documents"]["synthetic-source"] = json!("authority.json");
        profile["scene_contexts"][0]["verify_presentation_sources"] = json!(true);
        write(&f, "inspect.json", &profile);
        (f, source_hash)
    }
    fn report(f: &Fixture) -> Report {
        inspect(f.root(), "inspect.json").unwrap()
    }
    fn node(r: &Report) -> &Node {
        &r.nodes["scene-context:bilingual-scene"]
    }
    #[test]
    fn selected_native_source_checks_exact_wording_and_ownership_without_disclosing_text() {
        let (f, sha) = fixture(|_, _| {});
        let r = report(&f);
        assert_eq!(
            node(&r).details["presentation_source_valid"],
            true,
            "{}",
            serde_json::to_string(&r.findings).unwrap()
        );
        assert_eq!(
            node(&r).details["presentation_source_checks"]["en"]["text_state"],
            "project-draft-review-held"
        );
        assert_eq!(
            node(&r).details["presentation_source_checks"]["es"]["human_approval_inferred"],
            false
        );
        assert_eq!(
            r.nodes[&format!("cache://sha256/{sha}")].kind,
            "asset-document"
        );
        assert!(r.edges.iter().any(|e| e.kind == "selected-source-text"));
        assert!(
            !serde_json::to_string(&r)
                .unwrap()
                .contains("private synthetic wording")
        );
    }
    #[test]
    fn native_wording_stanza_scope_authority_and_language_failures_hold_only_bad_lane() {
        for case in 0..7 {
            let (f, _) = fixture(|lang, v| {
                if lang == "es" {
                    match case {
                        0 => v["lines"][0]["text"] = json!("changed"),
                        1 => v["lines"][0]["stanza_break_before"] = json!(true),
                        2 => v["source_scope_ids"] = json!(["foreign"]),
                        3 => v["source_authority_id"] = json!("foreign"),
                        4 => v["language"] = json!("foreign"),
                        5 => v["source_document_sha256"] = json!("d".repeat(64)),
                        _ => v["text_state"] = json!("approved-translation"),
                    }
                }
            });
            let r = report(&f);
            assert_eq!(node(&r).status, "source-held", "case {case}");
            assert_eq!(node(&r).details["native_resolution_valid"], true);
            assert_eq!(
                node(&r).details["presentation_source_checks"]["en"]["valid"],
                true
            );
            assert_eq!(
                node(&r).details["presentation_source_checks"]["es"]["valid"],
                false
            );
        }
    }
    #[test]
    fn authored_lines_cannot_claim_an_unrelated_native_performance() {
        let (f, _) = fixture(|_, v| v["lines"][0]["audio_cue_id"] = json!("missing-performance"));
        edit(&f, "scene.json", |v| {
            for lang in ["es", "en"] {
                v["presentation"]["content"]["lines_by_language"][lang][0]["audio_cue_id"] =
                    json!("missing-performance");
            }
        });
        let r = report(&f);
        assert_eq!(node(&r).status, "source-held");
        assert!(
            r.findings
                .iter()
                .any(|f| f.code == "presentation-source-check-failed"
                    && f.detail.contains("performance missing"))
        );
    }
    #[test]
    fn first_tier_corruption_and_wrong_schema_do_not_hide_behind_presence() {
        let (f, sha) = fixture(|_, _| {});
        fs::write(object(&f.root().join("fast"), &sha), b"corrupt").unwrap();
        let r = report(&f);
        assert_eq!(node(&r).details["presentation_source_valid"], false);
        assert!(r.findings.iter().any(
            |f| f.code == "presentation-source-check-failed" && f.detail.contains("hash/size")
        ));
        let (f, _) = fixture(|_, v| v["schema"] = json!("unsupported.source"));
        assert_eq!(node(&report(&f)).status, "source-held");
    }
    #[test]
    fn selected_descriptor_bytes_and_v2_display_hashes_are_checked() {
        let (f, _) = fixture(|_, _| {});
        edit(&f, "scene-bindings.json", |v| {
            v["assets"]["source.es"]["bytes"] = json!(1)
        });
        let r = report(&f);
        assert_eq!(
            node(&r).details["presentation_source_checks"]["es"]["valid"],
            false
        );
        assert!(
            r.findings
                .iter()
                .any(|f| f.detail.contains("exact byte count"))
        );
        let (f, _) = fixture(|_, v| {
            v["schema"] = json!(text::SCENE_SOURCE_TEXT_SCHEMA_V2);
            v["source_scope_ids"] = json!(["cue-1", "title-scope", "credit-scope"]);
            v["byline"] = json!("synthetic credit");
            v["display_units"] = json!([
                {"source_scope_id":"title-scope","role":"poem-title","source_text":"original title","source_text_sha256":tiering::digest(b"original title"),"editable_text":v["title"],"editable_text_sha256":"a".repeat(64)},
                {"source_scope_id":"credit-scope","role":"poet-credit","source_text":"original credit","source_text_sha256":tiering::digest(b"original credit"),"editable_text":"synthetic credit","editable_text_sha256":tiering::digest(b"synthetic credit")}
            ]);
        });
        edit(&f, "scene.json", |v| {
            v["presentation_source_scope_ids"] = json!(["title-scope", "credit-scope"]);
            for lang in ["es", "en"] {
                v["presentation"]["content"]["bylines"][lang] = json!("synthetic credit");
            }
        });
        edit(&f, "authority.json", |v| {
            v["revisions"][0]["scope_ids"] = json!(["cue-1", "title-scope", "credit-scope"])
        });
        let r = report(&f);
        assert!(
            r.findings
                .iter()
                .any(|f| f.code == "presentation-source-check-failed"
                    && f.detail.contains("display text differ"))
        );
    }
    #[test]
    fn explicit_opt_in_budgets_and_snapshot_recheck_cover_selected_source_metadata() {
        let (f, sha) = fixture(|_, _| {});
        let error = inspect_with(f.root(), "inspect.json", || {
            fs::write(
                object(&f.root().join("fast"), &sha),
                b"changed after checks",
            )
            .unwrap();
        })
        .err()
        .unwrap()
        .to_string();
        assert!(error.contains("changed"), "{error}");
        let (f, sha) = fixture(|_, _| {});
        edit(&f, "inspect.json", |v| v["limits"] = json!({"documents":8}));
        assert!(inspect(f.root(), "inspect.json").is_err());
        edit(&f, "inspect.json", |v| {
            v.as_object_mut().unwrap().remove("limits");
            v["scene_contexts"][0]["verify_presentation_sources"] = json!(false);
        });
        fs::write(
            object(&f.root().join("fast"), &sha),
            vec![
                b'x';
                fs::metadata(object(&f.root().join("fast"), &sha))
                    .unwrap()
                    .len() as usize
            ],
        )
        .unwrap();
        let r = report(&f);
        assert_eq!(node(&r).status, "native-resolved");
        assert!(node(&r).details.get("presentation_source_valid").is_none());
    }
}
