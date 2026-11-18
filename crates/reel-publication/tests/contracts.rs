use reel_publication::*;
use std::collections::BTreeMap;

fn asset(id: &str, hash: char) -> Artifact {
    Artifact {
        logical_id: id.into(),
        sha256: hash.to_string().repeat(64),
        bytes: 20,
        media_type: "application/json".into(),
    }
}
fn outputs() -> OutputContracts {
    BTreeMap::from([(
        "reading-book".into(),
        OutputContract {
            media_type: "application/pdf".into(),
            max_bytes: 1_000_000,
            required: true,
        },
    )])
}
fn page(id: &str) -> ComicPage {
    ComicPage {
        id: id.into(),
        kind: PageKind::Story,
        source: asset(&format!("source/{id}"), 'a'),
        panel_order: vec!["panel-a".into(), "panel-b".into()],
        utterance_order: vec!["line-a".into(), "line-b".into()],
        spread_id: None,
    }
}
fn publication() -> Publication {
    Publication {
        schema: PUBLICATION_SCHEMA.into(),
        id: "synthetic-comic".into(),
        editions: vec![Edition {
            id: "es-reading".into(),
            language: "es".into(),
            content: Content::Comic {
                reading_direction: ReadingDirection::LeftToRight,
                pages: vec![page("page01"), page("page02")],
            },
            outputs: outputs(),
        }],
    }
}
#[test]
fn issue_part_metadata_is_available_without_a_renderer_and_preserves_source_identity() {
    use reel_publication::comic_issue_source::{Part, Plan};
    let mut work = publication();
    let Content::Comic { pages, .. } = &mut work.editions[0].content else {
        unreachable!()
    };
    pages[0].spread_id = Some("spread-a".into());
    pages[1].spread_id = Some("spread-a".into());
    let mut plan = Plan {
        schema: "reel.comic-issue-plan.v1".into(),
        publication: work.clone(),
        edition_id: "es-reading".into(),
        parts: vec![Part {
            id: "spread".into(),
            pages: vec!["page01".into(), "page02".into()],
        }],
    };
    plan.check().unwrap();
    assert_eq!(plan.part_publication("spread").unwrap(), work);
    plan.parts = vec![
        Part {
            id: "left".into(),
            pages: vec!["page01".into()],
        },
        Part {
            id: "right".into(),
            pages: vec!["page02".into()],
        },
    ];
    assert_eq!(plan.check().unwrap_err().code, "issue-part-split-spread");
}
#[test]
fn issue_plan_cli_is_metadata_only_even_without_the_comic_feature() {
    use reel_publication::comic_issue_source::{Part, Plan};
    let plan = Plan {
        schema: "reel.comic-issue-plan.v1".into(),
        publication: publication(),
        edition_id: "es-reading".into(),
        parts: vec![Part {
            id: "all".into(),
            pages: vec!["page01".into(), "page02".into()],
        }],
    };
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("issue.json");
    std::fs::write(&file, serde_json::to_vec(&plan).unwrap()).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_reel-publication"))
        .arg("comic-issue-check")
        .arg(file)
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["payloads_verified"], false);
    assert_eq!(report["outputs_written"], false);
}
fn pages(publication: &mut Publication) -> &mut Vec<ComicPage> {
    let Content::Comic { pages, .. } = &mut publication.editions[0].content else {
        panic!()
    };
    pages
}

#[test]
fn comics_have_spatial_order_and_language_editions_without_film_clocks() {
    let mut work = publication();
    let mut translated = work.editions[0].clone();
    translated.id = "en-reading".into();
    translated.language = "en".into();
    let Content::Comic { pages, .. } = &mut translated.content else {
        panic!()
    };
    pages[0].source = asset("source/page01-en", 'b');
    pages[0].utterance_order.reverse();
    work.editions.push(translated);
    check_publication(&work).unwrap();
    let encoded = serde_json::to_string(&work).unwrap();
    assert!(!encoded.contains("duration"));
    assert!(!encoded.contains("sample"));
}

#[test]
fn films_reference_existing_composition_contracts() {
    let mut work = publication();
    work.editions[0].content = Content::Film {
        composition: asset("film/source", 'a'),
    };
    work.editions[0]
        .outputs
        .get_mut("reading-book")
        .unwrap()
        .media_type = "video/mp4".into();
    check_publication(&work).unwrap();
}

#[test]
fn spreads_require_two_adjacent_pages_without_claiming_print_parity() {
    let mut work = publication();
    pages(&mut work)[0].spread_id = Some("spread-a".into());
    assert_eq!(
        check_publication(&work).unwrap_err().code,
        "spread-page-pair"
    );
    pages(&mut work)[1].spread_id = Some("spread-a".into());
    check_publication(&work).unwrap();
    pages(&mut work).insert(1, page("insert"));
    assert_eq!(
        check_publication(&work).unwrap_err().code,
        "spread-page-pair"
    );
}

#[test]
fn duplicate_pages_panels_and_utterances_refuse() {
    let mut work = publication();
    pages(&mut work)[1].id = "page01".into();
    assert!(check_publication(&work).is_err());
    work = publication();
    pages(&mut work)[0].panel_order.push("panel-a".into());
    assert!(check_publication(&work).is_err());
    work = publication();
    pages(&mut work)[0].utterance_order.push("line-a".into());
    assert!(check_publication(&work).is_err());
}

#[test]
fn silent_story_and_panel_free_cover_are_valid() {
    let mut work = publication();
    pages(&mut work)[0].utterance_order.clear();
    pages(&mut work)[1].kind = PageKind::Cover;
    pages(&mut work)[1].panel_order.clear();
    pages(&mut work)[1].utterance_order.clear();
    check_publication(&work).unwrap();
    pages(&mut work)[1].kind = PageKind::Story;
    assert!(check_publication(&work).is_err());
}

#[test]
fn named_outputs_require_complete_declared_sets_and_consistent_identity() {
    let contracts = BTreeMap::from([
        (
            "clean-art".into(),
            OutputContract {
                media_type: "image/png".into(),
                max_bytes: 100,
                required: true,
            },
        ),
        (
            "lettering".into(),
            OutputContract {
                media_type: "application/json".into(),
                max_bytes: 100,
                required: true,
            },
        ),
        (
            "preview".into(),
            OutputContract {
                media_type: "image/png".into(),
                max_bytes: 100,
                required: false,
            },
        ),
    ]);
    let mut art = asset("page/clean-art", 'a');
    art.media_type = "image/png".into();
    let mut result = ArtifactSet::from([("clean-art".into(), art)]);
    assert_eq!(
        check_outputs(&contracts, &result).unwrap_err().kind,
        ErrorKind::Missing
    );
    result.insert("lettering".into(), asset("page/lettering", 'b'));
    check_outputs(&contracts, &result).unwrap();
    result.insert("extra".into(), asset("page/extra", 'c'));
    assert_eq!(
        check_outputs(&contracts, &result).unwrap_err().code,
        "undeclared-output"
    );
    result.remove("extra");
    result.get_mut("lettering").unwrap().logical_id = "page/clean-art".into();
    assert_eq!(
        check_outputs(&contracts, &result).unwrap_err().kind,
        ErrorKind::Conflict
    );
}

#[test]
fn output_totals_are_bounded_and_unknown_contract_fields_refuse() {
    let mut contracts = outputs();
    contracts.get_mut("reading-book").unwrap().max_bytes = u64::MAX;
    contracts.insert(
        "other".into(),
        OutputContract {
            media_type: "text/plain".into(),
            max_bytes: 1,
            required: true,
        },
    );
    assert_eq!(
        check_contracts(&contracts).unwrap_err().kind,
        ErrorKind::Budget
    );
    let mut value = serde_json::to_value(publication()).unwrap();
    value["editions"][0]["content"]["seconds_per_panel"] = serde_json::json!(5);
    assert!(serde_json::from_value::<Publication>(value).is_err());
}

#[test]
fn cli_checks_metadata_without_reading_nonexistent_assets_or_claiming_print_ready() {
    let root = tempfile::tempdir().unwrap();
    let manifest = root.path().join("publication.json");
    std::fs::write(&manifest, serde_json::to_vec(&publication()).unwrap()).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_reel-publication"))
        .arg("check")
        .arg(&manifest)
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["print_ready"], false);
    assert_eq!(report["payloads_verified"], false);
    assert!(
        !String::from_utf8(output.stdout)
            .unwrap()
            .contains("synthetic-comic")
    );
    std::fs::write(&manifest, b"private-invalid-prompt").unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_reel-publication"))
        .arg("check")
        .arg(&manifest)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        !String::from_utf8(output.stdout)
            .unwrap()
            .contains("private-invalid-prompt")
    );
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}
