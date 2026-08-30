use std::fs;

#[test]
fn validates_sanitized_exact_lyrics_fixture() {
    let report = reel::song::validate(std::path::Path::new(
        "manifests/fixtures/song-generation/song.yaml",
    ))
    .expect("fixture validates");
    assert!(report.verified);
    assert!(report.human_listening_required);
    assert!(!report.public_release_declared);
    assert_eq!(report.lyric_lines, 2);
}

#[test]
fn packet_is_reproducible_and_receipt_does_not_disclose_lyrics_or_paths() {
    let temp = tempfile::tempdir().expect("tempdir");
    let packet = temp.path().join("packet");
    let manifest = std::path::Path::new("manifests/fixtures/song-generation/song.yaml");
    reel::song::write_plan(manifest, &packet).expect("packet writes");
    reel::song::check(&packet, manifest).expect("packet checks");

    let receipt = fs::read_to_string(packet.join("receipt.json")).expect("receipt reads");
    assert!(!receipt.contains("Canta la luz"));
    assert!(!receipt.contains("lyrics.txt"));
    assert!(!receipt.contains("working_directory"));
    let request = fs::read_to_string(packet.join("request.json")).expect("request reads");
    assert!(request.contains("Canta la luz"));
    assert!(!request.contains("Historic Cuban salon-song"));
}

#[test]
fn rejects_stale_lyrics_hash_and_remote_egress() {
    let fixture =
        fs::read_to_string("manifests/fixtures/song-generation/song.yaml").expect("fixture reads");
    let temp = tempfile::tempdir().expect("tempdir");
    fs::write(temp.path().join("lyrics.txt"), "texto cambiado\n").expect("lyrics write");
    fs::write(temp.path().join("song.yaml"), &fixture).expect("manifest write");
    let error =
        reel::song::validate(&temp.path().join("song.yaml")).expect_err("stale hash rejected");
    assert!(error.to_string().contains("lyrics sha256"));

    fs::copy(
        "manifests/fixtures/song-generation/lyrics.txt",
        temp.path().join("lyrics.txt"),
    )
    .expect("lyrics restored");
    let remote = fixture.replace("third_party_upload: false", "third_party_upload: true");
    fs::write(temp.path().join("song.yaml"), remote).expect("remote manifest write");
    let error =
        reel::song::validate(&temp.path().join("song.yaml")).expect_err("remote egress rejected");
    assert!(error.to_string().contains("third_party_upload"));
}

#[test]
fn keeps_release_and_assigned_voice_consent_as_explicit_human_gates() {
    let fixture =
        fs::read_to_string("manifests/fixtures/song-generation/song.yaml").expect("fixture reads");
    let temp = tempfile::tempdir().expect("tempdir");
    fs::copy(
        "manifests/fixtures/song-generation/lyrics.txt",
        temp.path().join("lyrics.txt"),
    )
    .expect("lyrics copied");

    let release = fixture.replace("public_release: false", "public_release: true");
    fs::write(temp.path().join("song.yaml"), release).expect("release manifest write");
    let error = reel::song::validate(&temp.path().join("song.yaml")).expect_err("release rejected");
    assert!(error.to_string().contains("separate decision"));

    let assigned = fixture.replace(
        "voice_identity: original-unassigned",
        "voice_identity: named-performer",
    );
    fs::write(temp.path().join("song.yaml"), assigned).expect("voice manifest write");
    let error = reel::song::validate(&temp.path().join("song.yaml"))
        .expect_err("assigned voice without consent rejected");
    assert!(error.to_string().contains("recorded consent evidence"));
}

#[test]
fn doctor_reports_local_fixture_ready_without_downloading() {
    let report = reel::song::doctor(std::path::Path::new(
        "manifests/fixtures/song-generation/song.yaml",
    ))
    .expect("doctor runs");
    assert!(report.executable_found);
    assert!(report.working_directory_exists);
    assert!(report.ready);
}
