//! Bounded retained-native proof checks; never executes the retained request.
use super::*;
use flate2::read::GzDecoder;
use std::io::Read;

const METADATA_LIMIT: usize = 4 * 1024 * 1024;
const SNAPSHOT_LIMIT: usize = 8 * 1024 * 1024;

#[derive(Debug)]
pub struct SnapshotView {
    pub slot: String,
    pub object: Object,
    pub sample_id: String,
    pub frame: u64,
}

pub struct RetainedSnapshot<'a> {
    pub request: &'a Object,
    pub request_bytes: &'a [u8],
    pub result: &'a Object,
    pub result_bytes: &'a [u8],
    pub compressed: &'a Object,
    pub compressed_bytes: &'a [u8],
    /// Reopened exact pinned root document, never a path to a historical checkout.
    pub root: &'a Value,
    pub required_views: &'a [SnapshotView],
}

fn parse(bytes: &[u8], expected: &Object) -> Result<Value> {
    ensure!(
        bytes.len() <= METADATA_LIMIT && Object::from_bytes(bytes) == *expected,
        "retained metadata pin differs or exceeds bound"
    );
    parse_json(bytes)
}

/// Compare the exact request/result with the gzip snapshot and all requested
/// output pins. Root booleans remain historical assertions, not a new custody or
/// deletion decision. Current workspace paths in this evidence are never opened.
pub fn validate_retired_snapshot(input: RetainedSnapshot<'_>) -> Result<()> {
    let request = parse(input.request_bytes, input.request)?;
    let result = parse(input.result_bytes, input.result)?;
    ensure!(
        input.compressed_bytes.len() <= METADATA_LIMIT
            && Object::from_bytes(input.compressed_bytes) == *input.compressed,
        "retirement gzip pin differs"
    );
    let mut decoded = Vec::new();
    GzDecoder::new(input.compressed_bytes)
        .take(SNAPSHOT_LIMIT as u64 + 1)
        .read_to_end(&mut decoded)?;
    ensure!(
        decoded.len() <= SNAPSHOT_LIMIT,
        "retirement snapshot exceeds decoded bound"
    );
    let snapshot = parse_json(&decoded)?;
    ensure!(
        text(&snapshot, "schema")? == "reel.job-retirement-evidence.v1",
        "unsupported retained snapshot"
    );
    ensure!(
        text(&snapshot, "request_utf8")?.as_bytes() == input.request_bytes,
        "retirement request differs"
    );
    let before = at(&snapshot, "result")?;
    ensure!(
        boolean(before, "workspace_retained")?,
        "snapshot must describe the pre-retirement workspace"
    );
    if before != &result {
        // jobs.rs publishes the snapshot first, then changes exactly these two
        // fields in the returned terminal result. A gzip cannot contain its own
        // final hash. No other execution/output differences are permitted.
        ensure!(
            !boolean(&result, "workspace_retained")?
                && pin(at(&result, "retirement_evidence")?)? == *input.compressed,
            "invalid post-retirement transition"
        );
        let mut expected = result.clone();
        expected
            .as_object_mut()
            .ok_or_else(|| anyhow!("invalid terminal result"))?
            .remove("retirement_evidence");
        expected["workspace_retained"] = json!(true);
        ensure!(
            &expected == before,
            "retirement result differs outside terminal transition"
        );
    }
    ensure!(
        text(&result, "manifest_sha256")? == input.request.sha256.as_str(),
        "result request identity differs"
    );
    ensure!(
        !boolean(&result, "original_inputs_modified")? && !boolean(&result, "daemon_started")?,
        "unexpected result side effects"
    );
    boolean(&result, "workspace_retained")?;
    ensure!(
        pin(at(input.root, "request")?)? == *input.request,
        "root request differs"
    );
    ensure!(
        pin(at(input.root, "retirement_evidence")?)? == *input.compressed,
        "root retirement identity differs"
    );
    ensure!(
        boolean(input.root, "positive_terminal_retired")?
            && boolean(
                input.root,
                "all_output_SHA_bytes_caps_and_dimensions_verified"
            )?,
        "root is not a positive retained proof"
    );
    ensure!(
        !boolean(input.root, "human_approval")?,
        "root cannot confer human approval"
    );
    let frames = array(input.root, "frames")?;
    let ids = array(input.root, "sample_ids")?;
    ensure!(
        !frames.is_empty() && frames.len() == ids.len() && frames.len() <= 512,
        "invalid retained root scope"
    );
    let mut samples = BTreeMap::new();
    for (frame, id) in frames.iter().zip(ids) {
        let frame = frame
            .as_u64()
            .ok_or_else(|| anyhow!("invalid root frame"))?;
        let id = id.as_str().ok_or_else(|| anyhow!("invalid root sample"))?;
        ensure!(samples.insert(id, frame).is_none(), "duplicate root sample");
    }
    let outputs = at(&result, "outputs")?
        .as_object()
        .ok_or_else(|| anyhow!("invalid result output table"))?;
    let declared = at(&snapshot, "declared_retained_outputs")?
        .as_object()
        .ok_or_else(|| anyhow!("invalid declared output table"))?;
    let requested = at(&request, "outputs")?
        .as_object()
        .ok_or_else(|| anyhow!("invalid request output table"))?;
    ensure!(
        !outputs.is_empty()
            && outputs.len() <= 2048
            && names(&json!(outputs))? == names(&json!(declared))?
            && names(&json!(outputs))? == names(&json!(requested))?,
        "retained output sets differ"
    );
    for (slot, output) in outputs {
        let object = pin(output)?;
        ensure!(
            pin(&declared[slot])? == object,
            "declared retained output differs"
        );
        ensure!(
            object.bytes <= integer(&requested[slot], "max_bytes")?,
            "retained output exceeds original cap"
        );
    }
    ensure!(
        !input.required_views.is_empty() && input.required_views.len() <= 1024,
        "invalid required view count"
    );
    let mut slots = BTreeSet::new();
    for view in input.required_views {
        ensure!(slots.insert(&view.slot), "duplicate required output slot");
        ensure!(
            samples.get(view.sample_id.as_str()) == Some(&view.frame),
            "view sample/frame differs from retained root"
        );
        ensure!(
            pin(outputs
                .get(&view.slot)
                .ok_or_else(|| anyhow!("required output is absent"))?)?
                == view.object,
            "required view differs from retained output"
        );
    }
    Ok(())
}

/// Header check after protected SHA/byte verification. Does not decode or view
/// pixels. Raw references can allow alpha; native RGB8 roles require color type 2.
pub fn validate_png_header(bytes: &[u8], dimensions: [u32; 2], require_rgb8: bool) -> Result<()> {
    ensure!(
        bytes.len() >= 33
            && &bytes[..8] == b"\x89PNG\r\n\x1a\n"
            && bytes[8..12] == 13_u32.to_be_bytes()
            && &bytes[12..16] == b"IHDR",
        "invalid PNG header"
    );
    let width = u32::from_be_bytes(bytes[16..20].try_into()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into()?);
    ensure!(
        width > 0 && height > 0 && [width, height] == dimensions,
        "PNG dimensions differ"
    );
    ensure!(
        bytes[24] == 8
            && if require_rgb8 {
                bytes[25] == 2
            } else {
                matches!(bytes[25], 2 | 6)
            },
        "PNG pixel format differs"
    );
    ensure!(
        bytes[26] == 0 && bytes[27] == 0 && bytes[28] <= 1,
        "unsupported PNG header flags"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::GzEncoder};
    use std::io::Write;
    #[test]
    fn png_header_requires_actual_dimensions_and_rgb8() {
        let mut bytes = vec![0; 33];
        bytes[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        bytes[8..12].copy_from_slice(&13_u32.to_be_bytes());
        bytes[12..16].copy_from_slice(b"IHDR");
        bytes[16..20].copy_from_slice(&4_u32.to_be_bytes());
        bytes[20..24].copy_from_slice(&3_u32.to_be_bytes());
        bytes[24] = 8;
        bytes[25] = 2;
        validate_png_header(&bytes, [4, 3], true).unwrap();
        assert!(validate_png_header(&bytes, [3, 4], true).is_err());
        bytes[25] = 6;
        assert!(validate_png_header(&bytes, [4, 3], true).is_err());
        validate_png_header(&bytes, [4, 3], false).unwrap();
        bytes[24] = 16;
        assert!(validate_png_header(&bytes, [4, 3], false).is_err());
        assert!(validate_png_header(&bytes[..28], [4, 3], true).is_err());
    }
    #[test]
    fn snapshot_cross_checks_all_tables_pins_caps_and_sample_mapping() {
        let image = Object::from_bytes(b"synthetic image");
        let request =
            serde_json::to_vec(&json!({"outputs":{"small":{"path":"small.png","max_bytes":100}}}))
                .unwrap();
        let request_pin = Object::from_bytes(&request);
        let result=serde_json::to_vec(&json!({"outputs":{"small":image},"manifest_sha256":request_pin.sha256,"original_inputs_modified":false,"daemon_started":false,"workspace_retained":true})).unwrap();
        let result_pin = Object::from_bytes(&result);
        let snapshot = json!({"schema":"reel.job-retirement-evidence.v1","request_utf8":String::from_utf8(request.clone()).unwrap(),"result":serde_json::from_slice::<Value>(&result).unwrap(),"declared_retained_outputs":{"small":image}});
        let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
        gzip.write_all(&serde_json::to_vec(&snapshot).unwrap())
            .unwrap();
        let compressed = gzip.finish().unwrap();
        let compressed_pin = Object::from_bytes(&compressed);
        let root = json!({"request":request_pin,"retirement_evidence":compressed_pin,"positive_terminal_retired":true,"all_output_SHA_bytes_caps_and_dimensions_verified":true,"human_approval":false,"frames":[12],"sample_ids":["one"]});
        let views = vec![SnapshotView {
            slot: "small".into(),
            object: image,
            sample_id: "one".into(),
            frame: 12,
        }];
        let check = |root: &Value, views: &[SnapshotView], bytes: &[u8]| {
            validate_retired_snapshot(RetainedSnapshot {
                request: &request_pin,
                request_bytes: &request,
                result: &result_pin,
                result_bytes: &result,
                compressed: &compressed_pin,
                compressed_bytes: bytes,
                root,
                required_views: views,
            })
        };
        check(&root, &views, &compressed).unwrap();
        let mut terminal: Value = serde_json::from_slice(&result).unwrap();
        terminal["workspace_retained"] = json!(false);
        terminal["retirement_evidence"] = json!(compressed_pin);
        let terminal_bytes = serde_json::to_vec(&terminal).unwrap();
        let terminal_pin = Object::from_bytes(&terminal_bytes);
        validate_retired_snapshot(RetainedSnapshot {
            request: &request_pin,
            request_bytes: &request,
            result: &terminal_pin,
            result_bytes: &terminal_bytes,
            compressed: &compressed_pin,
            compressed_bytes: &compressed,
            root: &root,
            required_views: &views,
        })
        .unwrap();
        terminal["retirement_evidence"] = json!(Object::from_bytes(b"wrong snapshot"));
        let bad_bytes = serde_json::to_vec(&terminal).unwrap();
        let bad_pin = Object::from_bytes(&bad_bytes);
        assert!(
            validate_retired_snapshot(RetainedSnapshot {
                request: &request_pin,
                request_bytes: &request,
                result: &bad_pin,
                result_bytes: &bad_bytes,
                compressed: &compressed_pin,
                compressed_bytes: &compressed,
                root: &root,
                required_views: &views
            })
            .is_err()
        );
        let mut bad_root = root.clone();
        bad_root["frames"][0] = json!(13);
        assert!(check(&bad_root, &views, &compressed).is_err());
        let mut bad_root = root;
        bad_root["human_approval"] = json!(true);
        assert!(check(&bad_root, &views, &compressed).is_err());
        assert!(check(&bad_root, &views, &compressed[..compressed.len() - 1]).is_err());
    }
}
