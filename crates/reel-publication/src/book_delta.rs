//! Compare exact local review candidates before an approved selection changes.
use crate::comic::{object, read};
use crate::comic_source::Object;
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::{collections::BTreeSet, path::Path};

#[derive(Deserialize)]
struct Receipt {
    schema: String,
    publication_id: String,
    language: String,
    pages: Vec<Page>,
}
#[derive(Deserialize, PartialEq, Eq)]
struct Page {
    page_id: String,
    sheet: usize,
    request: Object,
    source_receipt: Object,
    svg: Object,
    reading_png: Object,
}

fn open(root: &Path, expected: &str) -> Result<Receipt> {
    let raw = read(&root.join("receipt.json"), crate::MAX_CONTROL_BYTES as u64)?;
    ensure!(object(&raw).sha256 == expected, "book receipt pin");
    let receipt: Receipt = serde_json::from_slice(&raw)?;
    ensure!(
        receipt.schema == "reel.layered-book-receipt.v1"
            && !receipt.pages.is_empty()
            && receipt.pages.len() <= 128,
        "book receipt contract"
    );
    let mut ids = BTreeSet::new();
    for (index, page) in receipt.pages.iter().enumerate() {
        ensure!(
            page.sheet == index + 1 && crate::valid_id(&page.page_id) && ids.insert(&page.page_id),
            "book page identity or order"
        );
        let sheet = root.join(format!("sheet-{:03}", page.sheet));
        for (name, pin, limit) in [
            ("receipt.json", &page.source_receipt, 1024 * 1024),
            ("page.svg", &page.svg, 256 * 1024 * 1024),
            ("page.png", &page.reading_png, 16 * 1024 * 1024),
        ] {
            ensure!(pin.bytes <= limit, "book page byte limit");
            let bytes = read(&sheet.join(name), pin.bytes)?;
            ensure!(object(&bytes) == *pin, "book page payload pin");
        }
    }
    Ok(receipt)
}

pub fn check(
    baseline: &Path,
    baseline_sha: &str,
    candidate: &Path,
    candidate_sha: &str,
    target: &str,
) -> Result<usize> {
    let baseline = open(baseline, baseline_sha).context("baseline")?;
    let candidate = open(candidate, candidate_sha).context("candidate")?;
    ensure!(
        baseline.publication_id == candidate.publication_id
            && baseline.language == candidate.language
            && baseline.pages.len() == candidate.pages.len(),
        "edition identity"
    );
    let mut corrected = false;
    for (old, new) in baseline.pages.iter().zip(&candidate.pages) {
        ensure!(old.page_id == new.page_id, "page order changed");
        if old.page_id == target {
            ensure!(
                old.svg != new.svg && old.reading_png != new.reading_png,
                "target pixels or SVG unchanged"
            );
            corrected = true;
        } else {
            ensure!(old == new, "unaffected page changed");
        }
    }
    ensure!(corrected, "target not in edition");
    Ok(baseline.pages.len() - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    fn book(root: &Path, first: &[u8], second: &[u8]) -> String {
        let pages: Vec<_> = [first, second]
            .into_iter()
            .enumerate()
            .map(|(index, bytes)| {
                let folder = root.join(format!("sheet-{:03}", index + 1));
                fs::create_dir_all(&folder).unwrap();
                for file in ["receipt.json", "page.svg", "page.png"] {
                    fs::write(folder.join(file), bytes).unwrap();
                }
                let pin = object(bytes);
                json!({"page_id":format!("page-{}", index + 1),"sheet":index + 1,
                "request":pin,"source_receipt":pin,"svg":pin,"reading_png":pin})
            })
            .collect();
        let receipt = serde_json::to_vec(&json!({"schema":"reel.layered-book-receipt.v1",
            "publication_id":"book","language":"en","pages":pages}))
        .unwrap();
        fs::write(root.join("receipt.json"), &receipt).unwrap();
        object(&receipt).sha256
    }

    #[test]
    fn checks_one_changed_page_and_refuses_unaffected_changes_and_tampering() {
        let temp = tempfile::tempdir().unwrap();
        let baseline = temp.path().join("baseline");
        let candidate = temp.path().join("candidate");
        let old_sha = book(&baseline, b"old", b"stable");
        let new_sha = book(&candidate, b"corrected", b"stable");
        assert_eq!(
            check(&baseline, &old_sha, &candidate, &new_sha, "page-1").unwrap(),
            1
        );
        assert!(check(&baseline, &old_sha, &candidate, &new_sha, "page-2").is_err());
        fs::write(candidate.join("sheet-002/page.png"), b"forged").unwrap();
        assert!(check(&baseline, &old_sha, &candidate, &new_sha, "page-1").is_err());
        let new_sha = book(&candidate, b"corrected", b"also changed");
        assert!(check(&baseline, &old_sha, &candidate, &new_sha, "page-1").is_err());
    }
}
