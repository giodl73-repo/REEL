//! Explicit comic parts and private ordered reading assembly. This is not a
//! native job planner: actor, source-closure and retention admission remain separate.
use crate::{
    Artifact, ArtifactSet, ComicPage, Content, Publication, comic, comic_source::*, valid_id,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub id: String,
    pub pages: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub schema: String,
    pub publication: Publication,
    pub edition_id: String,
    /// Authored contiguous groups, in reading order; never silently chunked.
    pub parts: Vec<Part>,
}
/// Bounded, read-only metadata decoding. Call `Plan::check` to validate it.
pub fn read_plan(path: &Path) -> Result<Plan> {
    let raw = comic::read(&std::path::absolute(path)?, crate::MAX_CONTROL_BYTES as u64)?;
    Ok(serde_json::from_slice(&raw)?)
}
impl Plan {
    pub fn check(&self) -> Result<()> {
        ensure!(
            serde_json::to_vec(self)?.len() <= crate::MAX_CONTROL_BYTES,
            "issue plan byte bound exceeded"
        );
        crate::check_publication(&self.publication)?;
        ensure!(
            self.schema == "reel.comic-issue-plan.v1" && (1..=128).contains(&self.parts.len()),
            "issue plan contract refused"
        );
        let pages = self.pages()?;
        let mut offset = 0;
        let mut names = std::collections::BTreeSet::new();
        for part in &self.parts {
            ensure!(
                valid_id(&part.id)
                    && names.insert(part.id.to_ascii_lowercase())
                    && (1..=32).contains(&part.pages.len()),
                "issue part identity/count refused"
            );
            let end = offset + part.pages.len();
            ensure!(
                end <= pages.len()
                    && part
                        .pages
                        .iter()
                        .eq(pages[offset..end].iter().map(|p| &p.id)),
                "issue part page order/coverage differs"
            );
            if end < pages.len() {
                ensure!(
                    pages[end - 1].spread_id.is_none()
                        || pages[end - 1].spread_id != pages[end].spread_id,
                    "issue part splits spread"
                );
            }
            offset = end;
        }
        ensure!(offset == pages.len(), "issue pages missing");
        Ok(())
    }
    fn pages(&self) -> Result<&[ComicPage]> {
        let edition = self
            .publication
            .editions
            .iter()
            .find(|e| e.id == self.edition_id)
            .context("issue edition absent")?;
        let Content::Comic { pages, .. } = &edition.content else {
            anyhow::bail!("comic issue required")
        };
        Ok(pages)
    }
    /// A derived part definition preserves page/source/order and language identity.
    /// It is not a selected or committed asset, nor a workflow admission receipt.
    pub fn part_publication(&self, id: &str) -> Result<Publication> {
        self.check()?;
        let part = self
            .parts
            .iter()
            .find(|p| p.id == id)
            .context("issue part absent")?;
        let mut edition = self
            .publication
            .editions
            .iter()
            .find(|e| e.id == self.edition_id)
            .context("issue edition absent")?
            .clone();
        let Content::Comic { pages, .. } = &mut edition.content else {
            anyhow::bail!("comic issue required")
        };
        pages.retain(|p| part.pages.contains(&p.id));
        let publication = Publication {
            schema: self.publication.schema.clone(),
            id: self.publication.id.clone(),
            editions: vec![edition],
        };
        crate::check_publication(&publication)?;
        Ok(publication)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assembly {
    pub schema: String,
    pub plan: Pin,
    /// Part ID -> exact packet receipt. Paths are private, request-relative.
    pub parts: BTreeMap<String, Pin>,
    pub limits: Limits,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssemblyReceipt {
    pub schema: String,
    pub request: Object,
    pub plan: Object,
    pub parts: BTreeMap<String, Object>,
    pub page_order: Vec<String>,
    pub outputs: ArtifactSet,
    pub human_approval: bool,
    pub retained_owner_acknowledged: bool,
    pub print_ready: bool,
}
fn pinned(root: &Path, pin: &Pin, max: u64) -> Result<Vec<u8>> {
    ensure!(
        comic::portable(&pin.path) && (1..=max).contains(&pin.bytes),
        "issue pin/path bound refused"
    );
    let raw = comic::read(&root.join(&pin.path), pin.bytes)?;
    ensure!(
        comic::object(&raw)
            == Object {
                sha256: pin.sha256.clone(),
                bytes: pin.bytes
            },
        "issue pin differs"
    );
    Ok(raw)
}
/// Replays every part from its captured editable sources before reserving a new
/// output directory. Copies whole part packets, so restoration needs no worker folder.
/// The result is a private digital reading derivative, with no lifecycle effects.
pub fn assemble(request_path: &Path, destination: &Path) -> Result<AssemblyReceipt> {
    let request_path = std::path::absolute(request_path)?;
    let destination = std::path::absolute(destination)?;
    ensure!(!destination.exists(), "existing issue destination refused");
    let resolved_destination = destination
        .parent()
        .context("issue destination parent absent")?
        .canonicalize()?
        .join(
            destination
                .file_name()
                .context("issue destination name absent")?,
        );
    let raw = comic::read(&request_path, crate::MAX_CONTROL_BYTES as u64)?;
    let request: Assembly = serde_json::from_slice(&raw)?;
    ensure!(
        request.schema == "reel.comic-issue-assembly.v1"
            && (1..=128).contains(&request.parts.len())
            && (1..=128 * 1024 * 1024).contains(&request.limits.input_bytes)
            && (1..=128 * 1024 * 1024).contains(&request.limits.output_bytes),
        "issue assembly limits refused"
    );
    let root = request_path.parent().context("assembly parent absent")?;
    let plan_raw = pinned(root, &request.plan, crate::MAX_CONTROL_BYTES as u64)?;
    let plan: Plan = serde_json::from_slice(&plan_raw)?;
    plan.check()?;
    ensure!(
        request.parts.len() == plan.parts.len()
            && plan.parts.iter().all(|p| request.parts.contains_key(&p.id)),
        "issue part receipt set differs"
    );
    let edition = plan
        .publication
        .editions
        .iter()
        .find(|e| e.id == plan.edition_id)
        .context("issue edition absent")?;
    let pages = plan.pages()?;
    let mut files = BTreeMap::new();
    let mut part_receipts = BTreeMap::new();
    let mut pdfs = Vec::new();
    let mut transcript_pages = Vec::new();
    let mut used = (raw.len() + plan_raw.len()) as u64;
    let mut font_binding = None;
    for part in &plan.parts {
        let pin = &request.parts[&part.id];
        ensure!(
            Path::new(&pin.path)
                .file_name()
                .is_some_and(|n| n == "receipt.json"),
            "part receipt filename refused"
        );
        let receipt_raw = pinned(root, pin, crate::MAX_CONTROL_BYTES as u64)?;
        let packet_root = root
            .join(&pin.path)
            .parent()
            .context("part packet parent absent")?
            .to_path_buf();
        ensure!(
            !resolved_destination.starts_with(packet_root.canonicalize()?),
            "issue destination overlaps source packet"
        );
        let packet = comic::reopen_packet(&packet_root)?;
        ensure!(
            comic::object(&packet.files["receipt.json"]) == comic::object(&receipt_raw),
            "part receipt changed"
        );
        let build: Request = serde_json::from_slice(&packet.files["sources/request.json"])?;
        ensure!(
            build.publication_id == plan.publication.id
                && build.edition_id == plan.edition_id
                && build.language == edition.language
                && packet.receipt.page_order == part.pages,
            "part edition/language/page order differs"
        );
        let font = serde_json::to_vec(&(&build.font_family, &build.font, &build.font_license))?;
        if let Some(prior) = &font_binding {
            ensure!(*prior == font, "part font/license binding differs");
        } else {
            font_binding = Some(font);
        }
        for (id, source_pin) in part.pages.iter().zip(&build.pages) {
            let expected = pages
                .iter()
                .find(|p| &p.id == id)
                .context("part page absent")?;
            ensure!(
                expected.source.sha256 == source_pin.sha256
                    && expected.source.bytes == source_pin.bytes
                    && expected.source.media_type == "application/json",
                "part page source differs"
            );
            let source: PageSource =
                serde_json::from_slice(&packet.files[&format!("sources/{}", source_pin.path)])?;
            ensure!(
                source.id == *id
                    && source
                        .panels
                        .iter()
                        .map(|p| &p.id)
                        .eq(expected.panel_order.iter())
                    && source
                        .utterances
                        .iter()
                        .map(|u| &u.id)
                        .eq(expected.utterance_order.iter()),
                "part panel/utterance order differs"
            );
        }
        let transcript: serde_json::Value =
            serde_json::from_slice(&packet.files["transcript.json"])?;
        transcript_pages.extend(
            transcript["pages"]
                .as_array()
                .context("part transcript absent")?
                .iter()
                .cloned(),
        );
        pdfs.push(packet.files["reading.pdf"].clone());
        part_receipts.insert(part.id.clone(), comic::object(&receipt_raw));
        for (name, bytes) in packet.files {
            used = used
                .checked_add(bytes.len() as u64)
                .context("issue input overflow")?;
            ensure!(
                used <= request.limits.input_bytes,
                "issue input byte bound exceeded"
            );
            files.insert(format!("parts/{}/{name}", part.id), bytes);
        }
    }
    let pdf = reel_print::vector::merge_reading_pdfs(
        &pdfs.iter().map(Vec::as_slice).collect::<Vec<_>>(),
        pages.len(),
        request.limits.output_bytes,
    )?;
    let transcript = serde_json::to_vec_pretty(
        &serde_json::json!({"schema":"reel.comic-transcript.v1", "language":edition.language, "pages":transcript_pages}),
    )?;
    let outputs = [
        ("reading.pdf", "application/pdf", pdf),
        ("transcript.json", "application/json", transcript),
    ]
    .into_iter()
    .map(|(name, media, bytes)| {
        let object = comic::object(&bytes);
        let artifact = Artifact {
            logical_id: format!("{}/{}/{name}", plan.publication.id, plan.edition_id),
            sha256: object.sha256,
            bytes: object.bytes,
            media_type: media.into(),
        };
        files.insert(name.into(), bytes);
        (name.into(), artifact)
    })
    .collect();
    crate::check_outputs(&edition.outputs, &outputs)?;
    let receipt = AssemblyReceipt {
        schema: "reel.comic-issue-receipt.v1".into(),
        request: comic::object(&raw),
        plan: comic::object(&plan_raw),
        parts: part_receipts,
        page_order: pages.iter().map(|p| p.id.clone()).collect(),
        outputs,
        human_approval: false,
        retained_owner_acknowledged: false,
        print_ready: false,
    };
    files.insert("issue-plan.json".into(), plan_raw);
    // Captured packets are independently reopenable at their new relative paths.
    // Preserve the original request separately; do not pretend relocation kept its hash.
    let relocated = Assembly {
        plan: Pin {
            path: "issue-plan.json".into(),
            ..request.plan.clone()
        },
        parts: request
            .parts
            .iter()
            .map(|(id, pin)| {
                (
                    id.clone(),
                    Pin {
                        path: format!("parts/{id}/receipt.json"),
                        ..pin.clone()
                    },
                )
            })
            .collect(),
        ..request.clone()
    };
    files.insert(
        "assembly.json".into(),
        serde_json::to_vec_pretty(&relocated)?,
    );
    files.insert("source-assembly.json".into(), raw);
    files.insert("receipt.json".into(), serde_json::to_vec_pretty(&receipt)?);
    let total = files.values().try_fold(0u64, |n, b| {
        n.checked_add(b.len() as u64)
            .context("issue output overflow")
    })?;
    ensure!(
        total <= request.limits.output_bytes,
        "issue output byte bound exceeded"
    );
    ensure!(
        files.keys().all(|name| comic::portable(name)),
        "issue output path bound exceeded"
    );
    comic::write_new(&destination, &files)?;
    Ok(receipt)
}
