//! Native selected-binding resolution, with explicitly declared metadata inputs.
use super::*;
use reel_assembly::scene_authoring as native;

#[derive(Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CacheDocument {
    pub sha256: String,
    pub bytes: u64,
    pub cache_uri: String,
}

#[derive(Clone, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum DocumentSource {
    Repository(String),
    Cache(CacheDocument),
}
impl DocumentSource {
    pub(super) fn id(&self) -> Result<String> {
        match self {
            Self::Repository(path) => {
                if path.is_empty()
                    || path.contains('\\')
                    || Path::new(path)
                        .components()
                        .any(|p| !matches!(p, Component::Normal(_)))
                {
                    return Err("Invalid repository-relative metadata locator".into());
                }
                Ok(format!("repo:{path}"))
            }
            Self::Cache(a) => {
                if !valid_oid(&a.sha256) || a.cache_uri != format!("cache://sha256/{}", a.sha256) {
                    return Err(
                        "Cache metadata locator requires exact SHA-256, bytes and cache URI only"
                            .into(),
                    );
                }
                Ok(a.cache_uri.clone())
            }
        }
    }
}

/// Reuse the existing native resolver's seven inputs; do not invent scene semantics.
#[derive(Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SceneContext {
    pub id: String,
    pub scene: DocumentSource,
    pub episode: DocumentSource,
    pub templates: DocumentSource,
    pub policy: DocumentSource,
    pub season_bindings: DocumentSource,
    pub episode_bindings: DocumentSource,
    pub scene_bindings: DocumentSource,
    /// Explicitly open only native resolver-selected presentation source JSON.
    #[serde(default)]
    pub verify_presentation_sources: bool,
}
impl SceneContext {
    fn inputs(&self) -> [(&'static str, &DocumentSource, &'static str); 7] {
        [
            ("scene", &self.scene, native::SCENE_SCHEMA_V2),
            ("episode", &self.episode, native::EPISODE_SCHEMA),
            ("templates", &self.templates, native::CATALOG_SCHEMA),
            ("policy", &self.policy, native::POLICY_SCHEMA),
            (
                "season_bindings",
                &self.season_bindings,
                native::BINDINGS_SCHEMA,
            ),
            (
                "episode_bindings",
                &self.episode_bindings,
                native::BINDINGS_SCHEMA,
            ),
            (
                "scene_bindings",
                &self.scene_bindings,
                native::BINDINGS_SCHEMA,
            ),
        ]
    }
}

impl Inspector<'_> {
    pub(super) fn declared_document(
        &mut self,
        from: &str,
        source: &DocumentSource,
        kind: &str,
        field: &str,
        depth: usize,
        expected: Option<&str>,
    ) -> Result<String> {
        let id = source.id()?;
        match source {
            DocumentSource::Repository(path) => {
                self.document_ref(from, path, kind, field, depth, expected)?
            }
            DocumentSource::Cache(a) => {
                if self.registered.get(&a.sha256) != Some(&a.bytes) {
                    return Err(
                        "Cache metadata locator is not authorized with its exact byte count".into(),
                    );
                }
                self.asset(
                    from,
                    &Asset {
                        sha256: a.sha256.clone(),
                        bytes: a.bytes,
                        cache_uri: a.cache_uri.clone(),
                        extra: BTreeMap::new(),
                    },
                    kind,
                    field,
                )?;
                self.enqueue(id.clone(), depth + 1, expected, None)?;
            }
        }
        Ok(id)
    }
    pub(super) fn prepare_scene_contexts(&mut self) -> Result<()> {
        for context in self.project.scene_contexts.clone() {
            let id = format!("scene-context:{}", context.id);
            self.node(&id, "native-scene-context", "pending", json!({}))?;
            for (field, source, schema) in context.inputs() {
                self.declared_document(&id, source, "authoring-document", field, 0, Some(schema))?;
            }
        }
        Ok(())
    }
    pub(super) fn native_input<T: serde::de::DeserializeOwned>(
        &self,
        source: &DocumentSource,
        expected: &str,
    ) -> Result<T> {
        let id = source.id()?;
        let node = self
            .report
            .nodes
            .get(&id)
            .ok_or("Native context document is missing")?;
        if node.schema.as_deref() != Some(expected)
            || node.status != "inspected-reference-projection"
        {
            return Err(format!(
                "Native context target {id} has the wrong schema or failed metadata inspection"
            )
            .into());
        }
        let value = self
            .native_documents
            .get(&id)
            .ok_or("Native context target was not inspected")?;
        Ok(serde_json::from_value(value.clone())?)
    }
    fn resolve_context(&self, c: &SceneContext) -> Result<native::ResolvedScene> {
        let scene = self.native_input(&c.scene, native::SCENE_SCHEMA_V2)?;
        let episode = self.native_input(&c.episode, native::EPISODE_SCHEMA)?;
        let templates = self.native_input(&c.templates, native::CATALOG_SCHEMA)?;
        let policy = self.native_input(&c.policy, native::POLICY_SCHEMA)?;
        let season = self.native_input(&c.season_bindings, native::BINDINGS_SCHEMA)?;
        let episode_bindings = self.native_input(&c.episode_bindings, native::BINDINGS_SCHEMA)?;
        let scene_bindings = self.native_input(&c.scene_bindings, native::BINDINGS_SCHEMA)?;
        native::resolve_scene(
            &templates,
            &episode,
            &scene,
            &policy,
            &season,
            &episode_bindings,
            &scene_bindings,
        )
        .map_err(|e| e.to_string().into())
    }
    pub(super) fn resolve_scene_contexts(&mut self) -> Result<()> {
        for context in self.project.scene_contexts.clone() {
            let id = format!("scene-context:{}", context.id);
            match self.resolve_context(&context) {
                Err(e) => {
                    self.report.nodes.get_mut(&id).unwrap().status = "held".into();
                    self.report.nodes.get_mut(&id).unwrap().details = json!({"native_resolution_valid":false,"scope":"Symbolic native resolution only; no render, source-text verification or human approval"});
                    self.finding(&id, "native-scene-resolution-failed", e.to_string());
                }
                Ok(resolved) => {
                    let mut selected = BTreeMap::new();
                    let mut available = true;
                    for (key, asset) in &resolved.selected_inputs {
                        let descriptor = Asset {
                            sha256: asset.sha256.clone(),
                            bytes: asset.bytes,
                            cache_uri: asset.cache_uri.clone(),
                            extra: BTreeMap::new(),
                        };
                        let target = self.asset(
                            &id,
                            &descriptor,
                            "requires-asset",
                            &format!("selected_inputs/{key}"),
                        )?;
                        available &= matches!(
                            self.report.nodes[&target].status.as_str(),
                            "present-size-only" | "inspected-reference-projection"
                        );
                        selected.insert(key.clone(),json!({"asset":target,"logical_id":asset.logical_id,"selection_state":asset.selection_state}));
                    }
                    let mut documents = BTreeMap::new();
                    for (field, source, _) in context.inputs() {
                        let target = source.id()?;
                        documents.insert(field,json!({"node":target,"metadata_sha256":self.report.nodes[&target].details["metadata_sha256"]}));
                    }
                    let node = self.report.nodes.get_mut(&id).unwrap();
                    node.status = if available {
                        "native-resolved"
                    } else {
                        "inputs-held"
                    }
                    .into();
                    node.details = json!({"native_resolution_valid":true,"input_presence_valid":available,"scene_id":resolved.scene_id,"scene_policy_id":resolved.scene_policy_id,"fingerprint":resolved.fingerprint_sha256,"language_fingerprints":resolved.language_fingerprints,"template_definitions":resolved.template_definitions,"selected_bindings":selected,"documents":documents,"scope":"Native symbolic selected-binding resolution. Media presence is size-only; opt-in selected source-text checks are reported separately. No original-manuscript/audio-content verification, render/build certificate or human approval."});
                    if !available {
                        self.finding(
                            &id,
                            "native-scene-inputs-held",
                            "Native bindings resolve, but required asset presence is not valid",
                        );
                    }
                    if context.verify_presentation_sources {
                        self.check_presentation_sources(&id, &context, &resolved)?;
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::tiering::tests::Fixture;
    fn write(f: &Fixture, name: &str, value: &Value) {
        fs::write(f.root().join(name), serde_json::to_vec(value).unwrap()).unwrap();
    }
    fn cached(f: &Fixture, value: &Value) -> Value {
        let bytes = serde_json::to_vec(value).unwrap();
        let sha = f.put(&bytes, true);
        json!({"sha256":sha,"bytes":bytes.len(),"cache_uri":format!("cache://sha256/{sha}")})
    }
    pub(in crate::references) fn fixture() -> (Fixture, Value, String, String) {
        let f = Fixture::new();
        let mut assets = serde_json::Map::new();
        for key in [
            "picture.main",
            "take.es",
            "alignment.es",
            "take.en",
            "alignment.en",
            "sonic.hit",
            "vfx.dust",
            "music.theme",
            "unused",
        ] {
            let sha = f.put(key.as_bytes(), true);
            assets.insert(key.into(),json!({"logical_id":key,"sha256":sha,"bytes":key.len(),"cache_uri":format!("cache://sha256/{sha}"),"selection_state":"selected-private-production"}));
        }
        let required = assets["picture.main"]["sha256"]
            .as_str()
            .unwrap()
            .to_owned();
        let unused = assets["unused"]["sha256"].as_str().unwrap().to_owned();
        let mut languages = serde_json::Map::new();
        let mut bindings = serde_json::Map::new();
        for lang in ["es", "en"] {
            languages.insert(lang.into(),json!({"cues":[{"cue_id":"cue-1","source_id":"source-1","source_cue_ids":["cue-1"],"exact_text_sha256":tiering::digest(lang.as_bytes()),"narration_slot_id":format!("narration.{lang}"),"take_binding":format!("take.{lang}"),"phrase_alignment_binding":format!("alignment.{lang}")}]}));
            bindings.insert(lang.into(),json!([{"semantic_id":"arrival","event_id":format!("arrival.{lang}"),"cue_id":"cue-1","semantic_trigger_id":"entrance","picture_slot_id":"picture-1"}]));
        }
        write(
            &f,
            "scene.json",
            &json!({"schema":native::SCENE_SCHEMA_V2,"scene_id":"scene-one","episode_id":"pilot","authoring_state":"ready-for-private-build","canonical_cue_ids":["cue-1"],"source_authority_id":"synthetic-source","languages":languages,"shared_events":[{"semantic_id":"arrival","canonical_cue_id":"cue-1","picture_binding":"picture.main","score":{"disposition":"role","role":"theme"},"sonic_bindings":["sonic.hit"],"vfx_bindings":["vfx.dust"]}],"language_event_bindings":bindings,"presentation":{"role":"poem","template_id":"poem-template","content":{}}}),
        );
        write(
            &f,
            "episode.json",
            &json!({"schema":native::EPISODE_SCHEMA,"episode_id":"pilot","season_id":"season-one","authoring_state":"scene-build-context","master_template_id":"master","scene_policy_id":"policy","presentation":[],"score_palette":[{"role":"theme","theme_id":"theme-one","source_poem_id":"synthetic-poem","arrangement_id":"arrangement-one","asset_binding":"music.theme"}],"scene_ids":["scene-one"]}),
        );
        write(
            &f,
            "templates.json",
            &json!({"schema":native::CATALOG_SCHEMA,"templates":[{"template_id":"master","kind":"master","definition_sha256":"a".repeat(64),"required_content_keys":[]},{"template_id":"poem-template","kind":"poem","definition_sha256":"b".repeat(64),"required_content_keys":[]}]}),
        );
        write(
            &f,
            "policy.json",
            &json!({"schema":native::POLICY_SCHEMA,"policy_id":"policy","target_composition_seconds_min":5.0,"target_composition_seconds_max":10.0,"hard_unchanged_composition_seconds_max":10.0,"semantic_cuts_required":true}),
        );
        write(
            &f,
            "season-bindings.json",
            &json!({"schema":native::BINDINGS_SCHEMA,"scope_id":"season-one","assets":{}}),
        );
        write(
            &f,
            "episode-bindings.json",
            &json!({"schema":native::BINDINGS_SCHEMA,"scope_id":"pilot","assets":{}}),
        );
        write(
            &f,
            "scene-bindings.json",
            &json!({"schema":native::BINDINGS_SCHEMA,"scope_id":"scene-one","assets":assets}),
        );
        let profile = json!({"schema":PROFILE,"project_id":"synthetic","roots":["scene.json"],"documents":{"pilot":"episode.json","scene-one":"scene.json","master":"templates.json","poem-template":"templates.json","policy":"policy.json"},"scene_contexts":[{"id":"bilingual-scene","scene":"scene.json","episode":"episode.json","templates":"templates.json","policy":"policy.json","season_bindings":"season-bindings.json","episode_bindings":"episode-bindings.json","scene_bindings":"scene-bindings.json"}]});
        write(&f, "inspect.json", &profile);
        (f, profile, required, unused)
    }
    fn inspected(f: &Fixture) -> Report {
        inspect(f.root(), "inspect.json").unwrap()
    }
    fn context(r: &Report) -> &Node {
        &r.nodes["scene-context:bilingual-scene"]
    }
    #[test]
    fn native_resolver_emits_only_required_selected_inputs_and_stable_bilingual_fingerprints() {
        let (f, _, required, unused) = fixture();
        let r = inspected(&f);
        assert_eq!(
            context(&r).status,
            "native-resolved",
            "{}",
            serde_json::to_string(&r.findings).unwrap()
        );
        assert_eq!(context(&r).details["native_resolution_valid"], true);
        assert_eq!(
            context(&r).details["language_fingerprints"]
                .as_object()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            context(&r).details["selected_bindings"]
                .as_object()
                .unwrap()
                .len(),
            8
        );
        assert!(
            r.edges
                .iter()
                .any(|e| e.kind == "requires-asset" && e.to.ends_with(&required))
        );
        assert!(
            r.edges
                .iter()
                .any(|e| e.kind == "bound-asset" && e.to.ends_with(&unused))
        );
        assert!(
            !r.edges
                .iter()
                .any(|e| e.kind == "requires-asset" && e.to.ends_with(&unused))
        );
        assert_eq!(context(&r).details, context(&inspected(&f)).details);
        assert!(!f.root().join("offline-cold").exists());
    }
    #[test]
    fn explicit_cached_metadata_resolves_without_repository_copies_or_payload_parsing() {
        let (f, mut profile, _, _) = fixture();
        let templates = cached(&f, &crate::load(&f.root().join("templates.json")).unwrap());
        let bindings = cached(
            &f,
            &crate::load(&f.root().join("scene-bindings.json")).unwrap(),
        );
        profile["documents"]["master"] = templates.clone();
        profile["scene_contexts"][0]["templates"] = templates;
        profile["scene_contexts"][0]["scene_bindings"] = bindings.clone();
        write(&f, "inspect.json", &profile);
        fs::remove_file(f.root().join("templates.json")).unwrap();
        fs::remove_file(f.root().join("scene-bindings.json")).unwrap();
        let r = inspected(&f);
        assert_eq!(context(&r).status, "native-resolved");
        assert_eq!(
            r.nodes[bindings["cache_uri"].as_str().unwrap()]
                .schema
                .as_deref(),
            Some(native::BINDINGS_SCHEMA)
        );
        // Alignment bytes in this fixture are plain non-JSON strings. They remain opaque media.
        assert!(!r.findings.iter().any(|f| f.code == "invalid-json"));
    }
    #[test]
    fn native_scope_selection_ambiguity_and_missing_slots_are_held() {
        for change in [
            "scope",
            "selection",
            "ambiguous",
            "missing",
            "policy",
            "template",
            "source",
        ] {
            let (f, _, _, _) = fixture();
            let mut b = crate::load(&f.root().join("scene-bindings.json")).unwrap();
            match change {
                "scope" => b["scope_id"] = json!("other-scene"),
                "selection" => b["assets"]["picture.main"]["selection_state"] = json!("candidate"),
                "missing" => {
                    b["assets"].as_object_mut().unwrap().remove("take.en");
                }
                "ambiguous" => {
                    let mut e = crate::load(&f.root().join("episode-bindings.json")).unwrap();
                    e["assets"]["picture.main"] = b["assets"]["picture.main"].clone();
                    write(&f, "episode-bindings.json", &e);
                }
                "policy" => {
                    let mut p = crate::load(&f.root().join("policy.json")).unwrap();
                    p["policy_id"] = json!("other-policy");
                    write(&f, "policy.json", &p);
                }
                "template" => {
                    let mut t = crate::load(&f.root().join("templates.json")).unwrap();
                    t["templates"][1]["kind"] = json!("chapter");
                    write(&f, "templates.json", &t);
                }
                "source" => {
                    let mut s = crate::load(&f.root().join("scene.json")).unwrap();
                    s["canonical_cue_ids"] = json!([]);
                    write(&f, "scene.json", &s);
                }
                _ => unreachable!(),
            }
            write(&f, "scene-bindings.json", &b);
            let r = inspected(&f);
            assert_eq!(context(&r).status, "held", "{change}");
            let detail = &r
                .findings
                .iter()
                .find(|f| f.code == "native-scene-resolution-failed")
                .unwrap()
                .detail;
            let expected = match change {
                "scope" => "outside season",
                "selection" => "not selected",
                "missing" => "found 0",
                "ambiguous" => "found 2",
                "policy" => "policy does not match",
                "template" => "has kind",
                "source" => "canonical cue spine",
                _ => unreachable!(),
            };
            assert!(detail.contains(expected), "{change}: {detail}");
            assert_eq!(
                context(&r).details["native_resolution_valid"],
                false,
                "{change}"
            );
            assert!(
                !r.edges.iter().any(|e| e.kind == "requires-asset"),
                "{change}"
            );
        }
    }
    #[test]
    fn required_input_changes_affect_native_fingerprint_but_unused_bindings_do_not() {
        let (f, _, _, _) = fixture();
        let initial = inspected(&f);
        let first = &context(&initial).details;
        let mut bindings = crate::load(&f.root().join("scene-bindings.json")).unwrap();
        let hash = f.put(b"other unused object", true);
        bindings["assets"]["unused"]["sha256"] = json!(hash);
        bindings["assets"]["unused"]["bytes"] = json!(19);
        bindings["assets"]["unused"]["cache_uri"] = json!(format!("cache://sha256/{hash}"));
        write(&f, "scene-bindings.json", &bindings);
        let unused_changed = inspected(&f);
        let second = &context(&unused_changed).details;
        assert_eq!(first["fingerprint"], second["fingerprint"]);
        assert_ne!(
            first["documents"]["scene_bindings"]["metadata_sha256"],
            second["documents"]["scene_bindings"]["metadata_sha256"]
        );
        let hash = f.put(b"different selected picture", true);
        bindings["assets"]["picture.main"]["sha256"] = json!(hash);
        bindings["assets"]["picture.main"]["bytes"] = json!(26);
        bindings["assets"]["picture.main"]["cache_uri"] = json!(format!("cache://sha256/{hash}"));
        write(&f, "scene-bindings.json", &bindings);
        let required_changed = inspected(&f);
        assert_eq!(context(&required_changed).status, "native-resolved");
        assert_ne!(
            first["fingerprint"],
            context(&required_changed).details["fingerprint"]
        );
    }
    #[test]
    fn native_success_does_not_hide_missing_or_wrong_size_required_bytes() {
        let (f, _, required, _) = fixture();
        fs::remove_file(object(&f.root().join("fast"), &required)).unwrap();
        fs::remove_file(object(&f.root().join("history"), &required)).unwrap();
        let r = inspected(&f);
        assert_eq!(context(&r).status, "inputs-held");
        assert_eq!(context(&r).details["native_resolution_valid"], true);
        assert_eq!(context(&r).details["input_presence_valid"], false);
        assert!(
            r.findings
                .iter()
                .any(|f| f.code == "native-scene-inputs-held")
        );
    }
    #[test]
    fn cached_metadata_corruption_schema_and_descriptor_mismatch_do_not_fall_through() {
        let (f, mut profile, _, _) = fixture();
        let source = cached(
            &f,
            &crate::load(&f.root().join("scene-bindings.json")).unwrap(),
        );
        profile["scene_contexts"][0]["scene_bindings"] = source.clone();
        write(&f, "inspect.json", &profile);
        let p = object(&f.root().join("fast"), source["sha256"].as_str().unwrap());
        let len = fs::metadata(&p).unwrap().len();
        fs::write(p, vec![b'x'; len as usize]).unwrap();
        let r = inspected(&f);
        assert_eq!(context(&r).status, "held");
        assert!(r.findings.iter().any(|f| f.code == "unreadable-document"));
        profile["scene_contexts"][0]["scene_bindings"]["bytes"] = json!(1);
        write(&f, "inspect.json", &profile);
        assert!(inspect(f.root(), "inspect.json").is_err());
        profile["scene_contexts"][0]["scene_bindings"] = json!("policy.json");
        write(&f, "inspect.json", &profile);
        assert_eq!(context(&inspected(&f)).status, "held");
    }
    #[test]
    fn context_budget_ids_path_escape_and_snapshot_changes_are_rejected() {
        let (f, mut profile, _, _) = fixture();
        profile["limits"] = json!({"documents":2});
        write(&f, "inspect.json", &profile);
        assert!(inspect(f.root(), "inspect.json").is_err());
        profile.as_object_mut().unwrap().remove("limits");
        let c = profile["scene_contexts"][0].clone();
        profile["scene_contexts"].as_array_mut().unwrap().push(c);
        write(&f, "inspect.json", &profile);
        assert!(inspect(f.root(), "inspect.json").is_err());
        profile["scene_contexts"].as_array_mut().unwrap().pop();
        profile["scene_contexts"][0]["scene_bindings"] = json!("../escape.json");
        write(&f, "inspect.json", &profile);
        assert!(inspect(f.root(), "inspect.json").is_err());
        profile["scene_contexts"][0]["scene_bindings"] = json!("scene-bindings.json");
        write(&f, "inspect.json", &profile);
        assert!(
            inspect_with(f.root(), "inspect.json", || write(
                &f,
                "scene-bindings.json",
                &json!({})
            ))
            .is_err()
        );
    }
}
