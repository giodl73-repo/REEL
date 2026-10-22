//! Explicit selected-template compilation, without rendering or exposing text/ASS.
use super::*;
use reel_assembly::{scene_authoring as native, template_presentation as text};

fn compile_budget(invocation: &text::EditableTextInvocation, cue_count: usize) -> Result<u64> {
    // Poem compilation may expand each line into several reading states.
    // Bound estimated text/event work before allocating ASS, independently
    // of the existing input-document/count/aggregate read budgets.
    let lines = invocation.lines.len() as u64;
    let text_bytes = invocation.lines.iter().try_fold(0u64, |n, l| {
        n.checked_add(l.text.len() as u64)
            .ok_or("Presentation text size overflow")
    })?;
    let work = text_bytes
        .checked_add(lines.checked_mul(512).ok_or("Presentation work overflow")?)
        .and_then(|n| n.checked_mul(lines.max(1)))
        .ok_or("Presentation work overflow")?;
    if lines > 128 || cue_count > 256 || work > 8 * 1024 * 1024 {
        return Err(Budget("Presentation compilation work budget exceeded").into());
    }
    Ok(work)
}

impl Inspector<'_> {
    fn presentation_template_language(
        &mut self,
        id: &str,
        language: &str,
        scene: &native::Scene,
        definition: &text::EditableTextTemplate,
        resolved: &native::ResolvedScene,
    ) -> Result<Value> {
        let details = &self.report.nodes[id].details;
        if details["presentation_source_checks"][language]["valid"] != true
            || details["native_alignment_checks"][language]["valid"] != true
        {
            return Err("Presentation clock requires valid selected source and alignment checks for this language".into());
        }
        let content = &scene
            .presentation
            .as_ref()
            .ok_or("Scene has no presentation")?
            .content;
        let key = content["source_text_bindings"][language]
            .as_str()
            .ok_or("Missing source-text binding")?;
        let source = self.selected_source_text(
            id,
            key,
            resolved
                .selected_inputs
                .get(key)
                .ok_or("Source text is not selected")?,
        )?;
        let invocation =
            text::scene_invocation(scene, language, &definition.kind).map_err(|e| e.to_string())?;
        let lines = invocation.lines.len() as u64;
        let work = compile_budget(&invocation, scene.languages[language].cues.len())?;
        let (_, alignments) =
            self.inspect_alignment_language(id, language, &scene.languages[language], resolved)?;
        let empty = BTreeMap::new();
        let clocks = if definition.kind == "poem-title" {
            &empty
        } else {
            &alignments
        };
        let (compiled, duration_samples, sample_rate) =
            text::compile_scene_layer(scene, definition, language, &source, clocks)
                .map_err(|e| e.to_string())?;
        Ok(
            json!({"valid":true,"line_count":lines,"estimated_compile_work_bytes":work,
            "layer_duration_samples":compiled.duration_samples,"layer_sample_rate":compiled.sample_rate,
            "scene_duration_samples":duration_samples,"scene_sample_rate":sample_rate,
            "editable_layer_sha256":tiering::digest(compiled.ass.as_bytes()),"editable_layer_bytes":compiled.ass.len(),
            "rendered_timing_verified":false,"visual_layout_verified":false,"audio_content_verified":false,"human_approval_inferred":false}),
        )
    }

    pub(super) fn check_presentation_template(
        &mut self,
        id: &str,
        c: &SceneContext,
        resolved: &native::ResolvedScene,
    ) -> Result<()> {
        let mut checks = BTreeMap::new();
        let result = (|| -> Result<()> {
            if !c.verify_presentation_sources || !c.verify_native_alignments {
                return Err("Presentation template inspection requires explicit source-text and native-alignment checks".into());
            }
            let s: native::Scene = self.native_input(&c.scene, native::SCENE_SCHEMA_V2)?;
            let scene = native::materialize_scene(&s).map_err(|e| e.to_string())?;
            let episode: native::Episode = self.native_input(&c.episode, native::EPISODE_SCHEMA)?;
            let catalog: native::TemplateCatalog =
                self.native_input(&c.templates, native::CATALOG_SCHEMA)?;
            let a = c
                .presentation_template
                .as_ref()
                .ok_or("Missing explicit template descriptor")?;
            let presentation = scene
                .presentation
                .as_ref()
                .ok_or("Scene has no editable presentation")?;
            if resolved.template_definitions.get(&presentation.template_id) != Some(&a.sha256) {
                return Err(
                    "Explicit presentation template hash differs from native catalog selection"
                        .into(),
                );
            }
            let value = self.selected_native_metadata(
                id,
                &presentation.template_id,
                &native::AssetRef {
                    logical_id: presentation.template_id.clone(),
                    sha256: a.sha256.clone(),
                    bytes: a.bytes,
                    cache_uri: a.cache_uri.clone(),
                    selection_state: "selected-private-production".into(),
                },
                "selected-presentation-template",
                &[text::TEMPLATE_SCHEMA],
            )?;
            let definition: text::EditableTextTemplate = serde_json::from_value(value)?;
            if let Some(score) =
                text::validate_scene_template(&scene, &episode, &catalog, &definition, &a.sha256)
                    .map_err(|e| e.to_string())?
                && !resolved.selected_inputs.contains_key(&score)
            {
                return Err("Required template soundtrack is not a native selected input".into());
            }
            for language in scene.languages.keys() {
                match self.presentation_template_language(
                    id,
                    language,
                    &scene,
                    &definition,
                    resolved,
                ) {
                    Ok(v) => {
                        checks.insert(language.clone(), v);
                    }
                    Err(e) => {
                        if e.is::<Budget>() {
                            return Err(e);
                        }
                        self.finding(
                            id,
                            "presentation-template-check-failed",
                            format!("{language}: {e}"),
                        );
                        checks.insert(language.clone(), json!({"valid":false}));
                    }
                }
            }
            Ok(())
        })();
        if let Err(e) = result {
            if e.is::<Budget>() {
                return Err(e);
            }
            self.finding(id, "presentation-template-check-failed", e.to_string());
            checks.insert("context".into(), json!({"valid":false}));
        }
        let valid = !checks.is_empty() && checks.values().all(|v| v["valid"] == true);
        let n = self.report.nodes.get_mut(id).unwrap();
        n.details["presentation_template_valid"] = json!(valid);
        n.details["presentation_template_checks"] = json!(checks);
        n.details["presentation_template_check_scope"] = json!(
            "Exact catalog-selected template metadata, shared content/soundtrack rules and in-memory native presentation compilation only. ASS/text are not included. No measured audio, rendered layout/timing or human approval."
        );
        if !valid {
            n.status = if matches!(
                n.status.as_str(),
                "source-held" | "alignment-held" | "checks-held"
            ) {
                "checks-held"
            } else {
                "template-held"
            }
            .into();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiering::tests::Fixture;
    fn edit(f: &Fixture, name: &str, change: impl FnOnce(&mut Value)) {
        let p = f.root().join(name);
        let mut v: Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
        change(&mut v);
        fs::write(p, serde_json::to_vec(&v).unwrap()).unwrap();
    }
    fn definition() -> Value {
        json!({"schema":text::TEMPLATE_SCHEMA,"template_id":"poem-template","kind":"opening-poem","canvas_width":1280,"canvas_height":720,"font_name":"Arial","title_size":42,"body_size":30,"title_x":880,"title_y":32,"body_x":880,"body_y":94,"line_spacing":42,"future_rgb":[120,120,120],"active_rgb":[200,200,200],"completed_rgb":[255,255,255],"panel":{"x":853,"width":427,"background_rgb":[20,18,16],"divider_rgb":[211,178,107],"divider_alpha":185},"fixed_duration_seconds":null})
    }
    fn bind_definition(f: &Fixture, bytes: &[u8]) -> String {
        let h = f.put(bytes, true);
        edit(
            f,
            "inspect.json",
            |v| {
                v["scene_contexts"][0]["presentation_template"] = json!({"sha256":h,"bytes":bytes.len(),"cache_uri":format!("cache://sha256/{h}")})
            },
        );
        edit(f, "templates.json", |v| {
            v["templates"][1]["definition_sha256"] = json!(h)
        });
        h
    }
    fn fixture() -> (Fixture, String) {
        let (f, _) = super::super::presentation_source::tests::fixture(|_, _| {});
        edit(&f, "scene.json", |v| {
            v["presentation"]["role"] = json!("opening-poem")
        });
        edit(&f, "templates.json", |v| {
            v["templates"][1]["kind"] = json!("opening-poem")
        });
        edit(&f, "inspect.json", |v| {
            v["scene_contexts"][0]["verify_native_alignments"] = json!(true)
        });
        for lang in ["es", "en"] {
            let bindings: Value =
                serde_json::from_slice(&fs::read(f.root().join("scene-bindings.json")).unwrap())
                    .unwrap();
            let a = json!({"schema":native::NATIVE_ALIGNMENT_SCHEMA,"language":lang,"cue_id":"cue-1","selected_take_sha256":bindings["assets"][format!("take.{lang}")]["sha256"],"sample_rate":48000,"cue_end_sample":48000,"semantic_markers":{"entrance":0,"first-word":0}});
            let bytes = serde_json::to_vec(&a).unwrap();
            let h = f.put(&bytes, true);
            edit(&f, "scene-bindings.json", |v| {
                v["assets"][format!("alignment.{lang}")] = json!({"logical_id":format!("alignment.{lang}"),"sha256":h,"bytes":bytes.len(),"cache_uri":format!("cache://sha256/{h}"),"selection_state":"selected-private-production"})
            });
        }
        let h = bind_definition(&f, &serde_json::to_vec(&definition()).unwrap());
        (f, h)
    }
    fn inspect_fixture(f: &Fixture) -> Report {
        inspect(f.root(), "inspect.json").unwrap()
    }
    fn context(r: &Report) -> &Node {
        &r.nodes["scene-context:bilingual-scene"]
    }
    #[test]
    fn compiles_selected_template_clocks_without_exposing_text_ass_or_claiming_render_acceptance() {
        let (f, h) = fixture();
        let r = inspect_fixture(&f);
        let n = context(&r);
        assert_eq!(
            n.details["presentation_template_valid"],
            true,
            "{:?}",
            serde_json::to_string(&r.findings)
        );
        for lang in ["es", "en"] {
            let c = &n.details["presentation_template_checks"][lang];
            assert_eq!(c["scene_duration_samples"], 48000);
            assert_eq!(c["scene_sample_rate"], 48000);
            assert_eq!(c["visual_layout_verified"], false);
            assert_eq!(c["rendered_timing_verified"], false);
            assert!(c["editable_layer_bytes"].as_u64().unwrap() > 0);
        }
        assert!(
            r.edges
                .iter()
                .any(|e| e.kind == "selected-presentation-template")
        );
        assert_eq!(
            r.nodes[&format!("cache://sha256/{h}")].kind,
            "asset-document"
        );
        let report = serde_json::to_string(&r).unwrap();
        assert!(!report.contains("private synthetic wording"));
        assert!(!report.contains("[Script Info]"));
        assert!(!f.root().join("offline-cold").exists());
    }
    #[test]
    fn missing_presentation_marker_holds_only_its_language_and_keeps_other_flags() {
        let (f, _) = fixture();
        edit(&f, "scene.json", |v| {
            v["presentation"]["content"]["lines_by_language"]["es"][0]["semantic_trigger_id"] =
                json!("missing")
        });
        let r = inspect_fixture(&f);
        let n = context(&r);
        assert_eq!(n.status, "template-held");
        assert_eq!(n.details["native_alignment_valid"], true);
        assert_eq!(n.details["presentation_source_valid"], true);
        assert_eq!(
            n.details["presentation_template_checks"]["es"]["valid"],
            false
        );
        assert_eq!(
            n.details["presentation_template_checks"]["en"]["valid"],
            true
        );
    }
    #[test]
    fn rejects_catalog_identity_style_required_content_and_soundtrack_disagreements() {
        for case in 0..6 {
            let (f, _) = fixture();
            let mut d = definition();
            match case {
                0 => {
                    d["template_id"] = json!("foreign");
                }
                1 => {
                    d["canvas_width"] = json!(0);
                }
                2 => edit(&f, "scene.json", |v| {
                    v["presentation"]["content"]["font"] = json!("injected")
                }),
                3 => edit(&f, "templates.json", |v| {
                    v["templates"][1]["required_content_keys"] = json!(["missing"])
                }),
                4 => {
                    d["soundtrack_requirement"] = json!("unknown");
                    edit(&f, "templates.json", |v| {
                        v["templates"][1]["soundtrack_requirement"] = json!("unknown")
                    });
                }
                _ => {}
            }
            bind_definition(&f, &serde_json::to_vec(&d).unwrap());
            if case == 5 {
                edit(&f, "inspect.json", |v| {
                    v["scene_contexts"][0]["presentation_template"]["sha256"] =
                        json!("f".repeat(64))
                });
            }
            let r = inspect_fixture(&f);
            assert!(
                context(&r).details["presentation_template_valid"] == false
                    || context(&r).details["native_resolution_valid"] == false,
                "case {case}"
            );
        }
    }
    #[test]
    fn exact_descriptor_first_tier_corruption_and_raw_native_parsing_are_enforced() {
        for case in 0..4 {
            let (f, h) = fixture();
            let path = object(&f.root().join("fast"), &h);
            match case {
                0 => {
                    let n = fs::metadata(&path).unwrap().len();
                    fs::write(path, vec![b'x'; n as usize]).unwrap();
                }
                1 => edit(&f, "inspect.json", |v| {
                    v["scene_contexts"][0]["presentation_template"]["bytes"] = json!(1)
                }),
                _ => {
                    let b = serde_json::to_vec(&definition()).unwrap();
                    let bad = if case == 2 {
                        [b"\xef\xbb\xbf".as_slice(), &b].concat()
                    } else {
                        [br#"{"kind":"foreign", "#.as_slice(), &b[1..]].concat()
                    };
                    bind_definition(&f, &bad);
                }
            }
            let r = inspect_fixture(&f);
            assert!(
                context(&r).details["presentation_template_valid"] == false
                    || context(&r).details["native_resolution_valid"] == false,
                "case {case}"
            );
        }
    }
    #[test]
    fn opt_in_is_explicit_and_prerequisite_holds_are_not_promoted_to_passes() {
        let (f, h) = fixture();
        edit(&f, "inspect.json", |v| {
            v["scene_contexts"][0]
                .as_object_mut()
                .unwrap()
                .remove("presentation_template");
        });
        fs::write(object(&f.root().join("fast"), &h), b"unread template").unwrap();
        let r = inspect_fixture(&f);
        assert!(
            context(&r)
                .details
                .get("presentation_template_valid")
                .is_none()
        );
        for case in 0..2 {
            let (f, _) = fixture();
            edit(&f, "inspect.json", |v| {
                v["scene_contexts"][0][if case == 0 {
                    "verify_presentation_sources"
                } else {
                    "verify_native_alignments"
                }] = json!(false)
            });
            let r = inspect_fixture(&f);
            assert_eq!(context(&r).details["presentation_template_valid"], false);
        }
        let (f, _) = fixture();
        edit(&f, "scene.json", |v| {
            v["presentation"]["content"]["lines_by_language"]["es"][0]["text"] = json!("changed")
        });
        let r = inspect_fixture(&f);
        assert_eq!(context(&r).status, "checks-held");
        assert_eq!(
            context(&r).details["presentation_template_checks"]["es"]["valid"],
            false
        );
        assert_eq!(
            context(&r).details["presentation_template_checks"]["en"]["valid"],
            true
        );
    }
    #[test]
    fn selected_template_snapshot_and_compile_work_budgets_remain_bounded() {
        let (f, h) = fixture();
        assert!(
            inspect_with(f.root(), "inspect.json", || {
                fs::write(
                    object(&f.root().join("fast"), &h),
                    b"changed after inspection",
                )
                .unwrap();
            })
            .is_err()
        );
        let mut invocation:text::EditableTextInvocation=serde_json::from_value(json!({"schema":text::INVOCATION_SCHEMA,"template_id":"poem-template","language":"es","title":"title","lines":[{"text":"source line","cue_id":"cue","semantic_trigger_id":"line"}]})).unwrap();
        assert!(compile_budget(&invocation, 1).is_ok());
        assert!(compile_budget(&invocation, 257).is_err());
        invocation.lines[0].text = "x".repeat(8 * 1024 * 1024);
        assert!(compile_budget(&invocation, 1).is_err());
        invocation.lines[0].text = "short line".into();
        invocation.lines = vec![invocation.lines[0].clone(); 129];
        assert!(compile_budget(&invocation, 1).is_err());
        let (f, _) = fixture();
        edit(&f, "inspect.json", |v| {
            v["limits"] = json!({"total_document_bytes":1})
        });
        assert!(inspect(f.root(), "inspect.json").is_err());
    }
}
