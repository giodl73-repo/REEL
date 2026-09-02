use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::production::{
    AudioDuckingPolicy, AudioRole, DynamicEqPolicy, GainAutomationPoint, GainCurve,
    ProductionManifest,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ResolvedGainPoint {
    pub time_ms: u64,
    pub gain_db: f64,
    pub curve: GainCurve,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ResolvedGainAutomation {
    pub event_id: String,
    pub points: Vec<ResolvedGainPoint>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CompiledDuckingPolicy {
    pub id: String,
    pub detector_roles: Vec<AudioRole>,
    pub target_roles: Vec<AudioRole>,
    pub threshold: f64,
    pub ratio: f64,
    pub max_reduction_db: Option<f64>,
    pub attack_ms: u64,
    pub release_ms: u64,
    pub dynamic_eq_render_supported: bool,
    pub dynamic_eq: Option<DynamicEqPolicy>,
}

#[derive(Clone, Debug)]
pub struct StemLabels {
    pub dialogue: String,
    pub music: String,
    pub effects: String,
    pub pre_master: String,
    pub no_score: String,
    pub mono_review: String,
    pub small_speaker_review: String,
}

#[derive(Clone, Debug)]
pub struct CompiledAudioMix {
    pub filters: Vec<String>,
    pub final_label: String,
    pub stems: Option<StemLabels>,
    pub resolved_automation: Vec<ResolvedGainAutomation>,
    pub ducking: Vec<CompiledDuckingPolicy>,
    pub dynamic_eq_render_supported: bool,
}

pub fn compile(
    manifest: &ProductionManifest,
    timeline_seconds: f64,
    input_offset: usize,
    include_stems: bool,
    sample_rate_hz: u32,
    channels: u8,
) -> Result<CompiledAudioMix> {
    if manifest.audio_events.is_empty() {
        bail!("audio mix compilation requires audio events");
    }
    let marker_times = manifest
        .beat_markers
        .iter()
        .map(|marker| (marker.id.as_str(), seconds_to_ms(marker.time_seconds)))
        .collect::<BTreeMap<_, _>>();
    let mut filters = Vec::new();
    let mut by_role = BTreeMap::<AudioRole, Vec<String>>::new();
    let mut resolved_automation = Vec::new();
    for (index, event) in manifest.audio_events.iter().enumerate() {
        let duration = event
            .duration_seconds
            .unwrap_or(timeline_seconds - event.start_seconds);
        let points = resolve_automation(
            &event.gain_automation,
            event.start_seconds,
            duration,
            &marker_times,
        )?;
        let gain = if points.is_empty() {
            format!("volume={:.3}dB", event.gain_db)
        } else {
            let expression = gain_expression(event.gain_db, &points);
            resolved_automation.push(ResolvedGainAutomation {
                event_id: event.id.clone(),
                points: points.clone(),
            });
            format!("volume='{expression}':eval=frame")
        };
        let input_index = input_offset + index;
        let mut chain = format!(
            "[{input_index}:a:0]atrim=start={:.3}:duration={duration:.3},asetpts=PTS-STARTPTS,{gain}",
            event.source_in_seconds
        );
        if event.fade_in_ms > 0 {
            chain.push_str(&format!(
                ",afade=t=in:st=0:d={:.3}",
                event.fade_in_ms as f64 / 1000.0
            ));
        }
        if event.fade_out_ms > 0 {
            let fade = event.fade_out_ms as f64 / 1000.0;
            chain.push_str(&format!(
                ",afade=t=out:st={:.3}:d={fade:.3}",
                (duration - fade).max(0.0)
            ));
        }
        chain.push_str(&format!(
            ",adelay={}:all=1[ae{index}]",
            seconds_to_ms(event.start_seconds)
        ));
        filters.push(chain);
        by_role
            .entry(event.role)
            .or_default()
            .push(format!("ae{index}"));
    }

    let legacy_only = !include_stems
        && manifest.audio_ducking.is_empty()
        && manifest
            .audio_events
            .iter()
            .all(|event| event.gain_automation.is_empty())
        && !manifest
            .audio_events
            .iter()
            .any(|event| event.role == AudioRole::Dialogue);
    if legacy_only {
        return compile_legacy(manifest, timeline_seconds, filters, resolved_automation);
    }

    let mut role_labels = BTreeMap::new();
    for (role, labels) in &by_role {
        let output = format!("role_{}", role_name(*role));
        mix_labels(&mut filters, labels, &output);
        role_labels.insert(*role, output);
    }

    let policies = normalized_policies(manifest);
    let dynamic_eq_render_supported = policies.iter().all(|(_, dynamic)| !*dynamic);
    let compiled_policies = policies
        .iter()
        .map(|(policy, dynamic)| CompiledDuckingPolicy {
            id: policy.id.clone(),
            detector_roles: policy.detector_roles.clone(),
            target_roles: policy.target_roles.clone(),
            threshold: policy.threshold,
            ratio: policy.ratio,
            max_reduction_db: if policy.id == "legacy-narration-ducking" {
                None
            } else {
                Some(policy.max_reduction_db)
            },
            attack_ms: policy.attack_ms,
            release_ms: policy.release_ms,
            dynamic_eq_render_supported: !*dynamic,
            dynamic_eq: policy.dynamic_eq.clone(),
        })
        .collect::<Vec<_>>();

    let mix_role_labels = role_labels;

    let mut detector_uses = BTreeMap::<AudioRole, usize>::new();
    for (policy, _) in &policies {
        for role in &policy.detector_roles {
            *detector_uses.entry(*role).or_default() += 1;
        }
    }
    let mut programs = BTreeMap::new();
    let mut detectors = BTreeMap::<(AudioRole, usize), String>::new();
    for (role, label) in mix_role_labels {
        let uses = detector_uses.get(&role).copied().unwrap_or(0);
        if uses == 0 {
            programs.insert(role, label);
            continue;
        }
        let program = format!("{}_program", role_name(role));
        let detector_labels = (0..uses)
            .map(|index| format!("{}_detector_{index}", role_name(role)))
            .collect::<Vec<_>>();
        let outputs = std::iter::once(program.clone())
            .chain(detector_labels.iter().cloned())
            .map(|value| format!("[{value}]"))
            .collect::<String>();
        filters.push(format!("[{label}]asplit={}{outputs}", uses + 1));
        programs.insert(role, program);
        for (index, value) in detector_labels.into_iter().enumerate() {
            detectors.insert((role, index), value);
        }
    }

    let mut role_detector_cursor = BTreeMap::<AudioRole, usize>::new();
    let mut ducked_groups = Vec::new();
    let mut targeted = BTreeSet::new();
    for (policy_index, (policy, has_dynamic_eq)) in policies.iter().enumerate() {
        if *has_dynamic_eq {
            continue;
        }
        let mut detector_labels = Vec::new();
        for role in &policy.detector_roles {
            let cursor = role_detector_cursor.entry(*role).or_default();
            detector_labels.push(detectors[&(*role, *cursor)].clone());
            *cursor += 1;
        }
        let detector = format!("duck_detector_{policy_index}");
        mix_labels(&mut filters, &detector_labels, &detector);
        let target_labels = policy
            .target_roles
            .iter()
            .map(|role| programs[role].clone())
            .collect::<Vec<_>>();
        let target = format!("duck_target_{policy_index}");
        mix_labels(&mut filters, &target_labels, &target);
        let output = format!("ducked_{policy_index}");
        if policy.id == "legacy-narration-ducking" {
            filters.push(format!(
                "[{target}][{detector}]sidechaincompress=threshold={:.6}:ratio={:.3}:attack={}:release={}[{output}]",
                policy.threshold, policy.ratio, policy.attack_ms, policy.release_ms
            ));
        } else {
            let floor = 10f64.powf(-policy.max_reduction_db / 20.0);
            filters.push(format!(
                "[{target}]asplit=2[duck_dry_{policy_index}][duck_input_{policy_index}]"
            ));
            filters.push(format!(
                "[duck_input_{policy_index}][{detector}]sidechaincompress=threshold={:.6}:ratio={:.3}:attack={}:release={}[duck_compressed_{policy_index}]",
                policy.threshold, policy.ratio, policy.attack_ms, policy.release_ms
            ));
            filters.push(format!(
                "[duck_compressed_{policy_index}]volume={:.9}[duck_wet_{policy_index}]",
                1.0 - floor
            ));
            filters.push(format!(
                "[duck_dry_{policy_index}]volume={floor:.9}[duck_floor_{policy_index}]"
            ));
            filters.push(format!("[duck_wet_{policy_index}][duck_floor_{policy_index}]amix=inputs=2:normalize=0:dropout_transition=0[{output}]"));
        }
        targeted.extend(policy.target_roles.iter().copied());
        ducked_groups.push((stem_group(policy.target_roles[0]), output));
    }
    let mut final_components = programs
        .into_iter()
        .filter(|(role, _)| !targeted.contains(role))
        .map(|(role, label)| (stem_group(role), label))
        .collect::<Vec<_>>();
    final_components.extend(ducked_groups);
    let premaster = "mixedaudio";
    let stems = if include_stems {
        Some(compile_stems(
            &mut filters,
            &final_components,
            timeline_seconds,
            sample_rate_hz,
            channels,
            premaster,
        ))
    } else {
        mix_labels(
            &mut filters,
            &final_components
                .iter()
                .map(|(_, label)| label.clone())
                .collect::<Vec<_>>(),
            premaster,
        );
        None
    };
    let mastering = mastering_filter(manifest);
    let mastered_label = if include_stems {
        "mastered_base"
    } else {
        "finala"
    };
    filters.push(format!(
        "[{premaster}]aresample=async=1:first_pts=0,apad{mastering},atrim=duration={timeline_seconds:.3}[{mastered_label}]"
    ));
    if include_stems {
        filters.push(
            "[mastered_base]asplit=3[finala][review_mono_source][review_small_source]".into(),
        );
        let downmix = if channels == 1 {
            "anull".to_string()
        } else {
            "pan=mono|c0=0.5*c0+0.5*c1".to_string()
        };
        filters.push(format!("[review_mono_source]{downmix}[review_mono]"));
        filters.push(format!(
            "[review_small_source]{downmix},highpass=f=180,lowpass=f=5500[review_small_speaker]"
        ));
    }
    Ok(CompiledAudioMix {
        filters,
        final_label: "finala".into(),
        stems,
        resolved_automation,
        ducking: compiled_policies,
        dynamic_eq_render_supported,
    })
}

fn compile_legacy(
    manifest: &ProductionManifest,
    timeline_seconds: f64,
    mut filters: Vec<String>,
    resolved_automation: Vec<ResolvedGainAutomation>,
) -> Result<CompiledAudioMix> {
    let narration = manifest
        .audio_events
        .iter()
        .enumerate()
        .filter(|(_, event)| event.role == AudioRole::Narration)
        .map(|(index, _)| format!("ae{index}"))
        .collect::<Vec<_>>();
    let background = manifest
        .audio_events
        .iter()
        .enumerate()
        .filter(|(_, event)| event.role != AudioRole::Narration)
        .map(|(index, _)| format!("ae{index}"))
        .collect::<Vec<_>>();
    let mixed = match (background.is_empty(), narration.is_empty()) {
        (false, false) => {
            mix_labels(&mut filters, &background, "background");
            mix_labels(&mut filters, &narration, "narration");
            if let Some(ducking) = &manifest.narration_ducking {
                filters.push("[narration]asplit=2[narration_detector][narration_program]".into());
                filters.push(format!(
                    "[background][narration_detector]sidechaincompress=threshold={:.6}:ratio={:.3}:attack={}:release={}[ducked]",
                    ducking.threshold, ducking.ratio, ducking.attack_ms, ducking.release_ms
                ));
                filters.push("[ducked][narration_program]amix=inputs=2:normalize=0:dropout_transition=0[mixedaudio]".into());
            } else {
                filters.push("[background][narration]amix=inputs=2:normalize=0:dropout_transition=0[mixedaudio]".into());
            }
            "mixedaudio"
        }
        (false, true) => {
            mix_labels(&mut filters, &background, "mixedaudio");
            "mixedaudio"
        }
        (true, false) => {
            mix_labels(&mut filters, &narration, "mixedaudio");
            "mixedaudio"
        }
        (true, true) => unreachable!(),
    };
    let mastering = mastering_filter(manifest);
    filters.push(format!("[{mixed}]aresample=async=1:first_pts=0,apad{mastering},atrim=duration={timeline_seconds:.3}[finala]"));
    Ok(CompiledAudioMix {
        filters,
        final_label: "finala".into(),
        stems: None,
        resolved_automation,
        ducking: vec![],
        dynamic_eq_render_supported: true,
    })
}

fn compile_stems(
    filters: &mut Vec<String>,
    components: &[(StemGroup, String)],
    timeline_seconds: f64,
    sample_rate_hz: u32,
    channels: u8,
    premaster: &str,
) -> StemLabels {
    let layout = if channels == 1 { "mono" } else { "stereo" };
    let mut group = |name: &str, wanted: StemGroup| {
        let labels = components
            .iter()
            .filter(|(group, _)| *group == wanted)
            .map(|(_, label)| label.clone())
            .collect::<Vec<_>>();
        if labels.is_empty() {
            filters.push(format!("anullsrc=r={sample_rate_hz}:cl={layout},atrim=duration={timeline_seconds:.3}[{name}_raw]"));
        } else {
            mix_labels(filters, &labels, &format!("{name}_raw"));
        }
        filters.push(format!(
            "[{name}_raw]aresample={sample_rate_hz}:async=0,apad,atrim=duration={timeline_seconds:.3},aformat=sample_rates={sample_rate_hz}:channel_layouts={layout}[{name}_group]"
        ));
        let split = if wanted == StemGroup::Music { 2 } else { 3 };
        let review = if split == 3 {
            format!("[{name}_no_score]")
        } else {
            String::new()
        };
        filters.push(format!(
            "[{name}_group]asplit={split}[{name}_out][{name}_sum]{review}"
        ));
        format!("{name}_out")
    };
    let dialogue = group("stem_d", StemGroup::Dialogue);
    let music = group("stem_m", StemGroup::Music);
    let effects = group("stem_e", StemGroup::Effects);
    filters.push("[stem_d_sum][stem_m_sum][stem_e_sum]amix=inputs=3:normalize=0:dropout_transition=0[stem_premaster_base]".into());
    filters.push(format!(
        "[stem_premaster_base]asplit=2[stem_premaster][{premaster}]"
    ));
    filters.push("[stem_d_no_score][stem_e_no_score]amix=inputs=2:normalize=0:dropout_transition=0[review_no_score]".into());
    StemLabels {
        dialogue,
        music,
        effects,
        pre_master: "stem_premaster".into(),
        no_score: "review_no_score".into(),
        mono_review: "review_mono".into(),
        small_speaker_review: "review_small_speaker".into(),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StemGroup {
    Dialogue,
    Music,
    Effects,
}

fn stem_group(role: AudioRole) -> StemGroup {
    match role {
        AudioRole::Narration | AudioRole::Dialogue => StemGroup::Dialogue,
        AudioRole::Music => StemGroup::Music,
        AudioRole::Ambience | AudioRole::Effect => StemGroup::Effects,
    }
}

fn normalized_policies(manifest: &ProductionManifest) -> Vec<(AudioDuckingPolicy, bool)> {
    if !manifest.audio_ducking.is_empty() {
        return manifest
            .audio_ducking
            .iter()
            .cloned()
            .map(|policy| {
                let dynamic = policy.dynamic_eq.is_some();
                (policy, dynamic)
            })
            .collect();
    }
    manifest
        .narration_ducking
        .as_ref()
        .map(|legacy| {
            let targets = manifest
                .audio_events
                .iter()
                .map(|event| event.role)
                .filter(|role| *role != AudioRole::Narration)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            vec![(
                AudioDuckingPolicy {
                    id: "legacy-narration-ducking".into(),
                    detector_roles: vec![AudioRole::Narration],
                    target_roles: targets,
                    threshold: legacy.threshold,
                    ratio: legacy.ratio,
                    max_reduction_db: 60.0,
                    attack_ms: legacy.attack_ms,
                    release_ms: legacy.release_ms,
                    dynamic_eq: None,
                },
                false,
            )]
        })
        .unwrap_or_default()
}

fn resolve_automation(
    points: &[GainAutomationPoint],
    event_start_seconds: f64,
    duration_seconds: f64,
    markers: &BTreeMap<&str, u64>,
) -> Result<Vec<ResolvedGainPoint>> {
    let event_start_ms = seconds_to_ms(event_start_seconds);
    let duration_ms = seconds_to_ms(duration_seconds);
    points
        .iter()
        .map(|point| {
            let time_ms = if let Some(seconds) = point.time_seconds {
                seconds_to_ms(seconds)
            } else {
                let id = point
                    .beat_marker_id
                    .as_deref()
                    .ok_or_else(|| anyhow!("automation point lacks anchor"))?;
                markers
                    .get(id)
                    .copied()
                    .ok_or_else(|| anyhow!("unknown automation beat marker {id}"))?
                    .checked_sub(event_start_ms)
                    .ok_or_else(|| anyhow!("automation marker {id} precedes event"))?
            };
            if time_ms > duration_ms {
                bail!("automation point exceeds event duration");
            }
            Ok(ResolvedGainPoint {
                time_ms,
                gain_db: point.gain_db,
                curve: point.curve,
            })
        })
        .collect()
}

fn gain_expression(base_gain_db: f64, points: &[ResolvedGainPoint]) -> String {
    let mut value = format!("{:.9}", points.last().expect("points").gain_db);
    for pair in points.windows(2).rev() {
        let left = &pair[0];
        let right = &pair[1];
        let start = left.time_ms as f64 / 1000.0;
        let end = right.time_ms as f64 / 1000.0;
        let x = format!("((t-{start:.6})/{:.6})", end - start);
        let segment = match left.curve {
            GainCurve::Hold => format!("{:.9}", left.gain_db),
            GainCurve::Linear => format!(
                "({:.9}+({:.9}-{:.9})*{x})",
                left.gain_db, right.gain_db, left.gain_db
            ),
            GainCurve::Smooth => format!(
                "({:.9}+({:.9}-{:.9})*({x}*{x}*(3-2*{x})))",
                left.gain_db, right.gain_db, left.gain_db
            ),
        };
        value = format!("if(lt(t,{end:.6}),{segment},{value})");
    }
    let first = points.first().expect("points");
    let first_time = first.time_ms as f64 / 1000.0;
    if first_time > 0.0 {
        value = format!("if(lt(t,{first_time:.6}),{:.9},{value})", first.gain_db);
    }
    format!("pow(10,({base_gain_db:.9}+{value})/20)")
}

fn mastering_filter(manifest: &ProductionManifest) -> String {
    manifest
        .audio_mastering
        .as_ref()
        .map_or_else(String::new, |mastering| {
            format!(
                ",loudnorm=I={:.3}:LRA={:.3}:TP={:.3},alimiter=limit={:.3}:level=false",
                mastering.integrated_lufs,
                mastering.loudness_range_lu,
                mastering.true_peak_dbfs,
                mastering.limiter
            )
        })
}

fn mix_labels(filters: &mut Vec<String>, labels: &[String], output: &str) {
    let inputs = labels
        .iter()
        .map(|label| format!("[{label}]"))
        .collect::<String>();
    if labels.len() == 1 {
        filters.push(format!("{inputs}anull[{output}]"));
    } else {
        filters.push(format!(
            "{inputs}amix=inputs={}:normalize=0:dropout_transition=0[{output}]",
            labels.len()
        ));
    }
}

fn role_name(role: AudioRole) -> &'static str {
    match role {
        AudioRole::Music => "music",
        AudioRole::Ambience => "ambience",
        AudioRole::Effect => "effect",
        AudioRole::Narration => "narration",
        AudioRole::Dialogue => "dialogue",
    }
}

fn seconds_to_ms(seconds: f64) -> u64 {
    (seconds * 1000.0).round() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::production::{self, AudioDuckingPolicy, AudioEvent, BeatMarker, DynamicEqPolicy};
    use std::path::Path;

    fn manifest() -> ProductionManifest {
        production::load(Path::new(
            "manifests/fixtures/vertical-sound-off/manifest.yaml",
        ))
        .unwrap()
        .manifest
    }

    fn event(id: &str, role: AudioRole) -> AudioEvent {
        AudioEvent {
            id: id.into(),
            role,
            source: format!("{id}.wav"),
            start_seconds: 0.0,
            duration_seconds: Some(6.0),
            source_in_seconds: 0.0,
            gain_db: 0.0,
            loop_source: false,
            fade_in_ms: 0,
            fade_out_ms: 0,
            beat_marker_id: None,
            gain_automation: vec![],
        }
    }

    #[test]
    fn dialogue_routes_to_d_and_only_music_is_ducked() {
        let mut manifest = manifest();
        manifest.audio_events = vec![
            event("dialogue", AudioRole::Dialogue),
            event("music", AudioRole::Music),
            event("effect", AudioRole::Effect),
        ];
        manifest.audio_ducking = vec![AudioDuckingPolicy {
            id: "speech-over-score".into(),
            detector_roles: vec![AudioRole::Dialogue],
            target_roles: vec![AudioRole::Music],
            threshold: 0.03,
            ratio: 3.0,
            max_reduction_db: 6.0,
            attack_ms: 25,
            release_ms: 350,
            dynamic_eq: None,
        }];
        let compiled = compile(&manifest, 6.0, 0, true, 48_000, 2).unwrap();
        let graph = compiled.filters.join(";");
        assert!(graph.contains("[role_dialogue]asplit="));
        assert!(graph.contains("[role_music]anull[duck_target_0]"));
        assert!(graph.contains("[role_effect]"));
        assert!(graph.contains("volume=0.501187234[duck_floor_0]"));
        assert!(!graph.contains("[role_effect]anull[duck_target_0]"));
        assert_eq!(compiled.stems.unwrap().dialogue, "stem_d_out");
    }

    #[test]
    fn automation_resolves_local_and_beat_anchors_deterministically() {
        let mut manifest = manifest();
        manifest.beat_markers = vec![BeatMarker {
            id: "reaction".into(),
            time_seconds: 3.0,
            label: String::new(),
            accent: false,
        }];
        let mut score = event("score", AudioRole::Music);
        score.start_seconds = 1.0;
        score.duration_seconds = Some(4.0);
        score.gain_db = -10.0;
        score.gain_automation = vec![
            GainAutomationPoint {
                time_seconds: Some(0.5),
                beat_marker_id: None,
                gain_db: -4.0,
                curve: GainCurve::Smooth,
            },
            GainAutomationPoint {
                time_seconds: None,
                beat_marker_id: Some("reaction".into()),
                gain_db: 2.0,
                curve: GainCurve::Linear,
            },
        ];
        manifest.audio_events = vec![score];
        let compiled = compile(&manifest, 6.0, 0, false, 48_000, 2).unwrap();
        assert_eq!(compiled.resolved_automation[0].points[0].time_ms, 500);
        assert_eq!(compiled.resolved_automation[0].points[1].time_ms, 2_000);
        let graph = compiled.filters.join(";");
        assert!(graph.contains("eval=frame"));
        assert!(graph.contains("3-2*"));
    }

    #[test]
    fn dynamic_eq_is_a_complete_engine_neutral_plan_but_not_render_claim() {
        let mut manifest = manifest();
        manifest.audio_events = vec![
            event("dialogue", AudioRole::Dialogue),
            event("music", AudioRole::Music),
        ];
        manifest.audio_ducking = vec![AudioDuckingPolicy {
            id: "presence-carve".into(),
            detector_roles: vec![AudioRole::Dialogue],
            target_roles: vec![AudioRole::Music],
            threshold: 0.03,
            ratio: 3.0,
            max_reduction_db: 6.0,
            attack_ms: 25,
            release_ms: 350,
            dynamic_eq: Some(DynamicEqPolicy {
                frequency_hz: 2_500.0,
                q: 1.2,
                max_cut_db: 4.0,
                attack_ms: 20,
                release_ms: 200,
            }),
        }];
        let compiled = compile(&manifest, 6.0, 0, false, 48_000, 2).unwrap();
        assert!(!compiled.dynamic_eq_render_supported);
        let plan = compiled.ducking[0].dynamic_eq.as_ref().unwrap();
        assert_eq!(plan.frequency_hz, 2_500.0);
        assert_eq!(compiled.ducking[0].target_roles, vec![AudioRole::Music]);
    }

    #[test]
    fn legacy_graph_keeps_the_original_filter_shape() {
        let mut manifest = manifest();
        manifest.audio_events = vec![
            event("room", AudioRole::Ambience),
            event("voice", AudioRole::Narration),
            event("music", AudioRole::Music),
        ];
        manifest.narration_ducking = Some(production::NarrationDucking {
            threshold: 0.03,
            ratio: 8.0,
            attack_ms: 20,
            release_ms: 300,
        });
        let compiled = compile(&manifest, 6.0, 0, false, 48_000, 2).unwrap();
        let graph = compiled.filters.join(";");
        assert!(graph.contains("[background][narration_detector]sidechaincompress=threshold=0.030000:ratio=8.000:attack=20:release=300[ducked]"));
        assert!(
            graph.contains("[ae0][ae2]amix=inputs=2:normalize=0:dropout_transition=0[background]")
        );
        assert!(!graph.contains("role_"));
        assert!(compiled.ducking.is_empty());
    }

    #[test]
    fn validation_rejects_invalid_automation_anchors_and_times() {
        let fixture = Path::new("manifests/fixtures/vertical-sound-off/manifest.yaml");
        let mut loaded = production::load(fixture).unwrap();
        let mut score = event("score", AudioRole::Music);
        score.duration_seconds = Some(2.0);
        score.gain_automation = vec![GainAutomationPoint {
            time_seconds: Some(0.5),
            beat_marker_id: Some("hit".into()),
            gain_db: -3.0,
            curve: GainCurve::Hold,
        }];
        loaded.manifest.beat_markers = vec![BeatMarker {
            id: "hit".into(),
            time_seconds: 0.5,
            label: String::new(),
            accent: false,
        }];
        loaded.manifest.audio_events = vec![score.clone()];
        assert!(
            production::validate(&loaded)
                .unwrap_err()
                .to_string()
                .contains("exactly one")
        );

        score.gain_automation = vec![
            GainAutomationPoint {
                time_seconds: Some(0.5),
                beat_marker_id: None,
                gain_db: -3.0,
                curve: GainCurve::Linear,
            },
            GainAutomationPoint {
                time_seconds: Some(0.5),
                beat_marker_id: None,
                gain_db: -4.0,
                curve: GainCurve::Smooth,
            },
        ];
        loaded.manifest.audio_events = vec![score.clone()];
        assert!(
            production::validate(&loaded)
                .unwrap_err()
                .to_string()
                .contains("unique ascending")
        );

        score.gain_automation = vec![GainAutomationPoint {
            time_seconds: Some(2.1),
            beat_marker_id: None,
            gain_db: -3.0,
            curve: GainCurve::Linear,
        }];
        loaded.manifest.audio_events = vec![score];
        assert!(
            production::validate(&loaded)
                .unwrap_err()
                .to_string()
                .contains("outside the event")
        );
    }

    #[test]
    fn dialogue_role_deserializes_without_changing_narration() {
        assert_eq!(
            serde_yaml::from_str::<AudioRole>("dialogue").unwrap(),
            AudioRole::Dialogue
        );
        assert_eq!(
            serde_yaml::from_str::<AudioRole>("narration").unwrap(),
            AudioRole::Narration
        );
    }
}
