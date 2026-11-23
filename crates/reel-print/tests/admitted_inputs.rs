use reel_print::{Object, Pin};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

fn object(raw: &[u8]) -> Object {
    Object {
        sha256: Sha256::digest(raw)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
        bytes: raw.len() as u64,
    }
}
fn pin(path: &Path, raw: &[u8]) -> serde_json::Value {
    let p = object(raw);
    json!({"path":path,"sha256":p.sha256,"bytes":p.bytes})
}
fn fixture(root: &Path) -> (Vec<u8>, BTreeMap<PathBuf, Vec<u8>>) {
    let (font, family) = [
        ("C:/Windows/Fonts/arial.ttf", "Arial"),
        (
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "DejaVu Sans",
        ),
        (
            "/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf",
            "Liberation Sans",
        ),
    ]
    .into_iter()
    .find(|(p, _)| Path::new(p).is_file())
    .expect("explicit fixture font required");
    let font = PathBuf::from(font);
    let font_raw = fs::read(&font).unwrap();
    let source = root.join("source.txt");
    let text = "Exact source with canción and its own wording."
        .as_bytes()
        .to_vec();
    fs::write(&source, &text).unwrap();
    let request=serde_json::to_vec(&json!({"schema":"reel.print-collection.v1","title":"Source-bound reading",
        "author":"Public synthetic fixture","notice":"Technical check only","private_only":true,
        "fonts":[pin(&font,&font_raw)],"body_font":family,
        "entries":[{"kind":"text","id":"chapter-one","title":"Chapter one","source":pin(&source,&text)}],
        "limits":{"input_bytes":16777216,"output_bytes":16777216,"pages":8}})).unwrap();
    (request, BTreeMap::from([(font, font_raw), (source, text)]))
}
fn namespace(root: &Path, receipt: &reel_print::Receipt) -> BTreeMap<String, Object> {
    let mut objects = receipt.outputs.clone();
    objects.insert(
        "receipt.json".into(),
        object(&fs::read(root.join("receipt.json")).unwrap()),
    );
    objects
}

#[test]
fn admitted_snapshots_match_legacy_render_and_refusal_never_falls_back() {
    let temp = tempfile::tempdir().unwrap();
    let (raw, files) = fixture(temp.path());
    let request = temp.path().join("request.json");
    fs::write(&request, &raw).unwrap();
    let legacy = temp.path().join("legacy");
    reel_print::render(&request, &legacy).unwrap();
    let mut refused_calls = 0;
    assert!(
        reel_print::plan_with_inputs(&raw, |_| {
            refused_calls += 1;
            anyhow::bail!("explicit provider refused")
        })
        .is_err()
    );
    assert_eq!(refused_calls, 1);
    // Both real files exist and the legacy planner succeeds. A callback refusal
    // therefore cannot have been converted into a successful filesystem retry.
    assert!(reel_print::plan(&request).is_ok());
    let mut value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    let font = files
        .keys()
        .find(|p| p.extension().is_some_and(|e| e == "ttf"))
        .unwrap();
    let source = files.keys().find(|p| p.ends_with("source.txt")).unwrap();
    let aliases = BTreeMap::from([
        (PathBuf::from("native/font"), files[font].clone()),
        (PathBuf::from("native/chapter"), files[source].clone()),
    ]);
    value["fonts"][0]["path"] = json!("native/font");
    value["entries"][0]["source"]["path"] = json!("native/chapter");
    let raw = serde_json::to_vec(&value).unwrap();
    let output = temp.path().join("admitted");
    let mut calls = BTreeMap::new();
    let planned = reel_print::plan_with_inputs(&raw, |p| {
        *calls.entry(p.path.clone()).or_insert(0) += 1;
        Ok(aliases[&p.path].clone())
    })
    .unwrap();
    assert!(calls.values().all(|count| *count == 1));
    assert_eq!(calls.len(), 2);
    assert!(!output.exists());
    let receipt =
        reel_print::render_with_inputs(&raw, |p| Ok(aliases[&p.path].clone()), &output).unwrap();
    assert_eq!(receipt, planned);
    for name in receipt.outputs.keys() {
        assert_eq!(
            fs::read(output.join(name)).unwrap(),
            fs::read(legacy.join(name)).unwrap()
        );
    }
    let objects = namespace(&output, &receipt);
    reel_print::check_with_inputs(
        &raw,
        |p| Ok(aliases[&p.path].clone()),
        &objects,
        |name, _| Ok(fs::read(output.join(name))?),
    )
    .unwrap();
    assert!(!receipt.human_approval && !receipt.published && !receipt.authorizes_eviction);
    assert!(
        receipt
            .pdf_raster_check
            .contains("external-verification-required")
    );
    let false_output = temp.path().join("false-source");
    assert!(
        reel_print::render_with_inputs(
            &raw,
            |p: &Pin| Ok(vec![0; p.bytes as usize]),
            &false_output
        )
        .is_err()
    );
    assert!(!false_output.exists());
}

#[test]
fn declared_packet_namespace_and_rehashed_false_pdf_refuse() {
    let temp = tempfile::tempdir().unwrap();
    let (raw, files) = fixture(temp.path());
    let output = temp.path().join("packet");
    let receipt =
        reel_print::render_with_inputs(&raw, |p| Ok(files[&p.path].clone()), &output).unwrap();
    let objects = namespace(&output, &receipt);
    for mutation in 0..3 {
        let mut bad = objects.clone();
        match mutation {
            0 => {
                bad.remove("collection.pdf");
            }
            1 => {
                bad.insert("unrequested.pdf".into(), object(b"unknown"));
            }
            _ => {
                bad.insert("collection.pdf".into(), object(b"rehashed false PDF"));
            }
        }
        let mut reads = 0;
        assert!(
            reel_print::check_with_inputs(
                &raw,
                |p| Ok(files[&p.path].clone()),
                &bad,
                |_, _| {
                    reads += 1;
                    Ok(vec![])
                }
            )
            .is_err()
        );
        assert_eq!(reads, 0);
    }
    assert!(
        reel_print::check_with_inputs(
            &raw,
            |p| Ok(files[&p.path].clone()),
            &objects,
            |name, _| {
                let mut bytes = fs::read(output.join(name))?;
                if name == "collection.pdf" {
                    bytes[0] ^= 1;
                }
                Ok(bytes)
            }
        )
        .is_err()
    );
}
