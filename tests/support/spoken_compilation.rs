use std::{collections::BTreeMap, cell::RefCell, path::{Path, PathBuf}};
use anyhow::{Context, Result};
use reel::{scene_delivery::{FileRef, InputFiles}, spoken_programme::{self, Document, PerformanceKind,
    compiler::{self, CompiledPlan, CompilerIdentity}}};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

struct Fixture {
    document: Document,
    metadata: BTreeMap<String, Vec<u8>>,
    runtime: CompilerIdentity,
}

impl Fixture {
    fn engineering() -> Self {
        let document: Document = serde_json::from_value(super::fixture()).unwrap();
        let mut f = Self { document, metadata:BTreeMap::new(),
            runtime:CompilerIdentity { sha256:hash(b"fixture-compiler-not-an-admitted-runtime"), bytes:128 } };
        for index in 0..f.document.performances.len() {
            f.document.performances[index].observed_text_sha256 = hash(b"");
            let p = &f.document.performances[index];
            let evidence = json!({"schema":compiler::ALIGNMENT_SCHEMA,"performance_id":p.id,
                "cue_id":p.cue_id,"language":p.language,"source":p.source,"sample_rate":48000,
                "duration_samples":p.duration_samples,"script_id":p.script_id,"script_sha256":p.script_sha256,
                "observation":{"kind":"engineering-signal"},"markers":p.markers,"word_bindings":[]});
            f.replace(index, &evidence);
        }
        f
    }

    fn lexical() -> Self {
        let mut f = Self::engineering();
        for index in 0..f.document.performances.len() {
            let p = &f.document.performances[index];
            let text = f.document.modules.iter().find(|m| m.id == p.module_id).unwrap()
                .cues.iter().find(|c| c.id == p.cue_id).unwrap().scripts.iter()
                .find(|s| s.id == p.script_id).unwrap().text.clone();
            let start = if p.language == "en" {2400} else {4800};
            let words = text.split_whitespace().enumerate().map(|(i,word)| json!({
                "word":word,"start_sample":start+i*2400,"end_sample":start+i*2400+1200
            })).collect::<Vec<_>>();
            let markers = json!([
                {"id":"first-word","kind":"word","start_sample":start,"end_sample":start+1200},
                {"id":"following-phrase","kind":"phrase","start_sample":start+2400,
                 "end_sample":start+(words.len()-1)*2400+1200}
            ]);
            let mut value = f.evidence(index);
            value["observation"] = json!({"kind":"word-evidence","observed_text":text,
                "timing":{"schema":"reel.scene-word-timing-evidence.v1","language":p.language,
                    "cue_id":p.cue_id,"selected_take_sha256":p.source.sha256,"sample_rate":48000,
                    "cue_end_sample":p.duration_samples,"words":words}});
            value["markers"] = markers.clone();
            value["word_bindings"] = json!([
                {"marker_id":"first-word","first_word_index":0,"end_word_index_exclusive":1},
                {"marker_id":"following-phrase","first_word_index":1,"end_word_index_exclusive":words.len()}
            ]);
            let p = &mut f.document.performances[index];
            p.scope.kind = PerformanceKind::RecordedSpeech;
            p.observed_text_sha256 = hash(text.as_bytes());
            p.markers = serde_json::from_value(markers).unwrap();
            f.replace(index, &value);
        }
        f
    }

    fn evidence(&self, index: usize) -> Value {
        serde_json::from_slice(&self.metadata[&self.document.performances[index].id]).unwrap()
    }

    fn replace(&mut self, index: usize, evidence: &Value) {
        let bytes = serde_json::to_vec(evidence).unwrap();
        let p = &mut self.document.performances[index];
        p.alignment.sha256 = hash(&bytes); p.alignment.bytes = bytes.len() as u64;
        self.metadata.insert(p.id.clone(), bytes);
    }

    fn compile(&self) -> Result<CompiledPlan> {
        compiler::compile(&self.document, &self.metadata, &self.runtime)
    }

    fn recipes(&self) -> BTreeMap<String,String> {
        self.compile().unwrap().editions.into_iter().map(|e| (e.recipe.edition_id,e.recipe_sha256)).collect()
    }
}

#[test]
fn compiles_four_exact_sample_plans() {
    let f = Fixture::engineering();
    let plan = f.compile().unwrap();
    let again = f.compile().unwrap();
    assert_eq!(serde_json::to_vec(&plan).unwrap(), serde_json::to_vec(&again).unwrap());
    let expected = [
        ("en-executive",vec![(0,144000),(156000,300000)]),
        ("en-technical",vec![(0,144000),(158400,350400),(364800,508800)]),
        ("es-executive",vec![(0,168000),(184800,352800)]),
        ("es-technical",vec![(0,168000),(187200,403200),(422400,590400)]),
    ];
    for (id,spans) in expected {
        let e = plan.editions.iter().find(|e| e.recipe.edition_id == id).unwrap();
        assert_eq!(e.cues.iter().map(|c| (c.start_sample,c.end_sample)).collect::<Vec<_>>(),spans);
        assert_eq!(e.duration_samples, spans.last().unwrap().1);
        assert_eq!(e.chapters.len(), spans.len());
        assert!(e.words.is_empty());
        assert!(e.cues.iter().all(|c| c.engineering_signal));
    }
    assert!(plan.alignment_metadata_verified);
    assert!(!plan.media_verified && !plan.authority_verified);
}

#[test]
fn lexical_metadata_is_not_audio_proof_and_keeps_language_local_positions() {
    let f = Fixture::lexical();
    let plan = f.compile().unwrap();
    for e in &plan.editions {
        assert!(!e.words.is_empty());
        assert!(!e.cues.iter().any(|c| c.engineering_signal));
        let offset = if e.recipe.language == "en" {2400} else {4800};
        assert_eq!(e.words[0].start_sample, offset);
        assert_eq!(e.markers[0].marker.start_sample, offset);
        assert!(e.words.iter().all(|w| w.end_sample <= e.duration_samples));
    }
    assert!(!plan.media_verified && !plan.authority_verified);
}

#[test]
fn rejects_context_marker_trim_overlap_and_clock_errors() {
    let mut f = Fixture::engineering();
    f.metadata.remove("en-opening");
    assert!(f.compile().unwrap_err().to_string().contains("evidence set"));
    let mut f = Fixture::engineering();
    f.metadata.insert("extra".into(),vec![]);
    assert!(f.compile().unwrap_err().to_string().contains("evidence set"));
    let mut f = Fixture::engineering();
    f.metadata.get_mut("en-opening").unwrap().push(b' ');
    assert!(f.compile().unwrap_err().to_string().contains("pin"));
    let mut f = Fixture::engineering();
    let mut evidence = f.evidence(0); evidence["cue_id"] = json!("wrong"); f.replace(0,&evidence);
    let error = f.compile().unwrap_err().to_string();
    assert!(error.contains("identity") && error.contains("performance index 0"));
    let mut f = Fixture::engineering();
    f.document.sample_rate = 44100;
    assert!(f.compile().unwrap_err().to_string().contains("sample rate"));
    let mut f = Fixture::engineering();
    f.document.performances[1].duration_samples = 720000;
    let mut evidence = f.evidence(1); evidence["duration_samples"] = json!(720000); f.replace(1,&evidence);
    f.document.editions[1].selections[1].takes[0].end_sample = 720000;
    assert!(f.compile().unwrap_err().to_string().contains("duration exceeds"));
    let mut f = Fixture::lexical();
    f.document.performances[0].protected.clear();
    f.document.editions[0].selections[0].takes[0].start_sample = 3600;
    let error = f.compile().unwrap_err().to_string();
    assert!(error.contains("trim excludes") && error.contains("edition index 0") && error.contains("cue index 0"));
}

#[test]
fn rehashed_word_evidence_still_requires_native_marker_and_text_correspondence() {
    let mut f = Fixture::lexical();
    let mut v = f.evidence(0); v["word_bindings"][0]["end_word_index_exclusive"] = json!(2); f.replace(0,&v);
    assert!(f.compile().unwrap_err().to_string().contains("native words"));
    let mut f = Fixture::lexical();
    let mut v = f.evidence(0); v["observation"]["observed_text"] = json!("private-canary wrong words");
    f.document.performances[0].observed_text_sha256 = hash(b"private-canary wrong words"); f.replace(0,&v);
    let error = f.compile().unwrap_err().to_string();
    assert!(error.contains("word sequence")); assert!(!error.contains("private-canary"));
    let mut f = Fixture::engineering();
    let mut v = f.evidence(0); v["private-canary"] = json!("must not leak"); f.replace(0,&v);
    let error = f.compile().unwrap_err().to_string();
    assert!(error.contains("strict JSON")); assert!(!error.contains("private-canary"));
    let mut f = Fixture::engineering();
    let mut v = f.evidence(0); v["observation"]["observed_text"] = json!("private-canary"); f.replace(0,&v);
    assert!(f.compile().unwrap_err().to_string().contains("strict JSON"));
}

#[test]
fn consumed_recipes_ignore_unused_variants_and_global_registry_order() {
    let mut f = Fixture::engineering();
    let before = f.recipes();
    let aggregate = f.compile().unwrap().declaration_semantics_sha256;
    f.document.modules.reverse(); f.document.performances.reverse(); f.document.editions.reverse();
    for e in &mut f.document.editions { e.required_modules.reverse(); }
    for p in &mut f.document.performances { p.scope.edition_ids.reverse(); }
    assert_eq!(before,f.recipes());
    assert_ne!(aggregate,f.compile().unwrap().declaration_semantics_sha256);
    let mut unused = f.document.performances[0].clone();
    unused.id = "unselected-variant".into(); unused.alignment.path = PathBuf::from("not-present.json");
    f.document.performances.push(unused);
    assert_eq!(before,f.recipes());
}

#[test]
fn spanish_detail_change_preserves_other_consumed_recipes() {
    let mut f = Fixture::engineering();
    let before = f.recipes();
    let index = f.document.performances.iter().position(|p| p.id == "es-detail").unwrap();
    let p = &mut f.document.performances[index];
    p.duration_samples += 24000; p.source.sha256 = hash(b"spanish-detail-v2");
    for region in &mut p.protected { region.end_sample += 24000; }
    let mut v = f.evidence(index);
    v["duration_samples"] = json!(f.document.performances[index].duration_samples);
    v["source"] = serde_json::to_value(&f.document.performances[index].source).unwrap();
    f.replace(index,&v);
    f.document.editions.iter_mut().find(|e| e.id == "es-technical").unwrap().selections[1].takes[0].end_sample += 24000;
    let after = f.recipes();
    for (id,recipe) in before { assert_eq!(recipe == after[&id], id != "es-technical", "{id}"); }
}

struct ControlsOnly { bytes:BTreeMap<PathBuf,Vec<u8>>, reads:RefCell<Vec<PathBuf>> }
impl InputFiles for ControlsOnly {
    fn verified_path(&self, _: &FileRef) -> Result<PathBuf> { anyhow::bail!("source media must not open") }
    fn control_bytes(&self, item: &FileRef) -> Result<Vec<u8>> {
        self.reads.borrow_mut().push(item.path.clone());
        self.bytes.get(&item.path).cloned().context("metadata not staged")
    }
}

#[test]
fn input_port_reads_only_selected_metadata_without_media() {
    let mut f = Fixture::engineering();
    let mut unused = f.document.performances[0].clone();
    unused.id = "unused".into(); unused.alignment.path = PathBuf::from("never-read.json");
    f.document.performances.push(unused);
    let provider = ControlsOnly { bytes:f.document.performances.iter().filter_map(|p|
        f.metadata.get(&p.id).map(|bytes| (p.alignment.path.clone(),bytes.clone()))).collect(), reads:RefCell::new(vec![]) };
    assert!(compiler::compile_with_inputs(&f.document,&provider,&f.runtime).is_ok());
    assert_eq!(provider.reads.borrow().len(),6);
    assert!(!provider.reads.borrow().iter().any(|p| p == Path::new("never-read.json")));
}

#[test]
fn cli_compiles_without_audio_and_never_clobbers_or_leaks_private_errors() {
    let f = Fixture::engineering();
    let root = tempfile::tempdir().unwrap();
    for p in &f.document.performances {
        let path = root.path().join(&p.alignment.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path,&f.metadata[&p.id]).unwrap();
    }
    let declaration = root.path().join("private-canary.json");
    std::fs::write(&declaration,serde_json::to_vec(&f.document).unwrap()).unwrap();
    let output = root.path().join("plan.json");
    let run = |destination: &Path| std::process::Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("spoken-edition-compile").arg(&declaration).arg("--output").arg(destination).output().unwrap();
    let result = run(&output);
    assert!(result.status.success(),"{}",String::from_utf8_lossy(&result.stderr));
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["editions"],4); assert_eq!(report["media_verified"],false);
    let original = std::fs::read(&output).unwrap();
    assert!(!run(&output).status.success()); assert_eq!(std::fs::read(&output).unwrap(),original);
    std::fs::write(root.path().join(&f.document.performances[0].alignment.path),b"private-canary corrupt").unwrap();
    let failed = root.path().join("not-published.json");
    let result = run(&failed); assert!(!result.status.success()); assert!(!failed.exists());
    assert!(!String::from_utf8_lossy(&result.stderr).contains("private-canary"));
    assert!(!String::from_utf8_lossy(&result.stdout).contains("private-canary"));
    assert!(f.document.performances.iter().all(|p| !root.path().join(&p.source.path).exists()));
}
