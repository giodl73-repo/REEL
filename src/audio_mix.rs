use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::production::{
    AudioDuckingPolicy, AudioRole, GainAutomationPoint, GainCurve, ProductionManifest,
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
}

#[derive(Clone, Debug)]
pub struct StemLabels {
    pub dialogue: String,
    pub music: String,
    pub effects: String,
    pub pre_master: String,
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
        return compile_legacy(
            manifest,
            timeline_seconds,
            filters,
            by_role,
            resolved_automation,
        );
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
        })
        .collect::<Vec<_>>();

    let mut stem_role_labels = BTreeMap::new();
    let mut mix_role_labels = BTreeMap::new();
    for (role, label) in role_labels {
        if include_stems {
            let stem = format!("{}_stem", role_name(role));
            let mix = format!("{}_mix", role_name(role));
            filters.push(format!("[{label}]asplit=2[{stem}][{mix}]"));
            stem_role_labels.insert(role, stem);
            mix_role_labels.insert(role, mix);
        } else {
            mix_role_labels.insert(role, label);
        }
    }
    let stems = if include_stems {
        Some(compile_stems(
            &mut filters,
            &stem_role_labels,
            timeline_seconds,
            sample_rate_hz,
            channels,
        ))
    } else {
        None
    };

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
        ducked_groups.push(output);
    }
    let mut final_components = programs
        .into_iter()
        .filter(|(role, _)| !targeted.contains(role))
        .map(|(_, label)| label)
        .collect::<Vec<_>>();
    final_components.extend(ducked_groups);
    let premaster = "mixedaudio";
    mix_labels(&mut filters, &final_components, premaster);
    let mastering = mastering_filter(manifest);
    filters.push(format!(
        "[{premaster}]aresample=async=1:first_pts=0,apad{mastering},atrim=duration={timeline_seconds:.3}[finala]"
    ));
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
    by_role: BTreeMap<AudioRole, Vec<String>>,
    resolved_automation: Vec<ResolvedGainAutomation>,
) -> Result<CompiledAudioMix> {
    let narration = by_role
        .get(&AudioRole::Narration)
        .cloned()
        .unwrap_or_default();
    let background = by_role
        .into_iter()
        .filter(|(role, _)| *role != AudioRole::Narration)
        .flat_map(|(_, labels)| labels)
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
    roles: &BTreeMap<AudioRole, String>,
    timeline_seconds: f64,
    sample_rate_hz: u32,
    channels: u8,
) -> StemLabels {
    let layout = if channels == 1 { "mono" } else { "stereo" };
    let mut group = |name: &str, members: &[AudioRole]| {
        let labels = members
            .iter()
            .filter_map(|role| roles.get(role).cloned())
            .collect::<Vec<_>>();
        if labels.is_empty() {
            filters.push(format!("anullsrc=r={sample_rate_hz}:cl={layout},atrim=duration={timeline_seconds:.3}[{name}_group]"));
        } else {
            mix_labels(filters, &labels, &format!("{name}_group"));
        }
        filters.push(format!("[{name}_group]asplit=2[{name}_out][{name}_sum]"));
        format!("{name}_out")
    };
    let dialogue = group("stem_d", &[AudioRole::Narration, AudioRole::Dialogue]);
    let music = group("stem_m", &[AudioRole::Music]);
    let effects = group("stem_e", &[AudioRole::Ambience, AudioRole::Effect]);
    filters.push("[stem_d_sum][stem_m_sum][stem_e_sum]amix=inputs=3:normalize=0:dropout_transition=0[stem_premaster]".into());
    StemLabels {
        dialogue,
        music,
        effects,
        pre_master: "stem_premaster".into(),
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
