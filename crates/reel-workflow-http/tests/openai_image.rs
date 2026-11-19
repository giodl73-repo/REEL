use base64::{Engine, engine::general_purpose::STANDARD};
use reel_workflow::{Artifact, ErrorKind};
use reel_workflow_http::{
    Config, SecretHeader,
    openai_image::{Input, Spec},
};
use serde_json::json;
use std::collections::BTreeMap;
fn spec() -> Spec {
    serde_json::from_value(json!({"schema":"reel.openai-image.v1","references":[],"mask":null,"size":[1024,1024],"quality":"high","background":"transparent"})).unwrap()
}
fn png(size: u32, alpha: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, size, size);
        encoder.set_color(if alpha {
            png::ColorType::Rgba
        } else {
            png::ColorType::Rgb
        });
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&vec![
                0;
                size as usize
                    * size as usize
                    * if alpha { 4 } else { 3 }
            ])
            .unwrap();
    }
    bytes
}
fn pin(id: &str, bytes: &[u8]) -> Artifact {
    Artifact {
        logical_id: id.into(),
        sha256: reel_workflow::digest(bytes),
        bytes: bytes.len() as u64,
        media_type: "image/png".into(),
    }
}
fn http(endpoint: &str) -> Config {
    Config {
        schema: "reel.workflow-http.v1".into(),
        endpoint: endpoint.into(),
        timeout_seconds: 1,
        request_byte_limit: 4 * 1024 * 1024,
        response_byte_limit: 4 * 1024 * 1024,
        secret_headers: BTreeMap::new(),
        allow_loopback_http: false,
    }
}
#[test]
fn generation_is_single_inline_png_with_exact_explicit_prompt_and_no_source_upload() {
    let s = spec();
    let raw = s
        .request(
            "gpt-image-2.5-sunburst-2026-09-08",
            "Texto español exacto.",
            &BTreeMap::new(),
            4096,
        )
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(body["prompt"], "Texto español exacto.");
    assert_eq!(body["model"], "gpt-image-2.5-sunburst-2026-09-08");
    assert_eq!(body["n"], 1);
    assert_eq!(body["stream"], false);
    assert_eq!(body["output_format"], "png");
    assert!(body.get("images").is_none());
    assert!(body.get("inputs").is_none());
    let image = png(1024, true);
    let response=serde_json::to_vec(&json!({"created":123,"output_format":"png","size":"1024x1024","data":[{"b64_json":STANDARD.encode(&image),"revised_prompt":"private revised text"}],"usage":{"input_tokens":3}})).unwrap();
    let outputs = s
        .normalize(
            "gpt-image-2.5-sunburst-2026-09-08",
            "Texto español exacto.",
            &response,
            1_000_000,
        )
        .unwrap();
    assert_eq!(outputs["candidate.png"], image);
    let meta: serde_json::Value = serde_json::from_slice(&outputs["image.json"]).unwrap();
    assert_eq!(meta["requested_model"], body["model"]);
    assert_eq!(meta["width"], 1024);
    assert_eq!(meta["transparent_pixels"], true);
    assert_eq!(meta["selected"], false);
    assert_eq!(meta["approved"], false);
    assert!(!String::from_utf8_lossy(&outputs["image.json"]).contains("private revised text"));
}
#[test]
fn reference_order_and_mask_are_bound_to_exact_declared_png_bytes() {
    let first = png(16, true);
    let second = png(32, true);
    let mask = png(16, true);
    let pins = [
        pin("first", &first),
        pin("second", &second),
        pin("mask", &mask),
    ];
    let inputs = BTreeMap::from([
        (
            "first".into(),
            Input {
                artifact: &pins[0],
                bytes: &first,
            },
        ),
        (
            "second".into(),
            Input {
                artifact: &pins[1],
                bytes: &second,
            },
        ),
        (
            "mask".into(),
            Input {
                artifact: &pins[2],
                bytes: &mask,
            },
        ),
    ]);
    let mut s = spec();
    s.references = vec!["second".into(), "first".into()];
    let raw = s
        .request("gpt-image-1.5", "Use these exact references", &inputs, 4096)
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(
        body["images"][0]["image_url"],
        format!("data:image/png;base64,{}", STANDARD.encode(&second))
    );
    assert_eq!(
        body["images"][1]["image_url"],
        format!("data:image/png;base64,{}", STANDARD.encode(&first))
    );
    assert_eq!(body["images"].as_array().unwrap().len(), 2);
    assert!(body.get("mask").is_none());
    s.mask = Some("mask".into());
    assert_eq!(
        s.request("gpt-image-1.5", "edit", &inputs, 4096)
            .unwrap_err()
            .code,
        "openai-image-mask-layout"
    );
    s.references.reverse();
    let body: serde_json::Value =
        serde_json::from_slice(&s.request("gpt-image-1.5", "edit", &inputs, 4096).unwrap())
            .unwrap();
    assert!(
        body["mask"]["image_url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,")
    );
    assert_eq!(
        s.request("gpt-image-1.5", "edit", &inputs, 30)
            .unwrap_err()
            .kind,
        ErrorKind::Budget
    );
    let changed = BTreeMap::from([
        (
            "first".into(),
            Input {
                artifact: &pins[0],
                bytes: b"changed",
            },
        ),
        (
            "second".into(),
            Input {
                artifact: &pins[1],
                bytes: &second,
            },
        ),
        (
            "mask".into(),
            Input {
                artifact: &pins[2],
                bytes: &mask,
            },
        ),
    ]);
    assert_eq!(
        s.request("gpt-image-1.5", "edit", &changed, 4096)
            .unwrap_err()
            .kind,
        ErrorKind::Integrity
    );
}
#[test]
fn endpoint_requires_canonical_service_path_credentials_or_literal_loopback() {
    let mut s = spec();
    let mut c = http("https://api.openai.com/v1/images/generations");
    assert!(s.check_endpoint(&c).is_err());
    c.secret_headers.insert(
        "Authorization".into(),
        SecretHeader {
            variable: "TEST_KEY".into(),
            prefix: "Bearer ".into(),
        },
    );
    s.check_endpoint(&c).unwrap();
    for url in [
        "https://example.invalid/v1/images/generations",
        "https://api.openai.com/v1/images/generations?redirect=1",
        "https://api.openai.com/v1/images/edits",
        "http://localhost/v1/images/generations",
    ] {
        c.endpoint = url.into();
        assert!(s.check_endpoint(&c).is_err());
    }
    c = http("http://127.0.0.1:1234/v1/images/generations");
    c.allow_loopback_http = true;
    s.check_endpoint(&c).unwrap();
    s.references.push("reference".into());
    assert!(s.check_endpoint(&c).is_err());
    c.endpoint = "http://127.0.0.1:1234/v1/images/edits".into();
    s.check_endpoint(&c).unwrap();
}
#[test]
fn url_only_multiple_corrupt_wrong_size_and_missing_alpha_outputs_refuse() {
    let s = spec();
    for data in [
        json!([{"url":"https://example.invalid/image.png"}]),
        json!([]),
        json!([{"b64_json":"AA=="},{"b64_json":"AA=="}]),
        json!([{"b64_json":STANDARD.encode(png(16,true))}]),
        json!([{"b64_json":STANDARD.encode(png(1024,false))}]),
        json!([{"b64_json":STANDARD.encode(b"not PNG")}]),
    ] {
        assert!(
            s.normalize(
                "gpt-image-1.5",
                "test",
                &serde_json::to_vec(&json!({"data":data})).unwrap(),
                1_000_000
            )
            .is_err()
        );
    }
    let mut corrupt = png(1024, true);
    let end = corrupt.len() - 1;
    corrupt[end] ^= 0xff;
    assert!(
        s.normalize(
            "gpt-image-1.5",
            "test",
            &serde_json::to_vec(&json!({"data":[{"b64_json":STANDARD.encode(corrupt)}]})).unwrap(),
            1_000_000
        )
        .is_err()
    );
    let response =
        serde_json::to_vec(&json!({"data":[{"b64_json":STANDARD.encode(png(1024,true))}]}))
            .unwrap();
    assert_eq!(
        s.normalize("gpt-image-1.5", "test", &response, 1)
            .unwrap_err()
            .kind,
        ErrorKind::Budget
    );
}
#[test]
fn sixteen_bit_alpha_is_checked_without_rounding_it_to_opaque() {
    let mut raw = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut raw, 1024, 1024);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Sixteen);
        let mut writer = encoder.write_header().unwrap();
        let mut pixels = vec![255; 1024 * 1024 * 8];
        for p in pixels.as_chunks_mut::<8>().0 {
            p[7] = 254;
        }
        writer.write_image_data(&pixels).unwrap();
    }
    let mut s = spec();
    s.background = "opaque".into();
    let response =
        serde_json::to_vec(&json!({"data":[{"b64_json":STANDARD.encode(&raw)}]})).unwrap();
    assert_eq!(
        s.normalize("gpt-image-1.5", "test", &response, 1_000_000)
            .unwrap_err()
            .code,
        "openai-image-output-layout"
    );
}
