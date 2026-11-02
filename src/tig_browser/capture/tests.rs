use super::*;
use crate::browser_composition::{HashedAsset, PixelRegion};
use std::{cell::Cell, collections::VecDeque, io::Cursor, rc::Rc};

fn limits() -> Limits {
    Limits {
        max_requests: 8,
        max_resource_bytes: 1024,
        max_served_bytes: 4096,
        max_message_bytes: 16384,
        max_messages: 256,
        max_png_bytes: 1024,
        max_frames: 8,
        max_canvas_pixels: 64,
        max_elapsed_ms: 2000,
        max_io_wait_ms: 100,
    }
}
fn manifest() -> BrowserCompositionManifest {
    BrowserCompositionManifest {
        schema: crate::browser_composition::MANIFEST_SCHEMA.into(),
        source: HashedAsset {
            path: "opening.svg".into(),
            sha256: "0".repeat(64),
        },
        assets: vec![],
        width: 8,
        height: 8,
        fps: 24,
        frame_count: 3,
        dynamic_regions: vec![],
        audio: None,
    }
}
fn png(color: [u8; 4]) -> Vec<u8> {
    encode(image::RgbaImage::from_pixel(8, 8, image::Rgba(color)))
}
fn encode(image: image::RgbaImage) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}
fn fetch(url: &str) -> Value {
    json!({"method":"Fetch.requestPaused","params":{"requestId":"r1",
        "request":{"url":url,"method":"GET","headers":{}}}})
}
#[derive(Default)]
struct Signals {
    captures: Cell<u64>,
    written: Cell<u64>,
    checks: Cell<u64>,
}
struct Wire {
    queue: VecDeque<Value>,
    sent: Vec<Value>,
    fetches: Vec<Value>,
    pngs: Vec<Vec<u8>>,
    signals: Rc<Signals>,
    bad_ack: bool,
    bad_clock: bool,
    navigate_away: bool,
    ack_first: bool,
    oversize: bool,
    flood: bool,
    exception: bool,
    duplicate: bool,
}
impl Wire {
    fn new(signals: Rc<Signals>) -> Self {
        Self {
            queue: VecDeque::new(),
            sent: vec![],
            fetches: vec![fetch(&resource_url("opening.svg").unwrap())],
            pngs: vec![png([10, 20, 30, 255]); 3],
            signals,
            bad_ack: false,
            bad_clock: false,
            navigate_away: false,
            ack_first: false,
            oversize: false,
            flood: false,
            exception: false,
            duplicate: false,
        }
    }
}
impl Transport for Wire {
    fn send(&mut self, text: &str, _: Duration) -> Result<()> {
        let value: Value = serde_json::from_str(text)?;
        let id = value["id"].clone();
        let mut result = json!({});
        match value["method"].as_str().unwrap() {
            "Page.navigate" => self.queue.extend(self.fetches.clone()),
            "Fetch.fulfillRequest" if self.bad_ack => {
                self.queue
                    .push_back(json!({"id":id,"error":{"message":"fixture failure"}}));
                self.sent.push(value);
                return Ok(());
            }
            "Page.captureScreenshot" => {
                assert_eq!(
                    self.signals.captures.get(),
                    self.signals.written.get(),
                    "capture ran ahead of consumer"
                );
                let n = self.signals.captures.get() as usize;
                result = json!({"data":STANDARD.encode(&self.pngs[n.min(self.pngs.len()-1)])});
                self.signals.captures.set(n as u64 + 1);
            }
            "Runtime.evaluate" => {
                let expression = value["params"]["expression"].as_str().unwrap();
                let url = if self.navigate_away {
                    "https://other.invalid/".into()
                } else {
                    resource_url("opening.svg")?
                };
                if expression.contains("document.readyState") {
                    result = json!({"result":{"value":json!({"url":url,"state":"complete"}).to_string()}});
                } else if expression.contains("__reelSetFrame") {
                    let frame = self.signals.captures.get() + u64::from(self.bad_clock);
                    result =
                        json!({"result":{"value":json!({"url":url,"frame":frame}).to_string()}});
                }
                if self.exception {
                    result["exceptionDetails"] = json!({"text":"fixture failure"});
                }
            }
            _ => (),
        }
        let response = json!({"id":id,"result":result});
        if self.ack_first && value["method"] == "Fetch.fulfillRequest" {
            self.queue.push_front(response.clone());
        } else {
            self.queue.push_back(response.clone());
        }
        if self.duplicate {
            self.queue.push_back(response);
        }
        self.sent.push(value);
        Ok(())
    }
    fn receive(&mut self, _: usize, _: Duration) -> Result<String> {
        if self.oversize {
            return Ok(" ".repeat(16385));
        }
        if self.flood {
            return Ok(json!({"method":"Runtime.consoleAPICalled"}).to_string());
        }
        Ok(self
            .queue
            .pop_front()
            .context("fixture response queue empty")?
            .to_string())
    }
}
struct Map {
    signals: Rc<Signals>,
    fail_after: Option<u64>,
    oversupply: bool,
    deny: bool,
}
impl Resources for Map {
    fn check(&mut self) -> Result<()> {
        self.signals.checks.set(self.signals.checks.get() + 1);
        if self
            .fail_after
            .is_some_and(|n| self.signals.written.get() >= n)
        {
            bail!("fixture source protection lost");
        }
        Ok(())
    }
    fn get(&mut self, url: &str, max: u64) -> Result<(Vec<u8>, &'static str)> {
        if self.deny || url != resource_url("opening.svg")? {
            bail!("fixture resource not admitted");
        }
        let bytes = if self.oversupply {
            vec![0; max as usize + 1]
        } else {
            b"<svg />".to_vec()
        };
        if !self.oversupply && bytes.len() as u64 > max {
            bail!("fixture resource over budget");
        }
        Ok((bytes, "image/svg+xml"))
    }
}
struct Sink {
    signals: Rc<Signals>,
    finished: bool,
    fail_at: Option<u64>,
    fail_finish: bool,
}
impl Frames for Sink {
    fn write(&mut self, index: u64, _: &[u8]) -> Result<()> {
        if self.fail_at == Some(index) {
            bail!("fixture encoder stopped");
        }
        assert_eq!(index, self.signals.written.get());
        self.signals.written.set(index + 1);
        Ok(())
    }
    fn finish(&mut self) -> Result<()> {
        if self.fail_finish {
            bail!("fixture encoder final drain failed");
        }
        self.finished = true;
        Ok(())
    }
}
fn setup() -> (Wire, Map, Sink) {
    let signals = Rc::new(Signals::default());
    (
        Wire::new(signals.clone()),
        Map {
            signals: signals.clone(),
            fail_after: None,
            oversupply: false,
            deny: false,
        },
        Sink {
            signals,
            finished: false,
            fail_at: None,
            fail_finish: false,
        },
    )
}
fn run(w: &mut Wire, r: &mut Map, s: &mut Sink) -> Result<Captured> {
    capture(w, r, &manifest(), false, &limits(), s)
}

#[test]
fn services_paused_resources_with_both_ack_orders_and_streams_without_read_ahead() {
    for ack_first in [false, true] {
        let (mut w, mut r, mut s) = setup();
        w.ack_first = ack_first;
        let report = run(&mut w, &mut r, &mut s).unwrap();
        assert_eq!(
            (report.frames, report.requests, report.served_bytes),
            (3, 1, 7)
        );
        assert_eq!(report.frame_sequence_sha256.len(), 64);
        assert!(s.finished);
        let fulfill = w
            .sent
            .iter()
            .find(|v| v["method"] == "Fetch.fulfillRequest")
            .unwrap();
        assert_eq!(
            STANDARD
                .decode(fulfill["params"]["body"].as_str().unwrap())
                .unwrap(),
            b"<svg />"
        );
        assert!(
            fulfill["params"]["responseHeaders"]
                .as_array()
                .unwrap()
                .iter()
                .any(|h| h["name"] == "Content-Security-Policy")
        );
    }
}
#[test]
fn rejects_unknown_resources_and_foreign_origins_without_fallback() {
    for url in [
        "https://reel.invalid/v1/missing.png",
        "https://remote.invalid/x",
        "file:///secret",
        "https://reel.invalid/v1/opening.svg?x=1",
    ] {
        let (mut w, mut r, mut s) = setup();
        w.fetches = vec![fetch(url)];
        assert!(run(&mut w, &mut r, &mut s).is_err());
        assert!(!s.finished);
        assert_eq!(w.signals.captures.get(), 0);
        assert!(
            !w.sent
                .iter()
                .any(|v| v["method"] == "Fetch.continueRequest")
        );
    }
}
#[test]
fn rejects_body_range_redirect_and_non_get_requests() {
    for variant in 0..5 {
        let (mut w, mut r, mut s) = setup();
        let p = &mut w.fetches[0]["params"];
        match variant {
            0 => p["request"]["method"] = json!("POST"),
            1 => p["request"]["postData"] = json!("x"),
            2 => p["request"]["headers"]["rAnGe"] = json!("bytes=0-1"),
            3 => p["redirectedRequestId"] = json!("old"),
            _ => p["responseStatusCode"] = json!(302),
        }
        assert!(run(&mut w, &mut r, &mut s).is_err());
        assert!(!s.finished);
    }
}
#[test]
fn checks_fulfill_errors_even_when_the_main_response_arrives_first() {
    let (mut w, mut r, mut s) = setup();
    w.bad_ack = true;
    assert!(run(&mut w, &mut r, &mut s).is_err());
    assert!(!s.finished);
}
#[test]
fn bounds_requests_served_bytes_resources_and_provider_oversupply() {
    for variant in 0..4 {
        let (mut w, mut r, mut s) = setup();
        let mut l = limits();
        match variant {
            0 => {
                w.fetches.push(w.fetches[0].clone());
                l.max_requests = 1;
            }
            1 => l.max_served_bytes = 6,
            2 => l.max_resource_bytes = 6,
            _ => r.oversupply = true,
        }
        assert!(capture(&mut w, &mut r, &manifest(), false, &l, &mut s).is_err());
        assert!(!s.finished);
    }
}
#[test]
fn bounds_incoming_messages_and_event_floods() {
    for flood in [false, true] {
        let (mut w, mut r, mut s) = setup();
        w.flood = flood;
        w.oversize = !flood;
        assert!(run(&mut w, &mut r, &mut s).is_err());
        assert!(!s.finished);
    }
}
#[test]
fn refuses_duplicate_command_replies() {
    let (mut w, mut r, mut s) = setup();
    w.duplicate = true;
    assert!(run(&mut w, &mut r, &mut s).is_err());
    assert!(!s.finished);
}
#[test]
fn source_protection_loss_stops_after_the_last_accepted_frame() {
    let (mut w, mut r, mut s) = setup();
    r.fail_after = Some(1);
    assert!(run(&mut w, &mut r, &mut s).is_err());
    assert_eq!(s.signals.written.get(), 1);
    assert_eq!(w.signals.captures.get(), 1);
    assert!(!s.finished);
}
#[test]
fn consumer_failure_and_final_drain_failure_produce_no_completed_result() {
    for final_drain in [false, true] {
        let (mut w, mut r, mut s) = setup();
        s.fail_finish = final_drain;
        if !final_drain {
            s.fail_at = Some(1);
        }
        assert!(run(&mut w, &mut r, &mut s).is_err());
        assert!(!s.finished);
        assert_eq!(w.signals.captures.get(), if final_drain { 3 } else { 2 });
    }
}
#[test]
fn changed_clock_script_exception_and_navigation_do_not_complete() {
    for variant in 0..3 {
        let (mut w, mut r, mut s) = setup();
        w.bad_clock = variant == 0;
        w.exception = variant == 1;
        w.navigate_away = variant == 2;
        let mut l = limits();
        l.max_messages = 30;
        assert!(capture(&mut w, &mut r, &manifest(), false, &l, &mut s).is_err());
        assert!(!s.finished);
    }
}
#[test]
fn refuses_corrupt_oversize_and_wrong_geometry_pngs_before_consumer() {
    for variant in 0..3 {
        let (mut w, mut r, mut s) = setup();
        match variant {
            0 => w.pngs[0][40] ^= 255,
            1 => w.pngs[0] = vec![0; 1025],
            _ => w.pngs[0][19] = 9,
        }
        assert!(run(&mut w, &mut r, &mut s).is_err());
        assert_eq!(s.signals.written.get(), 0);
        assert!(!s.finished);
    }
}
#[test]
fn dynamic_region_allows_local_change_but_refuses_background_change() {
    for outside in [false, true] {
        let (mut w, mut r, mut s) = setup();
        let mut image = image::RgbaImage::from_pixel(8, 8, image::Rgba([10, 20, 30, 255]));
        image.put_pixel(
            if outside { 7 } else { 2 },
            2,
            image::Rgba([99, 80, 60, 255]),
        );
        w.pngs[1] = encode(image);
        let mut m = manifest();
        m.dynamic_regions = vec![PixelRegion {
            x: 2,
            y: 2,
            width: 1,
            height: 1,
        }];
        let result = capture(&mut w, &mut r, &m, false, &limits(), &mut s);
        assert_eq!(result.is_err(), outside);
        assert_eq!(s.finished, !outside);
    }
}
#[test]
fn refuses_geometry_budget_before_sending_any_browser_command() {
    for frames in [true, false] {
        let (mut w, mut r, mut s) = setup();
        let mut l = limits();
        if frames {
            l.max_frames = 2;
        } else {
            l.max_canvas_pixels = 63;
        }
        assert!(capture(&mut w, &mut r, &manifest(), false, &l, &mut s).is_err());
        assert!(w.sent.is_empty());
    }
}
#[test]
fn exact_urls_encode_spaces_unicode_and_literal_percent_without_traversal_aliases() {
    assert_eq!(
        resource_url("images/niño 1%.png").unwrap(),
        "https://reel.invalid/v1/images/ni%C3%B1o%201%25.png"
    );
    for path in ["../escape", "/absolute", "a\\b", "a/../b"] {
        assert!(resource_url(path).is_err());
    }
}
#[test]
fn guarded_provider_checks_exact_bytes_and_never_reads_an_unknown_url() {
    struct Files(std::path::PathBuf, Cell<u64>);
    impl InputFiles for Files {
        fn verified_path(&self, _: &crate::scene_delivery::FileRef) -> Result<std::path::PathBuf> {
            self.1.set(self.1.get() + 1);
            Ok(self.0.clone())
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("input.svg");
    std::fs::write(&path, b"<svg />").unwrap();
    let files = Files(path.clone(), Cell::new(0));
    let file = crate::scene_delivery::FileRef {
        path: "svg/main".into(),
        sha256: production::sha256_bytes(b"<svg />"),
        bytes: 7,
    };
    let url = resource_url("opening.svg").unwrap();
    let checks = Cell::new(0);
    let mut guarded = Guarded {
        files: &files,
        inputs: BTreeMap::from([(url.clone(), (&file, "image/svg+xml"))]),
        check: Box::new(|| {
            checks.set(checks.get() + 1);
            Ok(())
        }),
    };
    assert!(guarded.get("file:///other", 1024).is_err());
    assert_eq!(files.1.get(), 0);
    assert!(guarded.get(&url, 6).is_err());
    assert_eq!(files.1.get(), 0);
    assert_eq!(guarded.get(&url, 7).unwrap().0, b"<svg />");
    assert!(checks.get() >= 4);
    std::fs::write(&path, b"<bad />").unwrap();
    assert!(guarded.get(&url, 7).is_err());
    std::fs::write(&path, b"<svg />x").unwrap();
    assert!(guarded.get(&url, 7).is_err());
}

fn socket_pair() -> (WebSocketTransport, std::net::TcpStream) {
    use tungstenite::{WebSocket, protocol::Role, stream::MaybeTlsStream};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let client = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (server, _) = listener.accept().unwrap();
    server
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    server
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    (
        WebSocketTransport::new(
            WebSocket::from_raw_socket(MaybeTlsStream::Plain(client), Role::Client, None),
            1024,
        )
        .unwrap(),
        server,
    )
}
#[test]
fn real_websocket_transport_enforces_message_bounds_and_roundtrips_commands() {
    use tungstenite::{Message, WebSocket, protocol::Role};
    for oversized in [false, true] {
        let (mut transport, server) = socket_pair();
        let thread = std::thread::spawn(move || {
            let mut ws = WebSocket::from_raw_socket(server, Role::Server, None);
            ws.send(Message::Text(
                if oversized {
                    "x".repeat(1025)
                } else {
                    "fixture".into()
                }
                .into(),
            ))
            .unwrap();
            if !oversized {
                assert_eq!(ws.read().unwrap().to_text().unwrap(), "command");
            }
        });
        let message = transport.receive(1024, Duration::from_secs(1));
        if oversized {
            assert!(message.is_err());
        } else {
            assert_eq!(message.unwrap(), "fixture");
            transport.send("command", Duration::from_secs(1)).unwrap();
        }
        thread.join().unwrap();
    }
}
#[test]
fn incomplete_trickling_websocket_frame_cannot_extend_absolute_deadline() {
    use std::io::Write;
    let (mut transport, mut server) = socket_pair();
    let thread = std::thread::spawn(move || {
        server.write_all(&[0x81, 100]).unwrap();
        for _ in 0..100 {
            if server.write_all(b"x").is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    });
    let start = Instant::now();
    assert!(transport.receive(1024, Duration::from_millis(25)).is_err());
    assert!(start.elapsed() < Duration::from_secs(1));
    drop(transport);
    thread.join().unwrap();
}
#[test]
fn clean_picture_mode_is_an_explicit_command_and_late_corruption_never_finishes() {
    let (mut w, mut r, mut s) = setup();
    capture(&mut w, &mut r, &manifest(), true, &limits(), &mut s).unwrap();
    assert!(w.sent.iter().any(|v| {
        v["params"]["expression"]
            .as_str()
            .is_some_and(|s| s.contains("reelCleanPicture"))
    }));
    let (mut w, mut r, mut s) = setup();
    w.pngs[2][40] ^= 255;
    assert!(run(&mut w, &mut r, &mut s).is_err());
    assert!(!s.finished);
    assert_eq!(s.signals.written.get(), 2);
}

#[test]
fn denied_document_resource_policy_and_detachment_refuse_completion() {
    for event in [
        json!({"method":"Runtime.bindingCalled","params":{"name":"__reelResourceRefusal","payload":"csp"}}),
        json!({"method":"Inspector.detached"}),
        json!({"method":"Fetch.authRequired"}),
    ] {
        let (mut w, mut r, mut s) = setup();
        w.fetches = vec![event];
        assert!(run(&mut w, &mut r, &mut s).is_err());
        assert!(!s.finished);
        assert_eq!(s.signals.written.get(), 0);
    }
}

#[test]
fn a_previously_loaded_document_cannot_substitute_for_the_admitted_source() {
    let (mut w, mut r, mut s) = setup();
    w.fetches.clear();
    assert!(run(&mut w, &mut r, &mut s).is_err());
    assert_eq!(s.signals.written.get(), 0);
    assert!(!s.finished);
}

#[test]
fn real_cdp_fractional_timestamps_are_wire_values_and_duplicates_still_refuse() {
    let value=protocol::parse(r#"{"method":"Network.requestWillBeSent","params":{"timestamp":123.125,"values":[-0.0,1e-5,null,true]}}"#).unwrap();
    assert_eq!(value["params"]["timestamp"].as_f64(), Some(123.125));
    for raw in [
        r#"{"id":1,"id":2}"#,
        r#"{"params":{"timestamp":1,"timestamp":2}}"#,
        r#"{"params":[{"x":1,"x":2}]}"#,
        r#"{"timestamp":1e999}"#,
    ] {
        assert!(protocol::parse(raw).is_err());
    }
    assert!(tig_core::identity::parse_json(b"{\"timestamp\":123.125}").is_err());
    let (mut w, mut r, mut s) = setup();
    w.fetches.insert(
        0,
        serde_json::json!({"method":"Network.requestWillBeSent","params":{"timestamp":123.125}}),
    );
    assert!(run(&mut w, &mut r, &mut s).is_ok());
}

#[test]
fn browser_tab_icon_is_denied_without_exempting_an_authored_image_or_serving_bytes() {
    for kind in ["Other", "Image", "Document"] {
        let (mut w, mut r, mut s) = setup();
        let mut icon = fetch("https://reel.invalid/favicon.ico");
        icon["params"]["resourceType"] = json!(kind);
        w.fetches.push(icon);
        let report = run(&mut w, &mut r, &mut s);
        if kind == "Other" {
            let report = report.unwrap();
            assert_eq!(report.denied_auxiliary_requests, 1);
            assert_eq!(report.served_bytes, 7);
            assert!(w.sent.iter().any(|v| v["method"] == "Fetch.failRequest"
                && v["params"]["errorReason"] == "BlockedByClient"));
        } else {
            assert!(report.is_err());
            assert!(!s.finished);
        }
    }
}

#[test]
fn browser_url_spellings_resolve_one_declared_utf8_path_without_decoding_traversal() {
    let expected = resource_url("pictures/niño 1%+&.svg").unwrap();
    for url in [
        "https://reel.invalid/v1/pictures/ni%C3%B1o%201%+&.svg",
        "https://reel.invalid/v1/pictures/ni%c3%b1o%201%25%2b%26.svg",
    ] {
        assert_eq!(canonical_resource_url(url).unwrap(), expected);
    }
    for url in [
        "https://reel.invalid/v1/a%2fb.svg",
        "https://reel.invalid/v1/a%5cb.svg",
        "https://reel.invalid/v1/%2e%2e/secret.svg",
        "https://reel.invalid/v1/a.svg?q=1",
        "https://reel.invalid/v1/a.svg#fragment",
        "https://other.invalid/v1/a.svg",
    ] {
        assert!(canonical_resource_url(url).is_err());
    }
}
