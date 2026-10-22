//! Declared native clocks and semantic-event spans, not audio measurement.
use super::*;
use reel_assembly::scene_authoring as native;

impl Inspector<'_> {
    fn inspect_alignment_language(
        &mut self,
        id: &str,
        language: &str,
        lane: &native::Language,
        resolved: &native::ResolvedScene,
    ) -> Result<Value> {
        let mut alignments = BTreeMap::new();
        let mut cues = BTreeMap::new();
        let mut rate = None;
        let mut samples = 0u64;
        for cue in &lane.cues {
            if !cue.take_segments.is_empty() {
                return Err(
                    "Segmented cue clocks are not supported by this inspection check".into(),
                );
            }
            let key = cue
                .phrase_alignment_binding
                .as_deref()
                .ok_or("Cue has no selected native alignment")?;
            let take_key = cue
                .take_binding
                .as_deref()
                .ok_or("Cue has no single selected take")?;
            let a = resolved
                .selected_inputs
                .get(key)
                .ok_or("Alignment is not a native selected input")?;
            let take = resolved
                .selected_inputs
                .get(take_key)
                .ok_or("Take is not a native selected input")?;
            let value = self.selected_native_metadata(
                id,
                key,
                a,
                "selected-native-alignment",
                &[native::NATIVE_ALIGNMENT_SCHEMA],
            )?;
            let alignment: native::NativeAlignment = serde_json::from_value(value)?;
            native::validate_native_alignment(&alignment, language, &cue.cue_id, &take.sha256)
                .map_err(|e| e.to_string())?;
            if rate.is_some_and(|r| r != alignment.sample_rate) {
                return Err(
                    "Native lane has inconsistent sample rates; no clock conversion was requested"
                        .into(),
                );
            }
            rate = Some(alignment.sample_rate);
            samples = samples
                .checked_add(alignment.cue_end_sample)
                .ok_or("Native lane sample count overflow")?;
            self.asset(
                &a.cache_uri,
                &Asset {
                    sha256: take.sha256.clone(),
                    bytes: take.bytes,
                    cache_uri: take.cache_uri.clone(),
                    extra: BTreeMap::new(),
                },
                "aligned-take",
                take_key,
            )?;
            cues.insert(cue.cue_id.clone(),json!({"alignment_asset":a.cache_uri,"selected_take":take.cache_uri,"sample_rate":alignment.sample_rate,"cue_end_sample":alignment.cue_end_sample,"marker_count":alignment.semantic_markers.len()}));
            alignments.insert(cue.cue_id.clone(), alignment);
        }
        let spans = native::compile_native_event_spans(language, lane, &alignments)
            .map_err(|e| e.to_string())?;
        Ok(
            json!({"valid":true,"sample_rate":rate,"total_samples":samples,"cue_count":cues.len(),"cues":cues,"event_spans":spans,
            "audio_content_verified":false,"alignment_measurement_verified":false,"human_approval_inferred":false}),
        )
    }

    pub(super) fn check_native_alignments(
        &mut self,
        id: &str,
        c: &SceneContext,
        resolved: &native::ResolvedScene,
    ) -> Result<()> {
        let s: native::Scene = self.native_input(&c.scene, native::SCENE_SCHEMA_V2)?;
        let scene = native::materialize_scene(&s).map_err(|e| e.to_string())?;
        let mut checks = BTreeMap::new();
        let mut valid = true;
        for (language, lane) in &scene.languages {
            match self.inspect_alignment_language(id, language, lane, resolved) {
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
                        "native-alignment-check-failed",
                        format!("{language}: {e}"),
                    );
                    checks.insert(language.clone(), json!({"valid":false}));
                }
            }
        }
        let n = self.report.nodes.get_mut(id).unwrap();
        n.details["native_alignment_valid"] = json!(valid);
        n.details["native_alignment_checks"] = json!(checks);
        n.details["native_alignment_check_scope"] = json!(
            "Selected alignment JSON identity, single-take clocks, all marker bounds and native semantic-event spans only. Audio/measurement truth, presentation line timing, template layout, rendered timing and human approval remain unverified."
        );
        if !valid {
            n.status = if n.status == "source-held" {
                "checks-held"
            } else {
                "alignment-held"
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
    fn write(f: &Fixture, name: &str, v: &Value) {
        fs::write(f.root().join(name), serde_json::to_vec(v).unwrap()).unwrap();
    }
    fn edit(f: &Fixture, name: &str, change: impl FnOnce(&mut Value)) {
        let mut v = parse(&fs::read(f.root().join(name)).unwrap()).unwrap();
        change(&mut v);
        write(f, name, &v);
    }
    fn fixture(change: impl Fn(&str, &mut Value)) -> (Fixture, String) {
        let (f, mut p, _, _) = super::super::scene_context::tests::fixture();
        let bindings = parse(&fs::read(f.root().join("scene-bindings.json")).unwrap()).unwrap();
        let mut es = String::new();
        for lang in ["es", "en"] {
            let mut a = json!({"schema":native::NATIVE_ALIGNMENT_SCHEMA,"language":lang,"cue_id":"cue-1","selected_take_sha256":bindings["assets"][format!("take.{lang}")]["sha256"],"sample_rate":48000,"cue_end_sample":48000,"semantic_markers":{"entrance":0}});
            change(lang, &mut a);
            let bytes = serde_json::to_vec(&a).unwrap();
            let h = f.put(&bytes, true);
            if lang == "es" {
                es = h.clone();
            }
            edit(&f, "scene-bindings.json", |v| {
                v["assets"][format!("alignment.{lang}")] = json!({"sha256":h,"bytes":bytes.len(),"cache_uri":format!("cache://sha256/{h}"),"logical_id":format!("native-alignment.{lang}"),"selection_state":"selected-private-production"})
            });
        }
        p["scene_contexts"][0]["verify_native_alignments"] = json!(true);
        write(&f, "inspect.json", &p);
        (f, es)
    }
    fn inspect_fixture(f: &Fixture) -> Report {
        inspect(f.root(), "inspect.json").unwrap()
    }
    fn context(r: &Report) -> &Node {
        &r.nodes["scene-context:bilingual-scene"]
    }
    #[test]
    fn native_clock_and_event_spans_are_explicit_without_audio_or_measurement_claims() {
        let (f, h) = fixture(|_, _| {});
        let r = inspect_fixture(&f);
        let n = context(&r);
        assert_eq!(
            n.details["native_alignment_valid"],
            true,
            "{}",
            serde_json::to_string(&r.findings).unwrap()
        );
        for lang in ["es", "en"] {
            let c = &n.details["native_alignment_checks"][lang];
            assert_eq!(c["sample_rate"], 48000);
            assert_eq!(c["total_samples"], 48000);
            assert_eq!(c["event_spans"][0]["start_sample"], 0);
            assert_eq!(c["event_spans"][0]["end_sample"], 48000);
            assert_eq!(c["audio_content_verified"], false);
            assert_eq!(c["alignment_measurement_verified"], false);
        }
        assert_eq!(
            r.nodes[&format!("cache://sha256/{h}")].kind,
            "asset-document"
        );
        for kind in ["selected-native-alignment", "aligned-take"] {
            assert!(r.edges.iter().any(|e| e.kind == kind));
        }
        assert!(!f.root().join("offline-cold").exists());
    }
    #[test]
    fn identity_take_clock_and_all_marker_bounds_hold_the_bad_language() {
        for case in 0..8 {
            let (f, _) = fixture(|lang, v| {
                if lang == "es" {
                    match case {
                        0 => v["schema"] = json!("unsupported"),
                        1 => v["language"] = json!("other"),
                        2 => v["cue_id"] = json!("other"),
                        3 => v["selected_take_sha256"] = json!("a".repeat(64)),
                        4 => v["sample_rate"] = json!(0),
                        5 => v["cue_end_sample"] = json!(0),
                        6 => v["semantic_markers"]["unused"] = json!(48000),
                        _ => v["semantic_markers"][""] = json!(1),
                    }
                }
            });
            let r = inspect_fixture(&f);
            assert_eq!(context(&r).status, "alignment-held", "case {case}");
            assert_eq!(context(&r).details["native_resolution_valid"], true);
            assert_eq!(
                context(&r).details["native_alignment_checks"]["en"]["valid"],
                true
            );
            assert_eq!(
                context(&r).details["native_alignment_checks"]["es"]["valid"],
                false
            );
        }
    }
    #[test]
    fn native_span_compiler_rejects_missing_or_nonzero_first_event_markers() {
        for case in 0..2 {
            let (f, _) = fixture(|_, v| {
                v["semantic_markers"] = if case == 0 {
                    json!({})
                } else {
                    json!({"entrance":2})
                }
            });
            let r = inspect_fixture(&f);
            assert!(
                r.findings
                    .iter()
                    .any(|f| f.code == "native-alignment-check-failed"
                        && if case == 0 {
                            f.detail.contains("missing marker")
                        } else {
                            f.detail.contains("sample zero")
                        })
            );
        }
    }
    #[test]
    fn wrong_descriptor_bytes_and_corrupt_first_tier_fail_without_fallback() {
        let (f, _) = fixture(|_, _| {});
        edit(&f, "scene-bindings.json", |v| {
            v["assets"]["alignment.es"]["bytes"] = json!(1)
        });
        let r = inspect_fixture(&f);
        assert!(
            r.findings
                .iter()
                .any(|f| f.code == "native-alignment-check-failed"
                    && f.detail.contains("exact byte count"))
        );
        let (f, h) = fixture(|_, _| {});
        fs::write(object(&f.root().join("fast"), &h), b"corrupt").unwrap();
        let r = inspect_fixture(&f);
        assert!(
            r.findings.iter().any(
                |f| f.code == "native-alignment-check-failed" && f.detail.contains("hash/size")
            )
        );
        let (f, _) = fixture(|_, _| {});
        edit(
            &f,
            "scene.json",
            |v| v["languages"]["es"]["cues"][0]["take_segments"] = json!([{"segment_id":"part-one","speaker_id":"synthetic","asset_binding":"take.es","sample_count":48000}]),
        );
        let r = inspect_fixture(&f);
        assert_eq!(context(&r).details["native_resolution_valid"], true);
        assert!(
            r.findings
                .iter()
                .any(|f| f.code == "native-alignment-check-failed"
                    && f.detail.contains("Segmented cue clocks"))
        );
        let (f, _) = fixture(|_, v| v["selected_take_sha256"] = json!("a".repeat(64)));
        edit(&f, "inspect.json", |v| {
            v["scene_contexts"][0]["verify_presentation_sources"] = json!(true)
        });
        let r = inspect_fixture(&f);
        assert_eq!(context(&r).status, "checks-held");
    }
    fn second_cue(f: &Fixture, rate: u32, end: u64) {
        let bindings = parse(&fs::read(f.root().join("scene-bindings.json")).unwrap()).unwrap();
        for lang in ["es", "en"] {
            let a = json!({"schema":native::NATIVE_ALIGNMENT_SCHEMA,"language":lang,"cue_id":"cue-2","selected_take_sha256":bindings["assets"][format!("take.{lang}")]["sha256"],"sample_rate":rate,"cue_end_sample":end,"semantic_markers":{"next":0}});
            let bytes = serde_json::to_vec(&a).unwrap();
            let h = f.put(&bytes, true);
            edit(f, "scene-bindings.json", |v| {
                v["assets"][format!("alignment.second.{lang}")] = json!({"sha256":h,"bytes":bytes.len(),"cache_uri":format!("cache://sha256/{h}"),"logical_id":format!("native-alignment.second.{lang}"),"selection_state":"selected-private-production"})
            });
            edit(f, "scene.json", |v| {
                let mut cue = v["languages"][lang]["cues"][0].clone();
                cue["cue_id"] = json!("cue-2");
                cue["source_cue_ids"] = json!(["cue-2"]);
                cue["phrase_alignment_binding"] = json!(format!("alignment.second.{lang}"));
                v["languages"][lang]["cues"]
                    .as_array_mut()
                    .unwrap()
                    .push(cue);
                let mut b = v["language_event_bindings"][lang][0].clone();
                b["semantic_id"] = json!("departure");
                b["event_id"] = json!(format!("departure.{lang}"));
                b["cue_id"] = json!("cue-2");
                b["semantic_trigger_id"] = json!("next");
                v["language_event_bindings"][lang]
                    .as_array_mut()
                    .unwrap()
                    .push(b);
            });
        }
        edit(f, "scene.json", |v| {
            v["canonical_cue_ids"]
                .as_array_mut()
                .unwrap()
                .push(json!("cue-2"));
            let mut e = v["shared_events"][0].clone();
            e["semantic_id"] = json!("departure");
            e["canonical_cue_id"] = json!("cue-2");
            v["shared_events"].as_array_mut().unwrap().push(e);
        });
    }
    #[test]
    fn mixed_lane_rates_and_checked_sum_overflow_are_held_without_conversion() {
        let (f, _) = fixture(|_, _| {});
        second_cue(&f, 96000, 96000);
        let r = inspect_fixture(&f);
        assert!(
            r.findings
                .iter()
                .any(|f| f.code == "native-alignment-check-failed"
                    && f.detail.contains("inconsistent sample rates"))
        );
        let (f, _) = fixture(|_, v| v["cue_end_sample"] = json!(u64::MAX));
        second_cue(&f, 48000, u64::MAX);
        let r = inspect_fixture(&f);
        assert!(r.findings.iter().any(|f|f.code=="native-alignment-check-failed"&&f.detail.contains("overflow")));
    }
    #[test]
    fn explicit_opt_in_budgets_and_snapshot_checks_cover_selected_alignment_bytes() {
        let (f, h) = fixture(|_, _| {});
        assert!(
            inspect_with(f.root(), "inspect.json", || {
                fs::write(object(&f.root().join("fast"), &h), b"changed after checks").unwrap();
            })
            .is_err()
        );
        let (f, h) = fixture(|_, _| {});
        edit(&f, "inspect.json", |v| v["limits"] = json!({"documents":7}));
        assert!(inspect(f.root(), "inspect.json").is_err());
        edit(&f, "inspect.json", |v| {
            v.as_object_mut().unwrap().remove("limits");
            v["scene_contexts"][0]["verify_native_alignments"] = json!(false);
        });
        let p = object(&f.root().join("fast"), &h);
        let size = fs::metadata(&p).unwrap().len();
        fs::write(p, vec![b'x'; size as usize]).unwrap();
        let r = inspect_fixture(&f);
        assert_eq!(context(&r).status, "native-resolved");
        assert!(context(&r).details.get("native_alignment_valid").is_none());
    }

    #[test]
    fn selected_raw_json_uses_native_parser_before_duplicate_fields_can_collapse() {
        for bom in [false, true] {
            let (f, h) = fixture(|_, _| {});
            let bytes = fs::read(object(&f.root().join("fast"), &h)).unwrap();
            let bad = if bom {
                [b"\xef\xbb\xbf".as_slice(), &bytes].concat()
            } else {
                let mut b = br#"{"cue_id":"foreign", "#.to_vec();
                b.extend_from_slice(&bytes[1..]);
                b
            };
            let sha = f.put(&bad, true);
            edit(
                &f,
                "scene-bindings.json",
                |v| v["assets"]["alignment.es"] = json!({"sha256":sha,"bytes":bad.len(),"cache_uri":format!("cache://sha256/{sha}"),"logical_id":"raw-native-alignment","selection_state":"selected-private-production"}),
            );
            let r = inspect_fixture(&f);
            assert_eq!(
                context(&r).details["native_alignment_checks"]["es"]["valid"],
                false
            );
            if !bom {
                assert!(
                    r.findings
                        .iter()
                        .any(|f| f.code == "native-alignment-check-failed"
                            && f.detail.contains("duplicate field"))
                );
            }
        }
    }
}
