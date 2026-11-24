//! Local comic board from pinned reading receipts and prepared source order.
use crate::{Board, Designation, Format, Language, Media, Scene, Unit, Version, error};
use reel_workflow::{Artifact, ErrorKind, Result, digest};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

#[derive(Deserialize)]
struct Receipt {
    schema: String,
    publication_id: String,
    language: String,
    book: Pin,
    pages: Vec<Page>,
}

#[derive(Deserialize)]
struct Book {
    schema: String,
    publication_id: String,
    language: String,
    pages: Vec<SourcePage>,
}

#[derive(Deserialize, PartialEq, Eq)]
struct Pin {
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize)]
struct Page {
    page_id: String,
    sheet: usize,
    reading_png: Pin,
    svg: Pin,
}

#[derive(Deserialize)]
struct SourcePage {
    page_id: String,
    sheet: usize,
    selected_png: Pin,
    selected_svg: Pin,
    review_ids: Option<Vec<String>>,
}

fn pinned(path: &Path, sha: &str) -> Result<Vec<u8>> {
    let mut raw = Vec::new();
    File::open(path)
        .map_err(|_| error(ErrorKind::Missing, "comic-board-control-open"))?
        .take(crate::MAX_CONTROL_BYTES + 1)
        .read_to_end(&mut raw)
        .map_err(|_| error(ErrorKind::Integrity, "comic-board-control-read"))?;
    if raw.len() as u64 > crate::MAX_CONTROL_BYTES || digest(&raw) != sha {
        return Err(error(ErrorKind::Integrity, "comic-board-control-pin"));
    }
    Ok(raw)
}

pub fn export(
    prepared_book: &Path,
    review: &Path,
    receipt_sha: &str,
    source_commit: &str,
    output: &Path,
) -> Result<Artifact> {
    let review = std::path::absolute(review)
        .map_err(|_| error(ErrorKind::Invalid, "comic-board-review-path"))?;
    let receipt_raw = pinned(&review.join("receipt.json"), receipt_sha)?;
    let receipt: Receipt = serde_json::from_slice(&receipt_raw)
        .map_err(|_| error(ErrorKind::Invalid, "comic-board-receipt-json"))?;
    let raw = pinned(prepared_book, &receipt.book.sha256)?;
    let book: Book = serde_json::from_slice(&raw)
        .map_err(|_| error(ErrorKind::Invalid, "comic-board-book-json"))?;
    if receipt.schema != "reel.layered-book-receipt.v1"
        || book.schema != "reel.bbj-book-import.v1"
        || raw.len() as u64 != receipt.book.bytes
        || receipt.publication_id != book.publication_id
        || receipt.language != book.language
        || !(1..=128).contains(&receipt.pages.len())
        || receipt.pages.len() != book.pages.len()
    {
        return Err(error(ErrorKind::Conflict, "comic-board-edition-binding"));
    }
    let mut board = Board {
        schema: "reel.workflow-board.v1".into(),
        project: book.publication_id.clone(),
        work_id: book.publication_id.clone(),
        title: book.publication_id,
        format: Format::Comic,
        source_commit: source_commit.into(),
        projection_request: Some(Artifact {
            logical_id: "workflow/comic-reading-receipt".into(),
            sha256: receipt_sha.into(),
            bytes: receipt_raw.len() as u64,
            media_type: "application/json".into(),
        }),
        approvals: vec![],
        synthetic: false,
        languages: vec![Language {
            id: book.language.clone(),
            label: book.language.clone(),
        }],
        scenes: vec![Scene {
            id: "issue".into(),
            title: "Issue".into(),
        }],
        units: vec![],
        cues: vec![],
        media: BTreeMap::new(),
        tasks: BTreeMap::new(),
    };
    for (index, (page, source)) in receipt.pages.into_iter().zip(book.pages).enumerate() {
        if page.sheet != index + 1
            || page.sheet != source.sheet
            || page.page_id != source.page_id
            || page.reading_png != source.selected_png
            || page.svg != source.selected_svg
        {
            return Err(error(ErrorKind::Conflict, "comic-board-page-binding"));
        }
        let key = format!("sheet-{:03}", page.sheet);
        let png = Artifact {
            logical_id: format!("workflow/{}/reading", page.page_id),
            sha256: page.reading_png.sha256,
            bytes: page.reading_png.bytes,
            media_type: "image/png".into(),
        };
        board.media.insert(
            key.clone(),
            Media {
                artifact: png.clone(),
                path: review.join(&key).join("page.png"),
                subject: None,
            },
        );
        board.units.push(Unit {
            id: page.page_id.clone(),
            story_id: page.page_id.clone(),
            display_id: page.page_id.clone(),
            semantic_id: page.page_id,
            scene_id: "issue".into(),
            title: format!("Sheet {}", page.sheet),
            versions: vec![Version {
                id: "reading".into(),
                label: "Reading copy".into(),
                asset: png,
                designation: Designation::Candidate,
                language: Some(book.language.clone()),
                preview: Some(key),
                approval_reference: None,
            }],
            spans: vec![],
            review_spans: vec![],
            task: None,
            review_targets: source.review_ids.unwrap_or_default(),
        });
    }
    board.check()?;
    let raw = serde_json::to_vec_pretty(&board)
        .map_err(|_| error(ErrorKind::Invalid, "comic-board-json"))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|_| error(ErrorKind::Conflict, "comic-board-new-output"))?;
    file.write_all(&raw)
        .and_then(|_| file.sync_all())
        .map_err(|_| error(ErrorKind::NeedsRecovery, "comic-board-output-write"))?;
    Ok(Artifact {
        logical_id: "workflow/comic-board".into(),
        sha256: digest(&raw),
        bytes: raw.len() as u64,
        media_type: "application/json".into(),
    })
}
