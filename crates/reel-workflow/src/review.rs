//! Versioned review findings and proposed repair impact. These are private
//! metadata contracts, not proof of viewing, authority, custody or issue delivery.
use crate::*;
pub const REQUEST_ID: &str = "control/review-request";
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub production_commit: String,
    pub copy_input: String,
    pub report_output: String,
}
pub struct Bound {
    packet: Packet,
    request: Request,
    copy_bytes: Vec<u8>,
}
impl Bound {
    pub fn report_output(&self) -> &str {
        &self.request.report_output
    }
    pub fn check(&self, result: &ReturnedResult, report: &[u8]) -> Result<CheckedReport> {
        check_returned_report_at(
            &self.packet,
            result,
            &self.request.copy_input,
            &self.copy_bytes,
            &self.request.report_output,
            report,
            &self.request.production_commit,
        )
    }
}
/// Frozen structured-review input checks, before a provider or manual export.
/// The reader owns native admission and must hold its byte guards separately.
pub fn bind(packet: &Packet, reader: &mut impl publication::InputReader) -> Result<Bound> {
    check_packet(packet)?;
    if packet.task.operation != Operation::ReviewReport {
        return fail(ErrorKind::Invalid, "review-task-operation");
    }
    let read = |reader: &mut dyn publication::InputReader, pin: &Artifact| -> Result<Vec<u8>> {
        if pin.media_type != "application/json" || pin.bytes > MAX_CONTROL_BYTES as u64 {
            return fail(ErrorKind::Budget, "review-task-control-pin");
        }
        let raw = reader.read(pin, MAX_CONTROL_BYTES as u64)?;
        if pin.bytes != raw.len() as u64 || pin.sha256 != digest(&raw) {
            return fail(ErrorKind::Integrity, "review-task-control-content");
        }
        Ok(raw)
    };
    let request_pin = packet
        .task
        .inputs
        .iter()
        .find(|a| a.logical_id == REQUEST_ID)
        .ok_or(Error {
            kind: ErrorKind::Missing,
            code: "review-task-request".into(),
        })?;
    let request: Request = decode(&read(reader, request_pin)?)?;
    if request.schema != "reel.workflow-review-request.v1"
        || !hex(&request.production_commit, &[40, 64])
        || request.copy_input == REQUEST_ID
        || !token(&request.report_output)
    {
        return fail(ErrorKind::Invalid, "review-request-contract");
    }
    let output = packet
        .task
        .outputs
        .get(&request.report_output)
        .ok_or(Error {
            kind: ErrorKind::Missing,
            code: "review-report-output".into(),
        })?;
    if output.media_type != "application/json"
        || !output.required
        || output.max_bytes > MAX_CONTROL_BYTES as u64
    {
        return fail(ErrorKind::Invalid, "review-report-output-contract");
    }
    let copy_pin = packet
        .task
        .inputs
        .iter()
        .find(|a| a.logical_id == request.copy_input)
        .ok_or(Error {
            kind: ErrorKind::Missing,
            code: "review-task-copy-input".into(),
        })?;
    let copy_bytes = read(reader, copy_pin)?;
    let copy: Copy = decode(&copy_bytes)?;
    check_copy(&copy)?;
    if copy.project != packet.project
        || copy.source_commit != request.production_commit
        || packet.task.language.as_ref() != Some(&copy.language)
    {
        return fail(ErrorKind::Stale, "review-task-edition");
    }
    if !packet.task.inputs.contains(&copy.publication) || !packet.task.inputs.contains(&copy.review)
    {
        return fail(ErrorKind::Missing, "review-task-source-pins");
    }
    Ok(Bound {
        packet: packet.clone(),
        request,
        copy_bytes,
    })
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Format {
    Film { frames: u64 },
    Comic { page_order: Vec<String> },
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Unit {
    Shot {
        shot_id: String,
        start_frame: u64,
        end_frame: u64,
    },
    Film {
        cel_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        shot_id: Option<String>,
        cue_id: Option<String>,
        start_frame: u64,
        end_frame: u64,
    },
    Comic {
        page_id: String,
        panel_id: Option<String>,
        balloon_id: Option<String>,
    },
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Copy {
    pub schema: String,
    pub project: String,
    pub source_commit: String,
    pub language: String,
    pub publication: Artifact,
    pub review: Artifact,
    pub format: Format,
    /// Existing producer IDs and exact ranges; this module allocates no IDs.
    pub units: BTreeMap<String, Unit>,
}
/// Derived private controls. A comic manifest references every ordered reading
/// image; writing these bytes does not retain them or authorize publication.
#[derive(Debug)]
pub struct Export {
    pub copy: Copy,
    pub manifest: Option<Vec<u8>>,
}
pub fn unit_key(unit: &Unit) -> Result<String> {
    canonical_digest(&("reel.workflow-review-unit.v1", unit))
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Modality {
    Visual,
    Audio,
    Audiovisual,
    Text,
    Layout,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Route {
    Project,
    Reel,
    Tig,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FrameRange {
    pub start: u64,
    pub end: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub code: String,
    pub unit: String,
    pub frames: Option<FrameRange>,
    pub modality: Modality,
    pub route: Route,
    pub requested_change: String,
    pub evidence: Vec<Artifact>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub schema: String,
    pub copy_sha256: String,
    pub findings: Vec<Finding>,
}
#[derive(Debug, Serialize)]
pub struct CheckedFinding {
    key: String,
    finding: Finding,
}
impl CheckedFinding {
    pub fn key(&self) -> &str {
        &self.key
    }
    pub fn finding(&self) -> &Finding {
        &self.finding
    }
}
#[derive(Debug, Serialize)]
pub struct CheckedReport {
    copy: Copy,
    report_sha256: String,
    findings: BTreeMap<String, CheckedFinding>,
}
impl CheckedReport {
    pub fn findings(&self) -> &BTreeMap<String, CheckedFinding> {
        &self.findings
    }
    pub fn report_sha256(&self) -> &str {
        &self.report_sha256
    }
}
fn id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}
pub fn check_copy(copy: &Copy) -> Result<()> {
    canonical_digest(copy)?;
    if copy.schema != "reel.workflow-review-copy.v1"
        || !token(&copy.project)
        || !hex(&copy.source_commit, &[40, 64])
        || !token(&copy.language)
        || copy.units.is_empty()
        || copy.units.len() > 4096
    {
        return fail(ErrorKind::Invalid, "review-copy-contract");
    }
    artifact(&copy.publication)?;
    artifact(&copy.review)?;
    let pages = match &copy.format {
        Format::Film { frames } if *frames > 0 => BTreeSet::new(),
        Format::Comic { page_order } if !page_order.is_empty() && page_order.len() <= 4096 => {
            let unique: BTreeSet<_> = page_order.iter().collect();
            if unique.len() != page_order.len() || page_order.iter().any(|s| !id(s)) {
                return fail(ErrorKind::Conflict, "review-page-order");
            }
            unique
        }
        _ => return fail(ErrorKind::Invalid, "review-format"),
    };
    for (key, unit) in &copy.units {
        if !id(key) {
            return fail(ErrorKind::Invalid, "review-unit-id");
        }
        let valid = match (&copy.format, unit) {
            (
                Format::Film { frames },
                Unit::Shot {
                    shot_id,
                    start_frame,
                    end_frame,
                },
            ) => id(shot_id) && start_frame < end_frame && end_frame <= frames,
            (
                Format::Film { frames },
                Unit::Film {
                    cel_id,
                    shot_id,
                    cue_id,
                    start_frame,
                    end_frame,
                },
            ) => {
                id(cel_id)
                    && shot_id.as_ref().is_none_or(|v| id(v))
                    && cue_id.as_ref().is_none_or(|v| id(v))
                    && start_frame < end_frame
                    && end_frame <= frames
            }
            (
                Format::Comic { .. },
                Unit::Comic {
                    page_id,
                    panel_id,
                    balloon_id,
                },
            ) => {
                pages.contains(page_id)
                    && panel_id.as_ref().is_none_or(|v| id(v))
                    && balloon_id.as_ref().is_none_or(|v| id(v))
                    && (balloon_id.is_none() || panel_id.is_some())
            }
            _ => false,
        };
        if !valid {
            return fail(ErrorKind::Invalid, "review-unit-location");
        }
    }
    Ok(())
}
pub fn check_report(copy: &Copy, report: &Report) -> Result<CheckedReport> {
    check_copy(copy)?;
    let report_sha256 = canonical_digest(report)?;
    if report.schema != "reel.workflow-review-report.v1" {
        return fail(ErrorKind::Invalid, "review-report-schema");
    }
    if report.copy_sha256 != canonical_digest(copy)? {
        return fail(ErrorKind::Stale, "review-copy-pin");
    }
    if report.findings.len() > 128 {
        return fail(ErrorKind::Budget, "review-finding-count");
    }
    let mut findings = BTreeMap::<String, CheckedFinding>::new();
    for finding in &report.findings {
        if !token(&finding.code)
            || finding.requested_change.trim().is_empty()
            || finding.requested_change.len() > 4096
            || finding.evidence.is_empty()
            || finding.evidence.len() > 16
        {
            return fail(ErrorKind::Invalid, "review-finding-contract");
        }
        let unit = copy.units.get(&finding.unit).ok_or(Error {
            kind: ErrorKind::Missing,
            code: "review-finding-unit".into(),
        })?;
        match unit {
            Unit::Shot {
                start_frame,
                end_frame,
                ..
            }
            | Unit::Film {
                start_frame,
                end_frame,
                ..
            } => {
                if finding.modality == Modality::Layout {
                    return fail(ErrorKind::Invalid, "review-film-modality");
                }
                if finding.frames.as_ref().is_some_and(|r| {
                    r.start >= r.end || r.start < *start_frame || r.end > *end_frame
                }) {
                    return fail(ErrorKind::Invalid, "review-finding-frames");
                }
            }
            Unit::Comic { .. } => {
                if finding.frames.is_some()
                    || matches!(finding.modality, Modality::Audio | Modality::Audiovisual)
                {
                    return fail(ErrorKind::Invalid, "review-comic-modality");
                }
            }
        }
        let mut evidence = BTreeSet::new();
        for pin in &finding.evidence {
            artifact(pin)?;
            if !evidence.insert(&pin.logical_id) {
                return fail(ErrorKind::Conflict, "review-evidence-identity");
            }
        }
        // Stable across editorial wording/evidence updates, scoped to the exact
        // reviewed version/language/location and defect code. No remote effect.
        let key = canonical_digest(&(
            "reel.workflow-finding-key.v1",
            &report.copy_sha256,
            &finding.unit,
            &finding.frames,
            &finding.code,
        ))?;
        if let Some(old) = findings.get(&key) {
            if old.finding != *finding {
                return fail(ErrorKind::Conflict, "review-duplicate-finding");
            }
        } else {
            findings.insert(
                key.clone(),
                CheckedFinding {
                    key,
                    finding: finding.clone(),
                },
            );
        }
    }
    Ok(CheckedReport {
        copy: copy.clone(),
        report_sha256,
        findings,
    })
}

/// Bind a provider/handoff review to the existing task/result protocol. Named
/// bytes are checked here; the native caller still owns admission and retention.
pub fn check_returned_report(
    packet: &Packet,
    result: &ReturnedResult,
    copy_input: &str,
    copy_bytes: &[u8],
    report_output: &str,
    report_bytes: &[u8],
) -> Result<CheckedReport> {
    if packet.task.operation == Operation::ReviewReport {
        return fail(ErrorKind::Denied, "review-request-bound-check-required");
    }
    check_returned_report_at(
        packet,
        result,
        copy_input,
        copy_bytes,
        report_output,
        report_bytes,
        &packet.source_commit,
    )
}
fn check_returned_report_at(
    packet: &Packet,
    result: &ReturnedResult,
    copy_input: &str,
    copy_bytes: &[u8],
    report_output: &str,
    report_bytes: &[u8],
    production_commit: &str,
) -> Result<CheckedReport> {
    check_result(packet, result)?;
    if copy_bytes.len() > MAX_CONTROL_BYTES || report_bytes.len() > MAX_CONTROL_BYTES {
        return fail(ErrorKind::Budget, "review-task-control-bytes");
    }
    if !matches!(
        packet.task.operation,
        Operation::ReviewFinding | Operation::ReviewReport
    ) {
        return fail(ErrorKind::Invalid, "review-task-operation");
    }
    let copy_pin = packet
        .task
        .inputs
        .iter()
        .find(|a| a.logical_id == copy_input)
        .ok_or(Error {
            kind: ErrorKind::Missing,
            code: "review-task-copy-input".into(),
        })?;
    if copy_pin.bytes != copy_bytes.len() as u64 || copy_pin.sha256 != digest(copy_bytes) {
        return fail(ErrorKind::Integrity, "review-task-copy-bytes");
    }
    check_output(packet, result, report_output, report_bytes)?;
    if copy_pin.media_type != "application/json"
        || result.artifacts[report_output].media_type != "application/json"
    {
        return fail(ErrorKind::Invalid, "review-task-json");
    }
    let copy: Copy = decode(copy_bytes)?;
    if copy.project != packet.project
        || copy.source_commit != production_commit
        || packet.task.language.as_ref() != Some(&copy.language)
    {
        return fail(ErrorKind::Stale, "review-task-edition");
    }
    if !packet.task.inputs.contains(&copy.publication) || !packet.task.inputs.contains(&copy.review)
    {
        return fail(ErrorKind::Missing, "review-task-source-pins");
    }
    let report: Report = decode(report_bytes)?;
    let checked = check_report(&copy, &report)?;
    for finding in checked.findings.values() {
        for evidence in &finding.finding.evidence {
            if !packet.task.inputs.contains(evidence)
                && !result.artifacts.values().any(|a| a == evidence)
            {
                return fail(ErrorKind::Missing, "review-task-evidence-pin");
            }
        }
    }
    Ok(checked)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Routes {
    pub project: String,
    pub reel: String,
    pub tig: String,
}
#[derive(Debug, Serialize)]
pub struct IssueProposal {
    pub schema: String,
    pub operation_key: String,
    pub finding_key: String,
    pub repository: String,
    pub copy: Copy,
    pub finding: Finding,
}
/// Prepare private local issue bodies. A tracker adapter must reconcile this
/// operation key and preserve remote URLs before retrying any uncertain effect.
pub fn issue_proposals(report: &CheckedReport, routes: &Routes) -> Result<Vec<IssueProposal>> {
    for repository in [&routes.project, &routes.reel, &routes.tig] {
        let parts: Vec<_> = repository.split('/').collect();
        if parts.len() != 2 || parts.iter().any(|s| !token(s)) {
            return fail(ErrorKind::Invalid, "review-issue-repository");
        }
    }
    let proposals: Vec<_> = report
        .findings
        .iter()
        .map(|(key, checked)| {
            let repository = match checked.finding.route {
                Route::Project => &routes.project,
                Route::Reel => &routes.reel,
                Route::Tig => &routes.tig,
            }
            .to_ascii_lowercase();
            Ok(IssueProposal {
                schema: "reel.workflow-issue-proposal.v1".into(),
                operation_key: canonical_digest(&(
                    "reel.workflow-issue-operation.v1",
                    &repository,
                    key,
                ))?,
                finding_key: key.clone(),
                repository,
                copy: report.copy.clone(),
                finding: checked.finding.clone(),
            })
        })
        .collect::<Result<_>>()?;
    canonical_digest(&proposals)?;
    Ok(proposals)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    pub before: Artifact,
    pub after: Artifact,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Repair {
    pub schema: String,
    pub report_sha256: String,
    pub finding_keys: Vec<String>,
    pub edits: Vec<Edit>,
}
#[derive(Debug, Serialize)]
pub struct Impact {
    pub schema: String,
    pub plan_sha256: String,
    pub repair_sha256: String,
    pub directly_affected: Vec<String>,
    pub rebuild: Vec<String>,
    pub unaffected: Vec<String>,
    /// Languages declared on affected tasks, including shared-input consumers.
    /// Producer/project review may require additional contextual editions.
    pub contextual_languages: Vec<String>,
    /// Review the entire corrected edition, including neighboring units. This
    /// is a requirement, not evidence that such review has happened.
    pub contextual_review: Copy,
}
/// Plans required rebuilds from exact input pins and the declared dependency DAG.
/// Does not mutate a plan, release old jobs, apply selections or close findings.
pub fn repair_impact(plan: &Plan, report: &CheckedReport, repair: &Repair) -> Result<Impact> {
    let order = check_plan(plan)?;
    if plan.project != report.copy.project || plan.source_commit != report.copy.source_commit {
        return fail(ErrorKind::Stale, "review-repair-baseline");
    }
    let repair_sha256 = canonical_digest(repair)?;
    if repair.schema != "reel.workflow-repair.v1" || repair.report_sha256 != report.report_sha256 {
        return fail(ErrorKind::Stale, "review-repair-report");
    }
    if repair.finding_keys.is_empty()
        || repair.finding_keys.len() > 128
        || repair.edits.is_empty()
        || repair.edits.len() > 128
    {
        return fail(ErrorKind::Budget, "review-repair-count");
    }
    let mut keys = BTreeSet::new();
    for key in &repair.finding_keys {
        if !report.findings.contains_key(key) {
            return fail(ErrorKind::Missing, "review-repair-finding");
        }
        if !keys.insert(key) {
            return fail(ErrorKind::Conflict, "review-repair-finding-duplicate");
        }
    }
    let mut changed = BTreeMap::new();
    for edit in &repair.edits {
        artifact(&edit.before)?;
        artifact(&edit.after)?;
        if edit.before.logical_id != edit.after.logical_id || edit.before == edit.after {
            return fail(ErrorKind::Invalid, "review-repair-edit");
        }
        if changed
            .insert(&edit.before.logical_id, &edit.before)
            .is_some()
        {
            return fail(ErrorKind::Conflict, "review-repair-edit-duplicate");
        }
        if !plan.tasks.iter().any(|t| t.inputs.contains(&edit.before)) {
            return fail(ErrorKind::Stale, "review-repair-input-pin");
        }
    }
    let direct: BTreeSet<_> = plan
        .tasks
        .iter()
        .filter(|t| {
            t.inputs
                .iter()
                .any(|a| changed.get(&a.logical_id).is_some_and(|old| **old == *a))
        })
        .map(|t| t.id.clone())
        .collect();
    let mut affected = direct.clone();
    let mut rebuild = Vec::new();
    let mut unaffected = Vec::new();
    for id in order {
        let task = plan
            .tasks
            .iter()
            .find(|t| t.id == id)
            .expect("checked plan order");
        if affected.contains(&id) || task.depends_on.iter().any(|d| affected.contains(d)) {
            affected.insert(id.clone());
            rebuild.push(id);
        } else {
            unaffected.push(id);
        }
    }
    let contextual_languages = plan
        .tasks
        .iter()
        .filter(|t| affected.contains(&t.id))
        .filter_map(|t| t.language.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let impact = Impact {
        schema: "reel.workflow-repair-impact.v1".into(),
        plan_sha256: canonical_digest(plan)?,
        repair_sha256,
        directly_affected: direct.into_iter().collect(),
        rebuild,
        unaffected,
        contextual_languages,
        contextual_review: report.copy.clone(),
    };
    canonical_digest(&impact)?;
    Ok(impact)
}
