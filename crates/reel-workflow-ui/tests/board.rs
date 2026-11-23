mod support;
use serde_json::{Value, json};
#[cfg(windows)]
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};
fn board() -> (tempfile::TempDir, std::path::PathBuf, Value) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("sample");
    let value = support::create(&root);
    (temp, root, value)
}
#[test]
fn clock_identity_and_candidate_target_are_checked_before_media_reads() {
    let (_temp, _root, value) = board();
    let parsed: reel_workflow_ui::Board = serde_json::from_value(value.clone()).unwrap();
    parsed.check().unwrap();
    for changed in [
        {
            let mut v = value.clone();
            v["units"][1]["spans"][1]["start_sample"] = json!(72000);
            v
        },
        {
            let mut v = value.clone();
            v["tasks"]["redraw-c002"]["outputs"]["picture.png"]["unit_id"] = json!("event-1");
            v
        },
        {
            let mut v = value.clone();
            v["cues"][1]["preview"] = json!("es");
            v
        },
        {
            let mut v = value.clone();
            v["media"]["cel1"]["path"] = json!("relative.png");
            v
        },
    ] {
        assert!(
            serde_json::from_value::<reel_workflow_ui::Board>(changed)
                .unwrap()
                .check()
                .is_err()
        );
    }
}
#[test]
fn scene_review_clocks_can_differ_by_language_and_require_consistent_rates() {
    let (_temp, _root, mut value) = board();
    value["units"][0]["review_spans"] = json!([
        {"language":"es","sample_rate":44100,"start_sample":0,"end_sample":44100},
        {"language":"en","sample_rate":48000,"start_sample":0,"end_sample":72000}
    ]);
    let parsed: reel_workflow_ui::Board = serde_json::from_value(value.clone()).unwrap();
    parsed.check().unwrap();
    value["units"][1]["review_spans"] = json!([
        {"language":"es","sample_rate":48000,"start_sample":48000,"end_sample":96000}
    ]);
    let parsed: reel_workflow_ui::Board = serde_json::from_value(value).unwrap();
    assert_eq!(parsed.check().unwrap_err().code, "board-review-span-clock");
}

#[cfg(windows)]
#[test]
fn observed_candidate_is_live_but_selection_and_source_profile_remain_exact() {
    let (_temp, root, value) = board();
    let raw = fs::read(root.join("board.json")).unwrap();
    let pin = reel_workflow::digest(&raw);
    let viewer = reel_workflow_ui::host::Viewer::new(
        reel_workflow_ui::host::load(&root.join("board.json"), &pin).unwrap(),
    );
    let initial: Value = serde_json::from_slice(&viewer.view().unwrap()).unwrap();
    assert_eq!(initial["tasks"]["redraw-c002"]["state"], "waiting");
    assert!(initial.get("media").is_none());
    assert!(
        !String::from_utf8(viewer.view().unwrap())
            .unwrap()
            .contains(root.to_str().unwrap())
    );
    support::advance(&root, false);
    std::thread::sleep(Duration::from_millis(800));
    let pending: Value = serde_json::from_slice(&viewer.view().unwrap()).unwrap();
    assert_eq!(pending["tasks"]["redraw-c002"]["state"], "publishing");
    assert_eq!(pending["units"][1]["versions"].as_array().unwrap().len(), 2);
    assert_eq!(
        pending["units"][1]["versions"][0]["asset"],
        value["units"][1]["versions"][0]["asset"]
    );
    assert_eq!(
        pending["units"][1]["versions"][0]["designation"],
        "selected"
    );
    assert_eq!(
        pending["units"][1]["versions"][1]["designation"],
        "candidate"
    );
    support::advance(&root, true);
    std::thread::sleep(Duration::from_millis(800));
    let retained: Value = serde_json::from_slice(&viewer.view().unwrap()).unwrap();
    assert_eq!(
        retained["tasks"]["redraw-c002"]["state"],
        "retained-recorded"
    );
    assert_eq!(
        retained["tasks"]["redraw-c002"]["native_custody_verified"],
        false
    );
    assert_eq!(fs::read(root.join("board.json")).unwrap(), raw);
}
#[cfg(windows)]
struct Running(Child);
#[cfg(windows)]
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[cfg(windows)]
fn host(root: &Path, pin: &str) -> (Running, String, String) {
    let child = Command::new(env!("CARGO_BIN_EXE_reel-workflow-ui"))
        .arg("serve")
        .arg(root.join("board.json"))
        .arg(pin)
        .args(["--max-seconds", "30"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut running = Running(child);
    let mut line = String::new();
    BufReader::new(running.0.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let start: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(start["media_opened_at_startup"], 0);
    let (authority, path) = start["url"]
        .as_str()
        .unwrap()
        .strip_prefix("http://")
        .unwrap()
        .split_once('/')
        .unwrap();
    (
        running,
        authority.into(),
        path.split('/').next().unwrap().into(),
    )
}
#[cfg(windows)]
fn get(host: &str, path: &str, method: &str, headers: &str) -> Vec<u8> {
    let mut socket = TcpStream::connect(host).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        socket,
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\n{headers}\r\n"
    )
    .unwrap();
    let mut raw = Vec::new();
    socket.read_to_end(&mut raw).unwrap();
    raw
}
#[cfg(windows)]
#[test]
fn live_candidate_preview_cannot_alias_an_authored_media_key() {
    let (_temp, root, mut value) = board();
    let selected = value["media"]
        .as_object_mut()
        .unwrap()
        .remove("cel2")
        .unwrap();
    value["media"]["candidate-0-0"] = selected;
    value["units"][1]["versions"][0]["preview"] = "candidate-0-0".into();
    let raw = serde_json::to_vec(&value).unwrap();
    fs::write(root.join("board.json"), &raw).unwrap();
    let (_child, authority, token) = host(&root, &reel_workflow::digest(&raw));
    let route = |name: &str| format!("/{token}/{name}");
    support::advance(&root, false);

    fn body(response: &[u8]) -> &[u8] {
        assert!(response.starts_with(b"HTTP/1.1 200"));
        let index = response.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        &response[index + 4..]
    }

    let raw = get(&authority, &route("state.json"), "GET", "");
    let view: Value = serde_json::from_slice(body(&raw)).unwrap();
    let versions = &view["units"][1]["versions"];
    let selected_path = versions[0]["preview"].as_str().unwrap();
    let candidate_path = versions[1]["preview"].as_str().unwrap();
    assert_ne!(selected_path, candidate_path);
    assert_eq!(
        body(&get(&authority, &route(selected_path), "GET", "")),
        fs::read(root.join("cel-2.png")).unwrap()
    );
    assert_eq!(
        body(&get(&authority, &route(candidate_path), "GET", "")),
        fs::read(root.join("candidate.png")).unwrap()
    );
}

#[cfg(windows)]
#[test]
fn live_returned_recording_is_bound_to_one_cue_language_and_preserves_the_selected_take() {
    use reel_workflow::{Mode, Plan, Provenance, ReturnedResult, handoff::Journal};
    use std::collections::BTreeMap;

    let (_temp, root, mut board) = board();
    let old = Journal::open(&root.join("journal")).unwrap();
    let plan: Plan = serde_json::from_value(json!({
        "schema": reel_workflow::PLAN_SCHEMA, "project": board["project"],
        "source_commit": board["source_commit"],
        "tasks": [{ "id": "record-opening-es", "operation": "speech-candidate", "language": "es",
            "prompt": "Synthetic cue audition.", "inputs": [], "depends_on": [],
            "outputs": {"voice.wav": {"media_type": "audio/wav", "max_bytes": 500000, "required": true}} }]
    })).unwrap();
    let mut ticket = old.ticket().clone();
    ticket.packet =
        reel_workflow::make_packet(&plan, "record-opening-es", BTreeMap::new()).unwrap();
    let path = root.join("new-take.wav");
    support::tone(&path, 2, 220.0);
    let take = support::asset(&path, "recording/cue/es/candidate", "audio/wav");
    let mut journal = Journal::create(
        &root.join("cue-journal"),
        ticket.clone(),
        &mut support::Synthetic { pending: false },
    )
    .unwrap();
    journal
        .import(
            ReturnedResult {
                schema: reel_workflow::RESULT_SCHEMA.into(),
                task_id: ticket.packet.task.id.clone(),
                task_key: ticket.packet.task_key.clone(),
                attempt_id: ticket.attempt_id.clone(),
                artifacts: BTreeMap::from([("voice.wav".into(), take.clone())]),
                provenance: Provenance {
                    mode: Mode::Direct,
                    provider: Some("synthetic".into()),
                    model: Some("fixture-v1".into()),
                    executable: None,
                },
                worker_receipt: None,
            },
            &mut support::Synthetic { pending: false },
        )
        .unwrap();
    board["tasks"]["record-opening-es"] = json!({
        "journal": root.join("cue-journal"), "ticket_sha256": reel_workflow::canonical_digest(&ticket).unwrap(),
        "task_key": ticket.packet.task_key, "attempt_id": ticket.attempt_id,
        "outputs": {
            "voice.wav": { "cue": {"id": "cue-1", "language": "es"}, "path": path, "label": "New recording" }
        }
    });
    let mut wrong_language = board["tasks"]["record-opening-es"].clone();
    wrong_language["outputs"]["voice.wav"]["cue"]["language"] = "en".into();
    board["tasks"]["incorrect-english-watch"] = wrong_language;
    let raw = serde_json::to_vec(&board).unwrap();
    fs::write(root.join("board.json"), &raw).unwrap();
    let (_child, authority, token) = host(&root, &reel_workflow::digest(&raw));
    let status = get(&authority, &format!("/{token}/state.json"), "GET", "");
    assert!(status.starts_with(b"HTTP/1.1 200"));
    let header = status.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let view: Value = serde_json::from_slice(&status[header + 4..]).unwrap();
    assert_eq!(view["cue_candidates"].as_array().unwrap().len(), 1);
    let candidate = &view["cue_candidates"][0];
    assert_eq!(candidate["cue"], json!({"id": "cue-1", "language": "es"}));
    assert_eq!(candidate["asset"], serde_json::to_value(take).unwrap());
    let route = format!("/{token}/{}", candidate["preview"].as_str().unwrap());
    let audio = get(&authority, &route, "GET", "");
    assert!(audio.starts_with(b"HTTP/1.1 200"));
    assert!(audio.ends_with(&fs::read(path).unwrap()));
    assert_eq!(
        view["cues"][0]["selected_take"],
        board["cues"][0]["selected_take"]
    );
    assert_eq!(view["cues"][0]["markers"], board["cues"][0]["markers"]);
    assert_eq!(view["cues"][0]["samples"], board["cues"][0]["samples"]);
}

#[cfg(windows)]
#[test]
fn actual_host_is_lazy_capability_scoped_and_range_reads_verified_bytes() {
    let (_temp, root, _value) = board();
    let pin = reel_workflow::digest(&fs::read(root.join("board.json")).unwrap());
    let (_child, authority, token) = host(&root, &pin);
    let route = |name: &str| format!("/{token}/{name}");
    assert!(get(&authority, &route("state.json"), "GET", "").starts_with(b"HTTP/1.1 200"));
    assert!(get(&authority, &route("state.json"), "POST", "").starts_with(b"HTTP/1.1 405"));
    assert!(
        get(
            &authority,
            &route("state.json"),
            "GET",
            "Origin: https://outside.invalid\r\n"
        )
        .starts_with(b"HTTP/1.1 403")
    );
    assert!(get(&authority, &route("../board.json"), "GET", "").starts_with(b"HTTP/1.1 404"));
    let png = fs::read(root.join("cel-1.png")).unwrap();
    let read = get(
        &authority,
        &route("media/cel1"),
        "GET",
        "Range: bytes=0-7\r\n",
    );
    assert!(read.starts_with(b"HTTP/1.1 206"));
    assert!(read.ends_with(&png[..8]));
    assert!(get(&authority, &route("media/cel1"), "HEAD", "").ends_with(b"\r\n\r\n"));
    fs::write(root.join("cel-2.png"), b"wrong bytes").unwrap();
    assert!(get(&authority, &route("media/cel2"), "GET", "").starts_with(b"HTTP/1.1 409"));
    assert!(get(&authority, &route("media/private"), "GET", "").starts_with(b"HTTP/1.1 404"));
}
