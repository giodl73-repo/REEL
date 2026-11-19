#[path = "support/film_fixture.rs"]
mod support;
use reel_assembly::scene_authoring::NativeEventSpan;
use reel_workflow::*;
use serde_json::json;
use std::collections::BTreeMap;

#[test]
fn directed_controls_keep_semantic_cuts_and_forecast_each_language_separately() {
    for (lang, frames, desired) in [("es", 24, 3), ("en", 36, 5)] {
        let mut f = support::Fixture::new(lang);
        let legacy = film::generate(&f.packet.clone(), &mut f).unwrap();
        let key = f.packet.task_key.clone();
        f.add_direction(support::direction_settings());
        assert_ne!(f.packet.task_key, key);
        let generated = film::generate(&f.packet.clone(), &mut f).unwrap();
        assert_eq!(generated.outputs.len(), 6);
        for name in film::OUTPUTS {
            assert_eq!(legacy.files[name], generated.files[name]);
        }
        let preview: serde_json::Value = decode(&generated.files[film::DIRECTION_OUTPUT]).unwrap();
        assert_eq!(preview["delivery_frames"], frames);
        assert_eq!(preview["desired_composition_count"], desired);
        assert_eq!(preview["actual_composition_count"], 2);
        assert_eq!(preview["additional_candidates_upper_bound"], desired - 2);
        assert_eq!(preview["settings"]["visual_school"], "realistic");
        assert_eq!(preview["settings"]["performance_style"], "theatrical");
        assert_eq!(preview["rendered"], false);
        assert_eq!(preview["exposure_asset_binding_required"], false);
        assert_eq!(
            film::generate(&f.packet.clone(), &mut f).unwrap().files,
            generated.files
        );
        let request: film::Request =
            decode(&support::Fixture::new(lang).values["request.json"].1).unwrap();
        assert!(
            serde_json::to_value(request)
                .unwrap()
                .get("direction_path")
                .is_none()
        );
    }
}
#[test]
fn drawing_cadence_changes_demand_without_changing_delivery_rate_or_cuts() {
    let mut f = support::Fixture::new("es");
    let mut settings = support::direction_settings();
    settings["cadence"] = json!({"kind":"every-frames","frames":2});
    f.add_direction(settings);
    let generated = film::generate(&f.packet.clone(), &mut f).unwrap();
    let preview: serde_json::Value = decode(&generated.files[film::DIRECTION_OUTPUT]).unwrap();
    assert_eq!(preview["delivery_frames"], 24);
    assert_eq!(preview["actual_run_drawing_slots"], 13);
    assert_eq!(preview["target_drawing_slots_upper_bound"], 14);
    assert_eq!(preview["additional_candidates_upper_bound"], 12);
    assert_eq!(preview["estimated_generation_cost_microunits"], 84);
    assert_eq!(preview["exposure_asset_binding_required"], true);
    assert_eq!(preview["runs"][0]["end_frame"], 7);
    assert_eq!(preview["runs"][1]["start_frame"], 7);
    f.change_json("direction-settings.json", |v| {
        v["budget"]["drawing_slots"] = json!(13)
    });
    assert_eq!(
        film::generate(&f.packet.clone(), &mut f)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Budget
    );
}
fn picture(id: &str) -> Artifact {
    Artifact {
        logical_id: id.into(),
        sha256: digest(b"same immutable drawing"),
        bytes: 22,
        media_type: "image/png".into(),
    }
}
#[test]
fn adjacent_aliases_across_different_cue_clocks_cannot_evade_hold_policy() {
    let mut settings: direction::Settings =
        decode(&serde_json::to_vec(&support::direction_settings()).unwrap()).unwrap();
    let spans = vec![
        NativeEventSpan {
            event_id: "one".into(),
            cue_id: "first".into(),
            start_sample: 0,
            end_sample: 6000,
            sample_rate: 1000,
        },
        NativeEventSpan {
            event_id: "two".into(),
            cue_id: "second".into(),
            start_sample: 0,
            end_sample: 12000,
            sample_rate: 2000,
        },
    ];
    let pictures = BTreeMap::from([
        ("one".into(), picture("drawing/one")),
        ("two".into(), picture("drawing/alias")),
    ]);
    assert_eq!(
        direction::preview(&settings, "es", &spans, &pictures)
            .unwrap_err()
            .code,
        "film-direction-composition-policy"
    );
    settings.policy.hard_unchanged_composition_seconds_max = 20.0;
    settings.budget.raw_rgb_bytes = 16 * 1024 * 1024 * 1024;
    let preview = direction::preview(&settings, "es", &spans, &pictures).unwrap();
    assert_eq!(preview.sample_rate, 2000);
    assert_eq!(preview.duration_samples, 24000);
    assert_eq!(preview.delivery_frames, 288);
    assert_eq!(preview.actual_composition_count, 1);
    assert_eq!(preview.unique_selected_drawings, 1);
    let mut impossible_clock = spans.clone();
    impossible_clock[0].sample_rate = 383999;
    impossible_clock[0].end_sample = 383999;
    impossible_clock[1].sample_rate = 383997;
    impossible_clock[1].end_sample = 383997;
    assert_eq!(
        direction::preview(&settings, "es", &impossible_clock, &pictures)
            .unwrap_err()
            .code,
        "film-direction-common-clock"
    );
    let mut gap = spans;
    gap[1].start_sample = 1;
    assert_eq!(
        direction::preview(&settings, "es", &gap, &pictures)
            .unwrap_err()
            .code,
        "film-direction-cue-order"
    );
}
#[test]
fn invalid_or_unbound_settings_and_over_budget_density_refuse_before_controls_return() {
    for case in 0..7 {
        let mut f = support::Fixture::new("es");
        let mut settings = support::direction_settings();
        match case {
            0 => settings["desired_seconds_per_cel"]["denominator"] = json!(0),
            1 => settings["policy"]["policy_id"] = json!("foreign-policy"),
            2 => settings["budget"]["raw_rgb_bytes"] = json!(1),
            3 => settings["references"] = json!([picture("not/declared")]),
            4 => {
                settings["desired_seconds_per_cel"] = json!({"numerator":1,"denominator":100});
                settings["policy"]["target_composition_seconds_min"] = json!(0.001);
            }
            5 => settings["cadence"] = json!({"kind":"every-frames","frames":0}),
            _ => settings["budget"]["cost"]["microunits_per_candidate"] = json!(u64::MAX),
        }
        // One extra candidate with u64::MAX cost is representable; request more.
        if case == 6 {
            settings["cadence"] = json!({"kind":"every-frames","frames":2});
        }
        f.add_direction(settings);
        assert!(
            film::generate(&f.packet.clone(), &mut f).is_err(),
            "case {case}"
        );
    }
    let mut f = support::Fixture::new("es");
    f.add_direction(support::direction_settings());
    f.change_json("request.json", |v| {
        v["schema"] = json!("reel.workflow-film-scene-control.v1")
    });
    assert!(film::generate(&f.packet.clone(), &mut f).is_err());
}
