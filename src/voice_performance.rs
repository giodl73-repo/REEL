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
pub const PROSODY_MEASUREMENTS_SCHEMA: &str = "reel.voice-prosody-measurements.v0.1";
pub const PROSODY_EVIDENCE_SCHEMA: &str = "reel.voice-prosody-evidence.v0.1";

#[derive(Clone, Copy, Debug, PartialEq, ValueEnum)]
pub enum VoiceEngine {
    Chatterbox,
    Indextts25,
    Generic,
}
impl VoiceEngine {
    fn name(self) -> &'static str {
        match self {
            Self::Chatterbox => "chatterbox",
            Self::Indextts25 => "indextts25",
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
    emotion_scope: Option<EmotionScope>,
    #[serde(default)]
    baseline_register: Option<BaselineRegister>,
    #[serde(default)]
    pitch_contour: Option<PitchContour>,
    #[serde(default)]
    terminal_boundary: Option<TerminalBoundary>,
    #[serde(default)]
    relative_pitch_target_semitones: Option<RelativePitchTarget>,
    #[serde(default)]
    join_after: Option<SpanJoin>,
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
en!(EmotionScope{WholeSpan=>"whole-span",Onset=>"onset",Body=>"body",Terminal=>"terminal"});
en!(BaselineRegister{SpeakerReference=>"speaker-reference",Lower=>"lower",Level=>"level",Higher=>"higher"});
en!(PitchContour{Level=>"level",Rising=>"rising",Falling=>"falling",RiseFall=>"rise-fall",FallRise=>"fall-rise"});
en!(TerminalBoundary{Open=>"open",Suspended=>"suspended",DecisiveFall=>"decisive-fall",QuestionRise=>"question-rise"});
en!(SpanJoin{Seamless=>"seamless",Natural=>"natural",ProtectedPause=>"protected-pause"});

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RelativePitchTarget {
    start: f64,
    middle: f64,
    end: f64,
}

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
    emotion_scope: Option<String>,
    baseline_register: Option<String>,
    pitch_contour: Option<String>,
    terminal_boundary: Option<String>,
    relative_pitch_target_semitones: Option<RelativePitchTarget>,
    join_after: Option<String>,
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
            if let Some(target) = &s.relative_pitch_target_semitones {
                for (name, value) in [
                    ("start", target.start),
                    ("middle", target.middle),
                    ("end", target.end),
                ] {
                    if !value.is_finite() || !(-24.0..=24.0).contains(&value) {
                        bail!(
                            "voice performance relative pitch target {name} must be within -24..24 semitones"
                        )
                    }
                }
                if matches!(s.terminal_boundary, Some(TerminalBoundary::DecisiveFall))
                    && target.end >= target.middle
                {
                    bail!(
                        "voice performance decisive-fall requires an ending below the middle target"
                    )
                }
                if matches!(s.terminal_boundary, Some(TerminalBoundary::QuestionRise))
                    && target.end <= target.middle
                {
                    bail!(
                        "voice performance question-rise requires an ending above the middle target"
                    )
                }
            }
            if matches!(s.join_after, Some(SpanJoin::Seamless)) && s.pause_after_ms > 0 {
                bail!("voice performance seamless join cannot declare a pause after")
            }
            if matches!(s.join_after, Some(SpanJoin::ProtectedPause)) && s.pause_after_ms == 0 {
                bail!("voice performance protected-pause join requires a pause after")
            }
            if matches!(s.terminal_boundary, Some(TerminalBoundary::DecisiveFall))
                && matches!(
                    s.pitch_contour,
                    Some(PitchContour::Rising | PitchContour::FallRise)
                )
            {
                bail!("voice performance decisive-fall contradicts requested pitch contour")
            }
            if matches!(s.terminal_boundary, Some(TerminalBoundary::QuestionRise))
                && matches!(
                    s.pitch_contour,
                    Some(PitchContour::Falling | PitchContour::RiseFall)
                )
            {
                bail!("voice performance question-rise contradicts requested pitch contour")
            }
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
            let mut advisory = match engine {
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
                VoiceEngine::Indextts25 | VoiceEngine::Generic => vec![
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
            for (requested, dimension) in [
                (s.emotion_scope.is_some(), "emotion_scope"),
                (s.baseline_register.is_some(), "baseline_register"),
                (s.pitch_contour.is_some(), "pitch_contour"),
                (s.terminal_boundary.is_some(), "terminal_boundary"),
                (
                    s.relative_pitch_target_semitones.is_some(),
                    "relative_pitch_target_semitones",
                ),
                (s.join_after.is_some(), "span_join"),
            ] {
                if requested {
                    advisory.push(dimension.into());
                }
            }
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
                    emotion_scope: s.emotion_scope.map(|v| v.name().into()),
                    baseline_register: s.baseline_register.map(|v| v.name().into()),
                    pitch_contour: s.pitch_contour.map(|v| v.name().into()),
                    terminal_boundary: s.terminal_boundary.map(|v| v.name().into()),
                    relative_pitch_target_semitones: s.relative_pitch_target_semitones.clone(),
                    join_after: s.join_after.map(|v| v.name().into()),
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

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProsodyMeasurements {
    schema: String,
    plan_sha256: String,
    rendered_audio_sha256: String,
    analyzer: String,
    analyzer_version: String,
    spans: Vec<ProsodyMeasurement>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProsodyMeasurement {
    span_id: String,
    start_seconds: f64,
    end_seconds: f64,
    median_f0_hz: f64,
    first_f0_hz: f64,
    middle_f0_hz: f64,
    final_f0_hz: f64,
    voiced_frame_coverage: f64,
    duration_seconds: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProsodySpanEvidence {
    pub span_id: String,
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub median_f0_hz: f64,
    pub first_f0_hz: f64,
    pub middle_f0_hz: f64,
    pub final_f0_hz: f64,
    pub first_to_middle_semitones: f64,
    pub middle_to_final_semitones: f64,
    pub first_to_final_semitones: f64,
    pub voiced_frame_coverage: f64,
    pub duration_seconds: f64,
    pub measurement_reliable: bool,
    pub detected_contour: String,
    pub requested_pitch_contour: Option<String>,
    pub pitch_contour_match: Option<bool>,
    pub requested_terminal_boundary: Option<String>,
    pub terminal_boundary_match: Option<bool>,
    pub relative_pitch_target_match: Option<bool>,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProsodyEvidence {
    pub schema: String,
    pub plan_sha256: String,
    pub plan_receipt_sha256: String,
    pub measurements_sha256: String,
    pub rendered_audio_sha256: String,
    pub analyzer: String,
    pub analyzer_version: String,
    pub spans: Vec<ProsodySpanEvidence>,
    pub passed: bool,
    pub human_listening_required: bool,
    pub verified: bool,
}

#[derive(Debug, Serialize)]
pub struct ProsodyPacket {
    pub schema: String,
    pub evidence: String,
    pub evidence_sha256: String,
    pub span_count: usize,
    pub passed: bool,
    pub verified: bool,
}

#[derive(Debug, Serialize)]
pub struct ProsodyCheck {
    pub schema: String,
    pub evidence_sha256: String,
    pub passed: bool,
    pub verified: bool,
}

pub struct ProsodyOptions<'a> {
    pub packet_dir: &'a Path,
    pub measurements: &'a Path,
    pub rendered_audio: &'a Path,
    pub output_dir: &'a Path,
}

pub fn write_prosody_evidence(o: ProsodyOptions<'_>) -> Result<ProsodyPacket> {
    if o.output_dir.exists() {
        bail!("voice prosody output directory already exists")
    }
    let evidence = build_prosody_evidence(o.packet_dir, o.measurements, o.rendered_audio)?;
    let bytes = serde_json::to_vec_pretty(&evidence)?;
    let evidence_sha256 = hash(&bytes);
    let parent = o.output_dir.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let tmp = tempdir_in(parent)?;
    fs::write(tmp.path().join("evidence.json"), &bytes)?;
    let tp = tmp.keep();
    fs::rename(tp, o.output_dir)?;
    Ok(ProsodyPacket {
        schema: "reel.voice-prosody-evidence-packet.v0.1".into(),
        evidence: o.output_dir.join("evidence.json").display().to_string(),
        evidence_sha256,
        span_count: evidence.spans.len(),
        passed: evidence.passed,
        verified: true,
    })
}

pub fn check_prosody_evidence(
    evidence_dir: &Path,
    packet_dir: &Path,
    measurements: &Path,
    rendered_audio: &Path,
) -> Result<ProsodyCheck> {
    let bytes = fs::read(evidence_dir.join("evidence.json"))?;
    let stored: ProsodyEvidence = serde_json::from_slice(&bytes)?;
    let expected = build_prosody_evidence(packet_dir, measurements, rendered_audio)?;
    if stored != expected {
        bail!("voice prosody evidence does not match bound inputs or computed findings")
    }
    Ok(ProsodyCheck {
        schema: "reel.voice-prosody-evidence-check.v0.1".into(),
        evidence_sha256: hash(&bytes),
        passed: stored.passed,
        verified: true,
    })
}

fn build_prosody_evidence(
    packet_dir: &Path,
    measurements_path: &Path,
    rendered_audio: &Path,
) -> Result<ProsodyEvidence> {
    let plan_bytes = fs::read(packet_dir.join("plan.json"))?;
    let plan_sha256 = hash(&plan_bytes);
    let plan: Plan = serde_json::from_slice(&plan_bytes)?;
    if plan.schema != PLAN_SCHEMA || !plan.human_listening_required {
        bail!("invalid voice performance plan for prosody evidence")
    }
    let plan_receipt_bytes = fs::read(packet_dir.join("receipt.json"))?;
    let plan_receipt: Receipt = serde_json::from_slice(&plan_receipt_bytes)?;
    if plan_receipt.schema != RECEIPT_SCHEMA
        || !plan_receipt.verified
        || plan_receipt.plan_sha256 != plan_sha256
        || plan_receipt.engine != plan.engine
        || plan_receipt.engine_version != plan.engine_version
        || plan_receipt.seed != plan.seed
    {
        bail!("invalid or stale voice performance plan receipt for prosody evidence")
    }
    let measurement_bytes = fs::read(measurements_path)?;
    let input: ProsodyMeasurements = serde_yaml::from_slice(&measurement_bytes)
        .context("voice prosody measurements are not valid YAML")?;
    if input.schema != PROSODY_MEASUREMENTS_SCHEMA {
        bail!("voice prosody measurements schema must be {PROSODY_MEASUREMENTS_SCHEMA}")
    }
    if input.plan_sha256 != plan_sha256 {
        bail!("voice prosody measurements plan hash is stale")
    }
    let rendered_audio_sha256 = production::sha256_path(rendered_audio)?;
    if input.rendered_audio_sha256 != rendered_audio_sha256 {
        bail!("voice prosody measurements rendered audio hash is stale")
    }
    if input.analyzer.trim().is_empty() || input.analyzer_version.trim().is_empty() {
        bail!("voice prosody analyzer and version must not be empty")
    }
    if input.spans.len() != plan.spans.len() {
        bail!("voice prosody measurements must cover every performance span")
    }
    let mut spans = Vec::new();
    let mut previous_end = 0.0;
    for (span, measurement) in plan.spans.iter().zip(&input.spans) {
        if span.span_id != measurement.span_id {
            bail!("voice prosody measurement spans must follow exact plan order")
        }
        validate_measurement(measurement)?;
        if measurement.start_seconds + 0.001 < previous_end {
            bail!("voice prosody measurement time bounds overlap")
        }
        previous_end = measurement.end_seconds;
        spans.push(evaluate_measurement(span, measurement));
    }
    let passed = spans.iter().all(|span| span.status != "failed");
    Ok(ProsodyEvidence {
        schema: PROSODY_EVIDENCE_SCHEMA.into(),
        plan_sha256,
        plan_receipt_sha256: hash(&plan_receipt_bytes),
        measurements_sha256: hash(&measurement_bytes),
        rendered_audio_sha256,
        analyzer: input.analyzer,
        analyzer_version: input.analyzer_version,
        spans,
        passed,
        human_listening_required: true,
        verified: true,
    })
}

fn validate_measurement(m: &ProsodyMeasurement) -> Result<()> {
    for (name, value) in [
        ("median_f0_hz", m.median_f0_hz),
        ("first_f0_hz", m.first_f0_hz),
        ("middle_f0_hz", m.middle_f0_hz),
        ("final_f0_hz", m.final_f0_hz),
        ("duration_seconds", m.duration_seconds),
    ] {
        if !value.is_finite() || value <= 0.0 {
            bail!("voice prosody {name} must be finite and positive")
        }
    }
    if !m.voiced_frame_coverage.is_finite() || !(0.0..=1.0).contains(&m.voiced_frame_coverage) {
        bail!("voice prosody voiced_frame_coverage must be within 0..1")
    }
    if !m.start_seconds.is_finite()
        || !m.end_seconds.is_finite()
        || m.start_seconds < 0.0
        || m.end_seconds <= m.start_seconds
    {
        bail!("voice prosody span time bounds must be finite, ordered, and nonnegative")
    }
    if ((m.end_seconds - m.start_seconds) - m.duration_seconds).abs() > 0.01 {
        bail!("voice prosody span duration disagrees with its time bounds")
    }
    Ok(())
}

fn evaluate_measurement(span: &Compiled, m: &ProsodyMeasurement) -> ProsodySpanEvidence {
    let first_middle = semitones(m.first_f0_hz, m.middle_f0_hz);
    let middle_final = semitones(m.middle_f0_hz, m.final_f0_hz);
    let first_final = semitones(m.first_f0_hz, m.final_f0_hz);
    let detected = detect_contour(first_middle, middle_final, first_final);
    let pitch_match = span
        .requested
        .pitch_contour
        .as_deref()
        .map(|requested| requested == detected);
    let terminal_match = span
        .requested
        .terminal_boundary
        .as_deref()
        .and_then(|requested| match requested {
            "decisive-fall" => Some(middle_final <= -0.5),
            "question-rise" => Some(middle_final >= 0.5),
            "open" | "suspended" => None,
            _ => None,
        });
    let relative_match = span
        .requested
        .relative_pitch_target_semitones
        .as_ref()
        .map(|target| {
            let observed = [0.0, first_middle, first_final];
            let requested = [target.start, target.middle, target.end];
            let offset = requested[0];
            observed
                .iter()
                .zip(requested.iter())
                .all(|(actual, expected)| (actual - (expected - offset)).abs() <= 1.0)
        });
    let checks = [pitch_match, terminal_match, relative_match]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let measurement_reliable = m.voiced_frame_coverage >= 0.25 && m.duration_seconds >= 0.2;
    let status = if !measurement_reliable || checks.iter().any(|matched| !matched) {
        "failed"
    } else if checks.is_empty() {
        "advisory-only"
    } else {
        "passed"
    };
    ProsodySpanEvidence {
        span_id: m.span_id.clone(),
        start_seconds: r3(m.start_seconds),
        end_seconds: r3(m.end_seconds),
        median_f0_hz: r3(m.median_f0_hz),
        first_f0_hz: r3(m.first_f0_hz),
        middle_f0_hz: r3(m.middle_f0_hz),
        final_f0_hz: r3(m.final_f0_hz),
        first_to_middle_semitones: r3(first_middle),
        middle_to_final_semitones: r3(middle_final),
        first_to_final_semitones: r3(first_final),
        voiced_frame_coverage: r3(m.voiced_frame_coverage),
        duration_seconds: r3(m.duration_seconds),
        measurement_reliable,
        detected_contour: detected.into(),
        requested_pitch_contour: span.requested.pitch_contour.clone(),
        pitch_contour_match: pitch_match,
        requested_terminal_boundary: span.requested.terminal_boundary.clone(),
        terminal_boundary_match: terminal_match,
        relative_pitch_target_match: relative_match,
        status: status.into(),
    }
}

fn semitones(from: f64, to: f64) -> f64 {
    12.0 * (to / from).log2()
}

fn detect_contour(first_middle: f64, middle_final: f64, first_final: f64) -> &'static str {
    const THRESHOLD: f64 = 0.5;
    if first_middle >= THRESHOLD && middle_final <= -THRESHOLD {
        "rise-fall"
    } else if first_middle <= -THRESHOLD && middle_final >= THRESHOLD {
        "fall-rise"
    } else if first_final >= THRESHOLD {
        "rising"
    } else if first_final <= -THRESHOLD {
        "falling"
    } else {
        "level"
    }
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
        assert_eq!(
            plan.spans[4].requested.pitch_contour.as_deref(),
            Some("falling")
        );
        assert_eq!(
            plan.spans[4].requested.terminal_boundary.as_deref(),
            Some("decisive-fall")
        );
        assert!(
            plan.spans[4]
                .execution
                .advisory_only
                .contains(&"pitch_contour".to_string())
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

    #[test]
    fn prosody_evidence_detects_requested_fall_and_rechecks() {
        let d = tempfile::tempdir().unwrap();
        let packet = d.path().join("packet");
        write_plan(Options {
            manifest: Path::new(M),
            performance: Path::new(P),
            engine: VoiceEngine::Indextts25,
            engine_version: "2.5",
            reference_audio: None,
            seed: 7,
            output_dir: &packet,
        })
        .unwrap();
        let plan_hash = hash(&fs::read(packet.join("plan.json")).unwrap());
        let audio = d.path().join("rendered.wav");
        fs::write(&audio, b"sanitized fixture audio bytes").unwrap();
        let measurements = d.path().join("measurements.yaml");
        fs::write(
            &measurements,
            passing_measurements(&plan_hash, &production::sha256_path(&audio).unwrap()),
        )
        .unwrap();
        let evidence_dir = d.path().join("evidence");
        let result = write_prosody_evidence(ProsodyOptions {
            packet_dir: &packet,
            measurements: &measurements,
            rendered_audio: &audio,
            output_dir: &evidence_dir,
        })
        .unwrap();
        assert!(result.passed);
        assert!(
            check_prosody_evidence(&evidence_dir, &packet, &measurements, &audio)
                .unwrap()
                .verified
        );
        let evidence: ProsodyEvidence =
            serde_json::from_slice(&fs::read(evidence_dir.join("evidence.json")).unwrap()).unwrap();
        assert_eq!(evidence.spans[4].detected_contour, "falling");
        assert_eq!(evidence.spans[4].pitch_contour_match, Some(true));
        assert_eq!(evidence.spans[4].terminal_boundary_match, Some(true));
        assert_eq!(evidence.spans[4].status, "passed");
    }

    #[test]
    fn prosody_evidence_keeps_rising_terminal_as_visible_failure() {
        let d = tempfile::tempdir().unwrap();
        let packet = d.path().join("packet");
        write_plan(Options {
            manifest: Path::new(M),
            performance: Path::new(P),
            engine: VoiceEngine::Generic,
            engine_version: "fixture",
            reference_audio: None,
            seed: 9,
            output_dir: &packet,
        })
        .unwrap();
        let plan_hash = hash(&fs::read(packet.join("plan.json")).unwrap());
        let audio = d.path().join("rendered.wav");
        fs::write(&audio, b"sanitized failed fixture audio bytes").unwrap();
        let measurements = d.path().join("measurements.yaml");
        let text = passing_measurements(&plan_hash, &production::sha256_path(&audio).unwrap())
            .replace(
                "middle_f0_hz: 188\n    final_f0_hz: 168",
                "middle_f0_hz: 188\n    final_f0_hz: 242",
            );
        fs::write(&measurements, text).unwrap();
        let evidence_dir = d.path().join("evidence");
        let result = write_prosody_evidence(ProsodyOptions {
            packet_dir: &packet,
            measurements: &measurements,
            rendered_audio: &audio,
            output_dir: &evidence_dir,
        })
        .unwrap();
        assert!(!result.passed);
        let evidence: ProsodyEvidence =
            serde_json::from_slice(&fs::read(evidence_dir.join("evidence.json")).unwrap()).unwrap();
        assert_eq!(evidence.spans[4].detected_contour, "fall-rise");
        assert_eq!(evidence.spans[4].pitch_contour_match, Some(false));
        assert_eq!(evidence.spans[4].terminal_boundary_match, Some(false));
        assert_eq!(evidence.spans[4].status, "failed");
    }

    #[test]
    fn prosody_evidence_rejects_low_coverage_as_unreliable() {
        let d = tempfile::tempdir().unwrap();
        let packet = d.path().join("packet");
        write_plan(Options {
            manifest: Path::new(M),
            performance: Path::new(P),
            engine: VoiceEngine::Generic,
            engine_version: "fixture",
            reference_audio: None,
            seed: 10,
            output_dir: &packet,
        })
        .unwrap();
        let audio = d.path().join("rendered.wav");
        fs::write(&audio, b"sanitized low coverage fixture").unwrap();
        let plan_hash = hash(&fs::read(packet.join("plan.json")).unwrap());
        let measurements = d.path().join("measurements.yaml");
        let text = passing_measurements(&plan_hash, &production::sha256_path(&audio).unwrap())
            .replacen(
                "voiced_frame_coverage: 0.8",
                "voiced_frame_coverage: 0.1",
                1,
            );
        fs::write(&measurements, text).unwrap();
        let evidence_dir = d.path().join("evidence");
        let result = write_prosody_evidence(ProsodyOptions {
            packet_dir: &packet,
            measurements: &measurements,
            rendered_audio: &audio,
            output_dir: &evidence_dir,
        })
        .unwrap();
        assert!(!result.passed);
        let evidence: ProsodyEvidence =
            serde_json::from_slice(&fs::read(evidence_dir.join("evidence.json")).unwrap()).unwrap();
        assert!(!evidence.spans[0].measurement_reliable);
        assert_eq!(evidence.spans[0].status, "failed");
    }

    fn passing_measurements(plan_hash: &str, audio_hash: &str) -> String {
        format!(
            "schema: {PROSODY_MEASUREMENTS_SCHEMA}\nplan_sha256: {plan_hash}\nrendered_audio_sha256: {audio_hash}\nanalyzer: sanitized-pyin\nanalyzer_version: fixture-1\nspans:\n  - span_id: neutral-setup\n    start_seconds: 0\n    end_seconds: 1\n    median_f0_hz: 200\n    first_f0_hz: 200\n    middle_f0_hz: 200\n    final_f0_hz: 200\n    voiced_frame_coverage: 0.8\n    duration_seconds: 1\n  - span_id: explosive-interruption\n    start_seconds: 1\n    end_seconds: 2\n    median_f0_hz: 220\n    first_f0_hz: 200\n    middle_f0_hz: 260\n    final_f0_hz: 170\n    voiced_frame_coverage: 0.8\n    duration_seconds: 1\n  - span_id: fear-warning\n    start_seconds: 2\n    end_seconds: 3\n    median_f0_hz: 220\n    first_f0_hz: 200\n    middle_f0_hz: 250\n    final_f0_hz: 180\n    voiced_frame_coverage: 0.8\n    duration_seconds: 1\n  - span_id: suspense-build\n    start_seconds: 3\n    end_seconds: 4\n    median_f0_hz: 190\n    first_f0_hz: 180\n    middle_f0_hz: 190\n    final_f0_hz: 210\n    voiced_frame_coverage: 0.8\n    duration_seconds: 1\n  - span_id: decisive-action\n    start_seconds: 4\n    end_seconds: 5\n    median_f0_hz: 188\n    first_f0_hz: 200\n    middle_f0_hz: 188\n    final_f0_hz: 168\n    voiced_frame_coverage: 0.8\n    duration_seconds: 1\n  - span_id: comic-button\n    start_seconds: 5\n    end_seconds: 6\n    median_f0_hz: 180\n    first_f0_hz: 190\n    middle_f0_hz: 180\n    final_f0_hz: 160\n    voiced_frame_coverage: 0.8\n    duration_seconds: 1\n"
        )
    }
}
