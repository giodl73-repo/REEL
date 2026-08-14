use crate::production;
use anyhow::{Context, Result, anyhow, bail};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use tempfile::tempdir_in;

pub const PERFORMANCE_SCHEMA: &str = "reel.voice-performance.v0.1";
pub const PLAN_SCHEMA: &str = "reel.voice-performance-plan.v0.1";
pub const RECEIPT_SCHEMA: &str = "reel.voice-performance-plan-receipt.v0.1";

#[derive(Clone, Copy, Debug, PartialEq, ValueEnum)]
pub enum VoiceEngine {
    Chatterbox,
    Generic,
}
impl VoiceEngine {
    fn name(self) -> &'static str {
        match self {
            Self::Chatterbox => "chatterbox",
            Self::Generic => "generic",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    schema: String,
    manifest_sha256: String,
    language: String,
    directing_context: ContextBlock,
    cues: Vec<Cue>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ContextBlock {
    register: String,
    constraint: String,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cue {
    cue_id: String,
    spans: Vec<Span>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Span {
    id: String,
    text_sha256: String,
    start_char: usize,
    end_char: usize,
    action: Action,
    intensity: f64,
    pace: Pace,
    pitch_shape: Pitch,
    energy: f64,
    onset: Onset,
    #[serde(default)]
    breathiness: Option<f64>,
    #[serde(default)]
    pause_before_ms: u64,
    #[serde(default)]
    pause_after_ms: u64,
    #[serde(default)]
    stress_tokens: Vec<String>,
}
macro_rules! en {($n:ident{$($v:ident=>$s:literal),+})=>{
    #[derive(Clone,Copy,Debug,Deserialize,Serialize)] #[serde(rename_all="kebab-case")] enum $n{$($v),+}
    impl $n{fn name(self)->&'static str{match self{$(Self::$v=>$s),+}}}
}}
en!(Action{NeutralNarration=>"neutral-narration",IntimateRecollection=>"intimate-recollection",ComicAside=>"comic-aside",BreathlessPlea=>"breathless-plea",ExasperatedDemand=>"exasperated-demand",ExplosiveInterruption=>"explosive-interruption",WoundedDignity=>"wounded-dignity",PreciseCounterattack=>"precise-counterattack",DangerousThreat=>"dangerous-threat",FearDrivenWarning=>"fear-driven-warning",SuspenseBuild=>"suspense-build",SuspendedDecision=>"suspended-decision",PhysicalEffort=>"physical-effort",AstonishedRelease=>"astonished-release",DryComicButton=>"dry-comic-button"});
en!(Pace{VerySlow=>"very-slow",Slow=>"slow",Measured=>"measured",Conversational=>"conversational",Urgent=>"urgent",Fast=>"fast"});
en!(Pitch{Level=>"level",GentleRise=>"gentle-rise",GentleFall=>"gentle-fall",SharpRiseDrop=>"sharp-rise-drop",RisingQuestionFall=>"rising-question-fall",LowTight=>"low-tight",ExpansiveRelease=>"expansive-release"});
en!(Onset{Soft=>"soft",Natural=>"natural",Hard=>"hard"});

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Plan {
    pub schema: String,
    pub manifest_sha256: String,
    pub performance_sha256: String,
    pub reference_audio_sha256: Option<String>,
    pub engine: String,
    pub engine_version: String,
    pub seed: u64,
    pub language: String,
    directing_context: ContextBlock,
    pub spans: Vec<Compiled>,
    pub unsupported_dimensions: Vec<String>,
    pub human_listening_required: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Compiled {
    pub cue_id: String,
    pub span_id: String,
    pub text_sha256: String,
    pub start_char: usize,
    pub end_char: usize,
    pub action: String,
    pub requested: Requested,
    pub execution: Execution,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Requested {
    intensity: f64,
    pace: String,
    pitch_shape: String,
    energy: f64,
    onset: String,
    breathiness: Option<f64>,
    pause_before_ms: u64,
    pause_after_ms: u64,
    stress_token_hashes: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Execution {
    pub status: String,
    pub native_parameters: BTreeMap<String, serde_json::Value>,
    pub deterministic_operations: Vec<String>,
    pub advisory_only: Vec<String>,
    pub clamps: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Receipt {
    pub schema: String,
    pub manifest_sha256: String,
    pub performance_sha256: String,
    pub reference_audio_sha256: Option<String>,
    pub plan_sha256: String,
    pub engine: String,
    pub engine_version: String,
    pub seed: u64,
    pub cue_count: usize,
    pub span_count: usize,
    pub executed_dimensions: Vec<String>,
    pub advisory_dimensions: Vec<String>,
    pub human_listening_required: bool,
    pub verified: bool,
}
#[derive(Serialize)]
pub struct Packet {
    pub schema: String,
    pub plan: String,
    pub receipt: String,
    pub receipt_sha256: String,
    pub span_count: usize,
    pub verified: bool,
}
#[derive(Debug, Serialize)]
pub struct Check {
    pub schema: String,
    pub plan_sha256: String,
    pub receipt_sha256: String,
    pub verified: bool,
}
pub struct Options<'a> {
    pub manifest: &'a Path,
    pub performance: &'a Path,
    pub engine: VoiceEngine,
    pub engine_version: &'a str,
    pub reference_audio: Option<&'a Path>,
    pub seed: u64,
    pub output_dir: &'a Path,
}

pub fn write_plan(o: Options<'_>) -> Result<Packet> {
    if o.output_dir.exists() {
        bail!("voice performance output directory already exists")
    };
    if o.engine_version.trim().is_empty() {
        bail!("engine version must not be empty")
    };
    let loaded = production::load(o.manifest)?;
    production::validate(&loaded)?;
    let mh = production::sha256_path(o.manifest)?;
    let bytes = fs::read(o.performance)?;
    let input: Input =
        serde_yaml::from_slice(&bytes).context("voice performance sidecar is not valid YAML")?;
    validate(&loaded, &input, &mh)?;
    let ph = hash(&bytes);
    let rh = o.reference_audio.map(production::sha256_path).transpose()?;
    let plan = compile(
        &input,
        mh.clone(),
        ph.clone(),
        rh.clone(),
        o.engine,
        o.engine_version,
        o.seed,
    );
    let pb = serde_json::to_vec_pretty(&plan)?;
    let psha = hash(&pb);
    let advisory = plan.unsupported_dimensions.clone();
    let mut executed_dimensions = vec!["phrase-boundary".into(), "pause".into(), "pace".into()];
    if o.engine == VoiceEngine::Chatterbox {
        executed_dimensions.push("intensity-conditioning".into());
    }
    let receipt = Receipt {
        schema: RECEIPT_SCHEMA.into(),
        manifest_sha256: mh,
        performance_sha256: ph,
        reference_audio_sha256: rh,
        plan_sha256: psha,
        engine: o.engine.name().into(),
        engine_version: o.engine_version.into(),
        seed: o.seed,
        cue_count: input.cues.len(),
        span_count: plan.spans.len(),
        executed_dimensions,
        advisory_dimensions: advisory,
        human_listening_required: true,
        verified: true,
    };
    let rb = serde_json::to_vec_pretty(&receipt)?;
    let parent = o.output_dir.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let tmp = tempdir_in(parent)?;
    fs::write(tmp.path().join("plan.json"), pb)?;
    fs::write(tmp.path().join("receipt.json"), &rb)?;
    let tp = tmp.keep();
    fs::rename(tp, o.output_dir)?;
    check(o.output_dir, o.manifest, o.performance, o.reference_audio)?;
    Ok(Packet {
        schema: "reel.voice-performance-plan-packet.v0.1".into(),
        plan: o.output_dir.join("plan.json").display().to_string(),
        receipt: o.output_dir.join("receipt.json").display().to_string(),
        receipt_sha256: hash(&rb),
        span_count: plan.spans.len(),
        verified: true,
    })
}
pub fn check(
    dir: &Path,
    manifest: &Path,
    performance: &Path,
    reference: Option<&Path>,
) -> Result<Check> {
    let pb = fs::read(dir.join("plan.json"))?;
    let rb = fs::read(dir.join("receipt.json"))?;
    let r: Receipt = serde_json::from_slice(&rb)?;
    if r.schema != RECEIPT_SCHEMA || !r.verified || !r.human_listening_required {
        bail!("invalid voice performance receipt")
    };
    let loaded = production::load(manifest)?;
    production::validate(&loaded)?;
    let current_manifest = production::sha256_path(manifest)?;
    let performance_bytes = fs::read(performance)?;
    let input: Input = serde_yaml::from_slice(&performance_bytes)
        .context("voice performance sidecar is not valid YAML")?;
    validate(&loaded, &input, &current_manifest)?;
    let current_performance = hash(&performance_bytes);
    let current_plan = hash(&pb);
    let checks = [
        (
            "manifest",
            r.manifest_sha256.as_str(),
            current_manifest.as_str(),
        ),
        (
            "performance",
            r.performance_sha256.as_str(),
            current_performance.as_str(),
        ),
        ("plan", r.plan_sha256.as_str(), current_plan.as_str()),
    ];
    for (n, a, b) in checks {
        if a != b {
            bail!("voice performance {n} hash does not match receipt")
        }
    }
    let plan: Plan = serde_json::from_slice(&pb)?;
    if plan.schema != PLAN_SCHEMA || !plan.human_listening_required {
        bail!("invalid voice performance plan")
    }
    let current_reference = reference.map(production::sha256_path).transpose()?;
    if r.reference_audio_sha256.as_ref() != current_reference.as_ref() {
        bail!("voice performance reference audio hash does not match receipt")
    };
    if plan.manifest_sha256 != r.manifest_sha256
        || plan.performance_sha256 != r.performance_sha256
        || plan.reference_audio_sha256 != r.reference_audio_sha256
        || plan.engine != r.engine
        || plan.engine_version != r.engine_version
        || plan.seed != r.seed
        || plan.spans.len() != r.span_count
        || input.cues.len() != r.cue_count
    {
        bail!("voice performance plan and receipt disagree")
    }
    Ok(Check {
        schema: "reel.voice-performance-plan-check.v0.1".into(),
        plan_sha256: current_plan,
        receipt_sha256: hash(&rb),
        verified: true,
    })
}
fn validate(loaded: &production::LoadedProductionManifest, i: &Input, mh: &str) -> Result<()> {
    if i.schema != PERFORMANCE_SCHEMA {
        bail!("voice performance schema must be {PERFORMANCE_SCHEMA}")
    };
    if i.manifest_sha256 != mh {
        bail!("voice performance manifest hash is stale")
    };
    for (n, v) in [
        ("language", &i.language),
        ("register", &i.directing_context.register),
        ("constraint", &i.directing_context.constraint),
    ] {
        if v.trim().is_empty() {
            bail!("voice performance {n} must not be empty")
        }
    }
    let cues = loaded
        .manifest
        .narration_cues
        .iter()
        .map(|c| (c.id.as_str(), c))
        .collect::<BTreeMap<_, _>>();
    let mut ci = BTreeSet::new();
    let mut si = BTreeSet::new();
    for dc in &i.cues {
        if !ci.insert(&dc.cue_id) {
            bail!("duplicate voice performance cue")
        };
        let c = cues
            .get(dc.cue_id.as_str())
            .ok_or_else(|| anyhow!("unknown voice performance cue {}", dc.cue_id))?;
        if c.text.is_empty() {
            bail!("voice performance requires inline exact text")
        };
        let chars = c.text.chars().collect::<Vec<_>>();
        let mut next = 0;
        for (idx, s) in dc.spans.iter().enumerate() {
            if !si.insert(&s.id) {
                bail!("duplicate voice performance span")
            };
            if s.start_char != next {
                bail!("voice performance cue has a gap or overlap")
            };
            if s.end_char <= s.start_char || s.end_char > chars.len() {
                bail!("invalid voice performance character bounds")
            };
            for (n, v) in [("intensity", s.intensity), ("energy", s.energy)] {
                bound(n, v)?
            }
            if let Some(v) = s.breathiness {
                bound("breathiness", v)?
            };
            let txt = chars[s.start_char..s.end_char].iter().collect::<String>();
            if hash(txt.as_bytes()) != s.text_sha256 {
                bail!("voice performance span text hash is stale")
            };
            let lo = txt.to_lowercase();
            for t in &s.stress_tokens {
                if t.trim().is_empty() || !lo.contains(&t.to_lowercase()) {
                    bail!("voice performance span has unknown stress token")
                }
            }
            if idx > 0 {
                let p = &dc.spans[idx - 1];
                if p.pause_after_ms > 0
                    && s.pause_before_ms > 0
                    && p.pause_after_ms != s.pause_before_ms
                {
                    bail!("voice performance spans declare contradictory pauses")
                }
            };
            next = s.end_char
        }
        if next != chars.len() {
            bail!("voice performance cue does not cover exact text")
        }
    }
    Ok(())
}
fn compile(
    i: &Input,
    mh: String,
    ph: String,
    rh: Option<String>,
    engine: VoiceEngine,
    version: &str,
    seed: u64,
) -> Plan {
    let mut out = Vec::new();
    let mut unsupported = BTreeSet::new();
    for c in &i.cues {
        for s in &c.spans {
            let mut native = BTreeMap::new();
            let mut clamps = Vec::new();
            let advisory = match engine {
                VoiceEngine::Chatterbox => {
                    let raw_exaggeration = 0.35 + s.intensity * 0.60;
                    let exaggeration = raw_exaggeration.clamp(0.35, 0.90);
                    if (raw_exaggeration - exaggeration).abs() > f64::EPSILON {
                        clamps.push(format!(
                            "exaggeration {:.3} clamped to {:.3}",
                            raw_exaggeration, exaggeration
                        ));
                    }
                    native.insert("exaggeration".into(), serde_json::json!(r3(exaggeration)));
                    native.insert(
                        "cfg_weight".into(),
                        serde_json::json!(r3((0.56 - s.intensity * 0.28).clamp(0.28, 0.56))),
                    );
                    vec![
                        "action",
                        "pitch_shape",
                        "onset",
                        "stress_tokens",
                        "breathiness",
                        "energy",
                        "cultural_register",
                    ]
                }
                VoiceEngine::Generic => vec![
                    "action",
                    "intensity",
                    "pitch_shape",
                    "energy",
                    "onset",
                    "stress_tokens",
                    "breathiness",
                    "cultural_register",
                ],
            }
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
            unsupported.extend(advisory.iter().cloned());
            out.push(Compiled {
                cue_id: c.cue_id.clone(),
                span_id: s.id.clone(),
                text_sha256: s.text_sha256.clone(),
                start_char: s.start_char,
                end_char: s.end_char,
                action: s.action.name().into(),
                requested: Requested {
                    intensity: s.intensity,
                    pace: s.pace.name().into(),
                    pitch_shape: s.pitch_shape.name().into(),
                    energy: s.energy,
                    onset: s.onset.name().into(),
                    breathiness: s.breathiness,
                    pause_before_ms: s.pause_before_ms,
                    pause_after_ms: s.pause_after_ms,
                    stress_token_hashes: s
                        .stress_tokens
                        .iter()
                        .map(|t| hash(t.to_lowercase().as_bytes()))
                        .collect(),
                },
                execution: Execution {
                    status: if native.is_empty() {
                        "advisory-only"
                    } else {
                        "partially-executable"
                    }
                    .into(),
                    native_parameters: native,
                    deterministic_operations: vec![
                        "split exact cue text at character bounds".into(),
                        format!("tempo factor {:.3}", pace(s.pace)),
                        format!(
                            "pause before {}ms after {}ms",
                            s.pause_before_ms, s.pause_after_ms
                        ),
                    ],
                    advisory_only: advisory,
                    clamps,
                },
            })
        }
    }
    Plan {
        schema: PLAN_SCHEMA.into(),
        manifest_sha256: mh,
        performance_sha256: ph,
        reference_audio_sha256: rh,
        engine: engine.name().into(),
        engine_version: version.into(),
        seed,
        language: i.language.clone(),
        directing_context: i.directing_context.clone(),
        spans: out,
        unsupported_dimensions: unsupported.into_iter().collect(),
        human_listening_required: true,
    }
}
fn pace(p: Pace) -> f64 {
    match p {
        Pace::VerySlow => 0.78,
        Pace::Slow => 0.88,
        Pace::Measured => 0.95,
        Pace::Conversational => 1.0,
        Pace::Urgent => 1.08,
        Pace::Fast => 1.15,
    }
}
fn bound(n: &str, v: f64) -> Result<()> {
    if !v.is_finite() || !(0.0..=1.0).contains(&v) {
        bail!("voice performance {n} must be within 0..1")
    };
    Ok(())
}
fn r3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}
fn hash(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|v| format!("{v:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const M: &str = "manifests/fixtures/voice-performance/manifest.yaml";
    const P: &str = "manifests/fixtures/voice-performance/performance.yaml";

    #[test]
    fn compiles_intense_chatterbox_span_and_discloses_limits() {
        let d = tempfile::tempdir().unwrap();
        let out = d.path().join("packet");
        let report = write_plan(Options {
            manifest: Path::new(M),
            performance: Path::new(P),
            engine: VoiceEngine::Chatterbox,
            engine_version: "0.1.7",
            reference_audio: None,
            seed: 1947,
            output_dir: &out,
        })
        .unwrap();
        assert!(report.verified);
        let plan: Plan = serde_json::from_slice(&fs::read(out.join("plan.json")).unwrap()).unwrap();
        assert_eq!(plan.spans.len(), 6);
        assert_eq!(plan.spans[1].action, "explosive-interruption");
        assert_eq!(
            plan.spans[1].execution.native_parameters["exaggeration"],
            serde_json::json!(0.9)
        );
        assert!(
            plan.unsupported_dimensions
                .contains(&"pitch_shape".to_string())
        );
        assert!(
            check(&out, Path::new(M), Path::new(P), None)
                .unwrap()
                .verified
        );
    }

    #[test]
    fn rejects_stale_span_hash_overlap_and_unknown_stress() {
        let loaded = production::load(M).unwrap();
        let mh = production::sha256_path(M).unwrap();
        let bytes = fs::read(P).unwrap();
        let mut v: serde_yaml::Value = serde_yaml::from_slice(&bytes).unwrap();
        v["cues"][0]["spans"][0]["text_sha256"] = serde_yaml::Value::String("bad".into());
        let i: Input = serde_yaml::from_value(v).unwrap();
        assert!(
            validate(&loaded, &i, &mh)
                .unwrap_err()
                .to_string()
                .contains("text hash")
        );
        let mut v: serde_yaml::Value = serde_yaml::from_slice(&bytes).unwrap();
        v["cues"][0]["spans"][1]["start_char"] = serde_yaml::Value::Number(1.into());
        let i: Input = serde_yaml::from_value(v).unwrap();
        assert!(
            validate(&loaded, &i, &mh)
                .unwrap_err()
                .to_string()
                .contains("gap or overlap")
        );
        let mut v: serde_yaml::Value = serde_yaml::from_slice(&bytes).unwrap();
        v["cues"][0]["spans"][0]["stress_tokens"] = serde_yaml::to_value(vec!["missing"]).unwrap();
        let i: Input = serde_yaml::from_value(v).unwrap();
        assert!(
            validate(&loaded, &i, &mh)
                .unwrap_err()
                .to_string()
                .contains("unknown stress")
        );
    }

    #[test]
    fn detects_tampered_plan() {
        let d = tempfile::tempdir().unwrap();
        let out = d.path().join("packet");
        write_plan(Options {
            manifest: Path::new(M),
            performance: Path::new(P),
            engine: VoiceEngine::Generic,
            engine_version: "fixture",
            reference_audio: None,
            seed: 7,
            output_dir: &out,
        })
        .unwrap();
        fs::write(out.join("plan.json"), "{}\n").unwrap();
        assert!(
            check(&out, Path::new(M), Path::new(P), None)
                .unwrap_err()
                .to_string()
                .contains("plan hash")
        );
    }
}
