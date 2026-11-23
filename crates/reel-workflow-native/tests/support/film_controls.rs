use super::*;
use reel::scene_delivery::{self, repair};
use reel_workflow_native::{InputReader, film_controls};

struct Reader(BTreeMap<String, Vec<u8>>);
impl InputReader for Reader {
    fn read(&mut self, a: &Artifact, maximum: u64) -> Result<Vec<u8>> {
        assert!(a.bytes <= maximum);
        Ok(self.0[&a.logical_id].clone())
    }
}
fn fixture(change: Option<&str>) -> (Packet, Reader, film_controls::Derivation) {
    let temp = tempfile::tempdir().unwrap();
    film_job::fixture(temp.path());
    let original: scene_delivery::Job =
        serde_json::from_slice(&fs::read(temp.path().join("job.json")).unwrap()).unwrap();
    let spec = repair::StillReplacement {
        schema: "reel.scene-still-replacement.v1".into(),
        attachments: vec!["p-a".into()],
        expected: original.pictures[0].source.clone(),
        replacement: scene_delivery::FileRef {
            path: "replacement.png".into(),
            sha256: digest(b"PNG candidate"),
            bytes: 13,
        },
    };
    let mut next = repair::derive(&original, &spec).unwrap().job;
    match change {
        Some("clock") => next.contract.sha256 = digest(b"wrong language clock"),
        Some("take") => next.audio[0].source.sha256 = digest(b"wrong recording"),
        Some("duration") => next.max_composition_samples += 1,
        Some("geometry") => next.pictures[0].source_start_frame = 1,
        None => {}
        _ => unreachable!(),
    }
    let mut f = film_input_fixture();
    let mut data = BTreeMap::new();
    let mut artifact = |id: &str, raw: Vec<u8>| {
        let a = Artifact {
            logical_id: id.into(),
            sha256: digest(&raw),
            bytes: raw.len() as u64,
            media_type: "application/json".into(),
        };
        data.insert(id.into(), raw);
        a
    };
    let old = artifact("scene/original", serde_json::to_vec(&original).unwrap());
    let new = artifact("scene/candidate", serde_json::to_vec(&next).unwrap());
    let t = &mut f.packet.task;
    t.inputs = vec![old.clone(), new.clone()];
    let tool = t.tool.as_mut().unwrap();
    tool.publication = new;
    tool.request = old.clone();
    tool.source_bindings = BTreeMap::from([
        ("job.json".into(), "scene/candidate".into()),
        ("baseline-job.json".into(), old.logical_id.clone()),
    ]);
    for (id, r) in [
        ("picture/old", &spec.expected),
        ("picture/new", &spec.replacement),
    ] {
        t.inputs.push(Artifact {
            logical_id: id.into(),
            sha256: r.sha256.clone(),
            bytes: r.bytes,
            media_type: "image/png".into(),
        });
        tool.source_bindings
            .insert(r.path.to_str().unwrap().into(), id.into());
    }
    f.repin();
    (
        f.packet,
        Reader(data),
        film_controls::Derivation {
            schema: "reel.workflow-film-control-derivation.v1".into(),
            original_job: old,
            replacement: spec,
        },
    )
}
#[test]
fn payload_check_accepts_only_the_exact_still_change() {
    let (p, mut reader, s) = fixture(None);
    let proof = film_controls::verify(&p, &mut reader, &s).unwrap();
    assert!(!proof.creative_approval);
    for change in ["clock", "take", "duration", "geometry"] {
        let (p, mut reader, s) = fixture(Some(change));
        assert_eq!(
            film_controls::verify(&p, &mut reader, &s)
                .err()
                .unwrap()
                .code,
            "film-control-unrequested-change"
        );
    }
}
#[test]
fn wrong_source_alias_and_corrupt_control_bytes_refuse() {
    let (p, mut reader, mut s) = fixture(None);
    s.replacement.replacement.path = "undeclared.png".into();
    assert!(film_controls::verify(&p, &mut reader, &s).is_err());
    let (p, mut reader, s) = fixture(None);
    reader
        .0
        .get_mut(&s.original_job.logical_id)
        .unwrap()
        .push(b' ');
    assert_eq!(
        film_controls::verify(&p, &mut reader, &s)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Integrity
    );
}
