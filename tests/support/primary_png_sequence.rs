use super::*;

#[test]
fn ordered_primary_pngs_cross_a_cut_without_payload_copies_or_input_movies() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("sequence 'quoted' root");
    fs::create_dir(&root).unwrap();
    let root = root.as_path();
    let mut job = fixture(root);
    for (name, value) in [("white 'one'.png", 255), ("black.png", 0)] {
        image::RgbaImage::from_pixel(64, 64, image::Rgba([value, value, value, 255]))
            .save(root.join(name))
            .unwrap();
    }
    let white = file(root, "white 'one'.png");
    let black = file(root, "black.png");
    let mut contract: Value =
        serde_json::from_slice(&fs::read(root.join("contract.json")).unwrap()).unwrap();
    contract["attachments"].as_array_mut().unwrap().push(json!({
        "id":"sequence", "target":{"kind":"overlay","shot_id":"shot","overlay_id":"storm-glow"},
        "start":{"kind":"cue-start","cue_id":"a","offset_samples":46000},
        "end":{"kind":"cue-start","cue_id":"b","offset_samples":6000}
    }));
    write_json(&root.join("contract.json"), &contract);
    job["contract"] = file(root, "contract.json");
    job["external_layers"] = json!([{
        "attachment_id":"sequence", "reason":"Five exact primary frames over a cut",
        "evidence":white,"render_mode":"timed-png-sequence-overlay",
        "frames":[white,black,white,black,white]
    }]);
    let selected = job.clone();
    for frames in [
        json!([]),
        json!([white]),
        json!([black, white, black, white, black]),
        json!([white, black, file(root, "red.ppm"), black, white]),
    ] {
        job = selected.clone();
        job["external_layers"][0]["frames"] = frames;
        write_json(&root.join("job.json"), &job);
        assert!(scene_delivery::plan(&root.join("job.json"), root).is_err());
    }
    job = selected.clone();
    job["external_layers"][0]["frames"] = Value::Array(vec![white.clone(); 4097]);
    write_json(&root.join("job.json"), &job);
    assert!(
        scene_delivery::plan(&root.join("job.json"), root)
            .unwrap_err()
            .to_string()
            .contains("budget")
    );
    job = selected.clone();
    job["external_layers"][0]["render_mode"] = json!("timed-still-overlay");
    write_json(&root.join("job.json"), &job);
    assert!(scene_delivery::plan(&root.join("job.json"), root).is_err());
    for key in ["font", "render_source", "derivation_receipt"] {
        job = selected.clone();
        job["external_layers"][0][key] = white.clone();
        write_json(&root.join("job.json"), &job);
        assert!(scene_delivery::plan(&root.join("job.json"), root).is_err());
    }
    job = selected;
    write_json(&root.join("job.json"), &job);
    let output = root.join("rendered");
    let receipt = scene_delivery::render(&root.join("job.json"), root, &output).unwrap();
    assert_eq!(
        (
            receipt.plan.external_layer_spans[0].start_frame,
            receipt.plan.external_layer_spans[0].end_frame
        ),
        (23, 28)
    );
    assert!(
        receipt
            .outputs
            .contains_key("selected-overlay.sequence.json")
    );
    assert!(!receipt.outputs.contains_key("selected-overlay.mkv"));
    assert!(!receipt.outputs.keys().any(|key| key.ends_with(".png")));
    let index: Value =
        serde_json::from_slice(&fs::read(output.join("selected-overlay.sequence.json")).unwrap())
            .unwrap();
    assert_eq!(index["frames"], job["external_layers"][0]["frames"]);
    assert!(
        !fs::read_to_string(output.join("selected-overlay.sequence.json"))
            .unwrap()
            .contains(root.to_str().unwrap())
    );
    assert!(!fs::read_dir(&output).unwrap().any(|f| {
        f.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".png-sequence-")
    }));
    scene_delivery::check(&root.join("job.json"), root, &output).unwrap();
    primary_reference::check(root, &root.join("job.json"), &output);
    assert_frame_colors(&output, 48, 23, &[255, 0, 255, 0, 255]);
    fs::write(output.join("selected-overlay.sequence.json"), b"tampered").unwrap();
    assert!(scene_delivery::check(&root.join("job.json"), root, &output).is_err());
    contract["frame_rate"] = json!({"numerator":30000,"denominator":1001});
    write_json(&root.join("contract.json"), &contract);
    job["contract"] = file(root, "contract.json");
    for still in [false, true] {
        if still {
            job["external_layers"][0]["render_mode"] = json!("timed-still-overlay");
            job["external_layers"][0]["frames"] = json!([]);
        }
        write_json(&root.join("job.json"), &job);
        let output = root.join(if still {
            "fractional-still"
        } else {
            "fractional-sequence"
        });
        let receipt = scene_delivery::render(&root.join("job.json"), root, &output).unwrap();
        assert_eq!(
            (
                receipt.plan.external_layer_spans[0].start_frame,
                receipt.plan.external_layer_spans[0].end_frame
            ),
            (29, 34)
        );
        scene_delivery::check(&root.join("job.json"), root, &output).unwrap();
        primary_reference::check(root, &root.join("job.json"), &output);
        assert_frame_colors(
            &output,
            60,
            29,
            if still {
                &[255, 255, 255, 255, 255]
            } else {
                &[255, 0, 255, 0, 255]
            },
        );
    }
}

fn assert_frame_colors(output: &Path, frames: usize, start: usize, expected: &[u8]) {
    let decoded = Command::new("ffmpeg")
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(output.join("picture.mkv"))
        .args([
            "-fps_mode",
            "passthrough",
            "-pix_fmt",
            "rgb24",
            "-f",
            "rawvideo",
            "-",
        ])
        .output()
        .unwrap();
    assert!(decoded.status.success());
    assert_eq!(decoded.stdout.len(), frames * 64 * 64 * 3);
    for (offset, expected) in expected.iter().enumerate() {
        let frame = start + offset;
        assert!(
            decoded.stdout[frame * 64 * 64 * 3..(frame + 1) * 64 * 64 * 3]
                .iter()
                .all(|v| v == expected),
            "frame {frame}: first RGB {:?}",
            &decoded.stdout[frame * 64 * 64 * 3..frame * 64 * 64 * 3 + 3]
        );
    }
}
