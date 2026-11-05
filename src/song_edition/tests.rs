use super::*;
use serde_json::json;

fn recipe() -> Recipe {
    Recipe {
        segments: vec![[0.0, 2.0], [3.0, 5.0]],
        crossfade_seconds: 0.25,
        fade_in_seconds: 0.125,
        fade_out_seconds: 0.125,
        max_duration_seconds: 4.0,
    }
}

#[test]
fn exact_clock_cores_edges_and_stereo() {
    let plan = plan(&recipe(), 8_000, 40_000).unwrap();
    assert_eq!(plan.output_frames, 30_000);
    assert_eq!(plan.segments[1].output_start, 14_000);
    assert_eq!(
        (plan.segments[0].core_start, plan.segments[0].core_end),
        (1000, 14000)
    );
    let source: Vec<f32> = (0..80_000)
        .map(|n| ((n % 997) as f32 - 498.0) / 1000.0)
        .collect();
    let mut output = render(&plan, &source, 2).unwrap();
    assert_eq!(output.len(), 60_000);
    assert_eq!(output[0], 0.0);
    assert_eq!(output[output.len() - 1], 0.0);
    assert_eq!(&output[2000..28000], &source[2000..28000]);
    assert_eq!(&output[32000..58000], &source[52000..78000]);
    output[2000] += 0.001;
    assert!(verify_cores(&plan, &source, &output, 2).is_err());
}

#[test]
fn sample_rounding_is_ties_even_and_one_sample_fades_work() {
    assert_eq!(frames(0.5 / 8000.0, 8000).unwrap(), 0);
    assert_eq!(frames(1.5 / 8000.0, 8000).unwrap(), 2);
    assert_eq!(frames(2.5 / 8000.0, 8000).unwrap(), 2);
    let r = Recipe {
        segments: vec![[0.0, 0.001]],
        crossfade_seconds: 0.0,
        fade_in_seconds: 1.0 / 8000.0,
        fade_out_seconds: 1.0 / 8000.0,
        max_duration_seconds: 1.0,
    };
    let p = plan(&r, 8000, 8).unwrap();
    assert_eq!(
        render(&p, &[0.5; 8], 1).unwrap(),
        vec![0.0, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.0]
    );
}

#[test]
fn crossfade_and_edge_envelopes_match_a_hand_computed_fixture() {
    let r = Recipe {
        segments: vec![[0.0, 8.0 / 8000.0], [12.0 / 8000.0, 20.0 / 8000.0]],
        crossfade_seconds: 4.0 / 8000.0,
        fade_in_seconds: 2.0 / 8000.0,
        fade_out_seconds: 2.0 / 8000.0,
        max_duration_seconds: 1.0,
    };
    let p = plan(&r, 8000, 20).unwrap();
    let mut source = vec![0.25; 20];
    source[12..].fill(0.75);
    let actual = render(&p, &source, 1).unwrap();
    let expected = [
        0.0,
        0.25,
        0.25,
        0.25,
        0.25,
        5.0 / 12.0,
        7.0 / 12.0,
        0.75,
        0.75,
        0.75,
        0.75,
        0.0,
    ];
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        assert!((actual - expected).abs() < 0.000_001);
    }
}

#[test]
fn planning_refuses_invalid_bounds_before_audio() {
    for mutate in 0..14 {
        let mut r = recipe();
        match mutate {
            0 => r.segments.clear(),
            1 => r.segments = vec![[0.0, 1.0]; 65],
            2 => r.segments[0] = [-1.0, 1.0],
            3 => r.segments[0] = [1.0, 1.0],
            4 => r.segments[1] = [1.0, 3.0],
            5 => r.segments[1] = [3.0, 6.0],
            6 => r.crossfade_seconds = 1.1,
            7 => r.fade_in_seconds = 5.1,
            8 => r.fade_out_seconds = -0.1,
            9 => r.max_duration_seconds = 3.0,
            10 => r.crossfade_seconds = f64::NAN,
            11 => r.segments[0][1] = f64::INFINITY,
            12 => r.max_duration_seconds = 0.0,
            _ => r.fade_in_seconds = 2.0,
        }
        assert!(plan(&r, 8000, 40_000).is_err(), "case {mutate}");
    }
    for (rate, frames) in [
        (7999, 40_000),
        (96001, 40_000),
        (8000, 0),
        (8000, 4_800_001),
    ] {
        assert!(plan(&recipe(), rate, frames).is_err());
    }
}

#[test]
fn unchanged_projection_retains_word_metadata_but_excludes_every_changed_edge() {
    let plan = plan(&recipe(), 8000, 40_000).unwrap();
    let pass = json!({"name":"mix30","model":"actual-hash","chunk_seconds":30,
        "tokens":[{"word":"before","span":[0.0,0.1]},
        {"word":"edge","span":[0.1,0.2]},
        {"word":"kept","span":[0.125,0.5],"confidence":0.81},
        {"word":"join","span":[1.7,1.8]},
        {"word":"cut","span":[2.1,2.5]},
        {"word":"next","span":[3.25,3.5],"speaker":"original"},
        {"word":"tail","span":[4.9,5.0]},
        {"word":"unknown","span":[null,null]}]});
    let mapped = remap_passes(&plan, std::slice::from_ref(&pass)).unwrap();
    assert_eq!(mapped[0]["text"], "kept next");
    assert_eq!(mapped[0]["tokens"][0]["confidence"], 0.81);
    assert_eq!(mapped[0]["tokens"][1]["span"], json!([2.0, 2.25]));
    assert_eq!(mapped[0]["source_pass_metadata"]["model"], "actual-hash");
    assert_eq!(mapped[0]["new_recognition"], false);
    assert!(mapped[0].get("canonical_prompt").is_none());
    assert_eq!(pass["tokens"][5]["span"], json!([3.25, 3.5]));
}

#[test]
fn no_fade_identity_preserves_negative_zero_and_rejects_tampering() {
    let r = Recipe {
        segments: vec![[0.0, 0.001]],
        crossfade_seconds: 0.0,
        fade_in_seconds: 0.0,
        fade_out_seconds: 0.0,
        max_duration_seconds: 1.0,
    };
    let p = plan(&r, 8000, 8).unwrap();
    let src = [-0.0, 0.0, 0.5, -0.5, 0.25, -0.25, 0.1, 0.2];
    let mut output = render(&p, &src, 1).unwrap();
    assert_eq!(output[0].to_bits(), (-0.0f32).to_bits());
    output[0] = 0.0;
    assert!(verify_cores(&p, &src, &output, 1).is_err());
    assert!(render(&p, &[f32::NAN; 8], 1).is_err());
    assert!(render(&p, &src, 0).is_err());
    assert!(render(&p, &src[..7], 1).is_err());
    assert!(verify_cores(&p, &src, &[f32::INFINITY; 8], 1).is_err());
}

#[test]
fn evidence_passes_are_bounded_and_never_silently_unprompted() {
    let p = plan(&recipe(), 8000, 40_000).unwrap();
    let good = json!({"name":"mix","tokens":[]});
    for bad in [
        json!({"name":"mix","tokens":[],"canonical_prompt":true}),
        json!({"name":"mix","tokens":[{"word":"","span":[0.2,0.3]}]}),
        json!({"name":"mix","tokens":false}),
    ] {
        assert!(remap_passes(&p, &[bad]).is_err());
    }
    assert!(remap_passes(&p, &[]).is_err());
    assert!(remap_passes(&p, &[good.clone(), good.clone()]).is_err());
    assert!(remap_passes(&p, &vec![good; 17]).is_err());
    let oversized =
        json!({"name":"mix","tokens":vec![json!({"word":"x","span":[0.2,0.3]});10_001]});
    assert!(remap_passes(&p, &[oversized]).is_err());
}
