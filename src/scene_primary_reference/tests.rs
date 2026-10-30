use super::*;
#[test]
fn checksum_protocols_require_every_exact_stream_and_sha256() {
    let hash = "a".repeat(64);
    let old = format!("0,v,SHA256={hash}\n1,v,SHA256={hash}\n2,a,SHA256={hash}\n");
    let current = format!(
        "#format: stream checksums\n#version: 2\n#hash: SHA256\n#media_type 0: video\n#codec_id 0: rawvideo\n#media_type 1: video\n#codec_id 1: rawvideo\n#media_type 2: audio\n#codec_id 2: pcm_s24le\n0,123,{hash}\n1,123,{hash}\n2,456,{hash}\n"
    );
    for raw in [&old, &current] {
        assert_eq!(
            parse_hashes(raw.as_bytes(), &['v', 'v', 'a']).unwrap(),
            vec![hash.clone(); 3]
        );
    }
    for bad in [
        old.replace("1,v,", "1,a,"),
        old.replace("SHA256=", "SHA1="),
        format!("{old}0,v,SHA256={hash}\n"),
        old.lines().take(2).collect::<Vec<_>>().join("\n"),
        current.replace("#version: 2", "#version: 3"),
        current.replace("#codec_id 2: pcm_s24le", "#codec_id 2: aac"),
        current.replace("2,456,", "2,0,"),
    ] {
        assert!(parse_hashes(bad.as_bytes(), &['v', 'v', 'a']).is_err());
    }
}
