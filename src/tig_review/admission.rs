//! Protected existing-byte entry. Historical source paths are provenance only.
use super::*;
use serde::Serialize;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use tig_store::existing::{ByteSource, Profile as ByteProfile, ReadCache};

const META_MAX: usize = 4 * 1024 * 1024;
const PROFILE_MAX: usize = 65_536;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub task: Object,
    pub method: Object,
    pub policy: Object,
    pub control_root: PathBuf,
    pub byte_source: ByteProfile,
    pub state: ScopeState,
    pub receipt: Option<Object>,
    pub evidence: Option<Object>,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ScopeState {
    Unseen,
    Counted { report: Object },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub schema: String,
    pub method: Object,
    pub task_schema: String,
    pub group_schema: String,
    pub observations: ObservationPolicy,
    pub role_dimensions: BTreeMap<String, [u32; 2]>,
    /// Retained request input name -> immutable task source field.
    pub media_inputs: BTreeMap<String, String>,
}

fn file(path: &Path, max: usize) -> Result<Vec<u8>> {
    ensure!(
        path.is_absolute(),
        "review profile must be an explicit absolute path"
    );
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(max as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= max, "review profile exceeds bound");
    Ok(bytes)
}
fn bytes(cache: &mut ReadCache, object: &Object, max: usize) -> Result<Vec<u8>> {
    ensure!(object.bytes <= max as u64, "review metadata exceeds bound");
    let mut held = cache.open_verified(object)?;
    let mut bytes = Vec::new();
    (&mut held).take(max as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(
        Object::from_bytes(&bytes) == *object,
        "review metadata differs"
    );
    Ok(bytes)
}
fn metadata(cache: &mut ReadCache, object: &Object) -> Result<Value> {
    parse_json(&bytes(cache, object, META_MAX)?)
}
fn key(object: &Object) -> (String, u64) {
    (object.sha256.as_str().to_owned(), object.bytes)
}

#[derive(Clone, Serialize)]
struct ImageSpec {
    object: Object,
    dimensions: Option<[u32; 2]>,
    rgb8: bool,
}
struct Session {
    profile: Profile,
    policy: Policy,
    cache: ReadCache,
    task: Value,
    images: BTreeMap<(String, u64), ImageSpec>,
}
impl Session {
    fn open(path: &Path) -> Result<Self> {
        let profile: Profile = serde_json::from_value(parse_json(&file(path, PROFILE_MAX)?)?)?;
        Self::open_with_cache(profile, None)
    }
    fn open_with_cache(profile: Profile, existing_cache: Option<ReadCache>) -> Result<Self> {
        ensure!(
            profile.schema == "reel.review-scope-profile.experimental-1",
            "unsupported review profile"
        );
        receipts::directory(&profile.control_root)?;
        let mut cache = match existing_cache {
            Some(cache) => cache,
            None => ReadCache::new(profile.byte_source.clone())?,
        };
        let task = metadata(&mut cache, &profile.task)?;
        bytes(&mut cache, &profile.method, 256 * 1024)?;
        let policy: Policy = serde_json::from_value(metadata(&mut cache, &profile.policy)?)?;
        ensure!(
            policy.schema == "reel.review-observation-policy.experimental-1"
                && policy.method == profile.method,
            "policy method differs"
        );
        policy.observations.validate()?;
        ensure!(
            pin(at(&task, "protocol_method")?)? == profile.method,
            "task method differs"
        );
        ensure!(
            text(&task, "schema")? == policy.task_schema
                && text(&task, "scope")? == policy.observations.scope,
            "task protocol differs"
        );
        ensure!(
            boolean(&task, "source_group_all_clocks_remain_immutable")?
                && boolean(&task, "all_scope_qualification_pending")?
                && !boolean(&task, "human_approval")?
                && !boolean(&task, "technical_AAC_r1_not_final_playback_copy")?,
            "task frozen/pending state differs"
        );
        let roles = unique_strings(array(&task, "roles")?)?;
        ensure!(
            !roles.is_empty()
                && roles.len() <= 16
                && roles == policy.role_dimensions.keys().cloned().collect(),
            "task/policy roles differ"
        );
        let group = metadata(&mut cache, &pin(at(&task, "original_frozen_group")?)?)?;
        ensure!(
            text(&group, "schema")? == policy.group_schema,
            "group schema differs"
        );
        ensure!(
            at(&task, "original_group_number")? == at(&group, "group_number")?,
            "group number differs"
        );
        for field in ["episode", "language", "actual_programme"] {
            ensure!(
                task.get(field) == group.get(field),
                "task/group context differs: {field}"
            );
        }
        ensure!(
            optional(&task, "actual_Review") == optional(&group, "actual_Review"),
            "task/group optional Review differs"
        );
        let clocks = array(&task, "clocks")?;
        let indices = array(&task, "original_clock_indices")?;
        ensure!(
            !clocks.is_empty()
                && clocks.len() <= policy.observations.max_clocks
                && clocks.len() == indices.len(),
            "invalid review clock bound"
        );
        ensure!(
            integer(&task, "clock_count")? == clocks.len() as u64
                && integer(&task, "image_occurrences")? == (clocks.len() * roles.len()) as u64,
            "task occurrence counts differ"
        );
        let original = array(&group, "clocks")?;
        if group.get("clock_count").is_some() {
            ensure!(
                integer(&group, "clock_count")? == original.len() as u64,
                "original group clock count differs"
            );
        }
        if group.get("future_fresh_pixel_worker_max_clocks").is_some() {
            ensure!(
                clocks.len() as u64 <= integer(&group, "future_fresh_pixel_worker_max_clocks")?,
                "source worker clock bound exceeded"
            );
        }
        let mut seen = BTreeSet::new();
        let mut previous = None;
        for (clock, index) in clocks.iter().zip(indices) {
            let index = index
                .as_u64()
                .ok_or_else(|| anyhow!("invalid original clock index"))?;
            ensure!(
                previous.is_none_or(|p| p < index)
                    && seen.insert(index)
                    && original.get(usize::try_from(index)?) == Some(clock),
                "task is not an exact disjoint frozen group selection"
            );
            previous = Some(index);
        }
        let features = metadata(&mut cache, &pin(at(&task, "raw_feature_source")?)?)?;
        let expected_ass = group
            .get("current_ASS")
            .or_else(|| features.get("current_ASS"))
            .unwrap_or(&Value::Null);
        ensure!(
            optional(&task, "current_ASS") == expected_ass,
            "task/group/feature ASS source differs"
        );
        if let Some(ass) = task.get("current_ASS").filter(|v| !v.is_null()) {
            if let Some(source) = features.get("current_ASS") {
                ensure!(pin(ass)? == pin(source)?, "feature ASS source differs");
            }
            bytes(&mut cache, &pin(ass)?, META_MAX)?;
        }
        let mut required_raw = BTreeSet::new();
        for rule in &policy.observations.targets {
            ensure!(
                !rule.source_field.is_empty(),
                "target source mapping required"
            );
            let source = match features.get(&rule.source_field) {
                Some(value) => value
                    .as_array()
                    .ok_or_else(|| anyhow!("invalid source target collection"))?
                    .clone(),
                None => Vec::new(),
            };
            let mut selected = Vec::new();
            for mut target in source {
                let span = array(&target, &rule.span_field)?;
                ensure!(span.len() == 2, "invalid source target span");
                let start = span[0]
                    .as_u64()
                    .ok_or_else(|| anyhow!("invalid target start"))?;
                let end = span[1]
                    .as_u64()
                    .ok_or_else(|| anyhow!("invalid target end"))?;
                ensure!(start < end, "invalid source target range");
                if !clocks.iter().any(|c| {
                    c["sample"]["frame"]
                        .as_u64()
                        .is_some_and(|f| (start..end).contains(&f))
                }) {
                    continue;
                }
                for (field, pointer) in &rule.source_exact_fields {
                    if target.get(field).is_none() {
                        target[field] = target
                            .pointer(pointer)
                            .ok_or_else(|| anyhow!("missing source exact-field fallback"))?
                            .clone();
                    }
                }
                for field in &rule.picture_fields {
                    required_raw.insert(key(&pin(at(&target, field)?)?));
                }
                selected.push(target);
            }
            ensure!(
                array(&task, &rule.task_field)? == &selected,
                "task targets differ from source feature notes"
            );
        }
        let mut images = BTreeMap::new();
        let mut actual_raw = BTreeSet::new();
        for raw in array(&task, "raw_references")? {
            let object = pin(raw)?;
            ensure!(actual_raw.insert(key(&object)), "duplicate raw reference");
            images.insert(
                key(&object),
                ImageSpec {
                    object,
                    dimensions: None,
                    rgb8: false,
                },
            );
        }
        ensure!(
            actual_raw == required_raw,
            "raw reference set differs from active targets"
        );
        for clock in clocks {
            ensure!(
                names(at(clock, "views")?)? == roles,
                "clock view roles differ"
            );
            let proof = at(clock, "native_job_proof")?;
            ensure!(
                boolean(proof, "workspace_absent")?
                    && boolean(proof, "positive_terminal_retired")?
                    && boolean(proof, "exact_request_gzip_outputs_tables_verified")?,
                "clock native proof is not retained"
            );
            let request_pin = pin(at(proof, "request")?)?;
            let result_pin = pin(at(proof, "result")?)?;
            let compressed_pin = pin(at(proof, "retirement_evidence")?)?;
            let request = bytes(&mut cache, &request_pin, META_MAX)?;
            let result = bytes(&mut cache, &result_pin, META_MAX)?;
            let compressed = bytes(&mut cache, &compressed_pin, META_MAX)?;
            let root = metadata(&mut cache, &pin(at(proof, "root_verified")?)?)?;
            ensure!(
                at(&root, "frames")? == at(proof, "frames")?
                    && at(&root, "sample_ids")? == at(proof, "sample_ids")?
                    && at(&root, "job")? == at(proof, "job")?,
                "clock/root native scope differs"
            );
            let native_request = parse_json(&request)?;
            ensure!(
                !policy.media_inputs.is_empty() && policy.media_inputs.len() <= 16,
                "source input policy required"
            );
            for (input, field) in &policy.media_inputs {
                ensure!(
                    pin(at(at(&native_request, "inputs")?, input)?)? == pin(at(&task, field)?)?,
                    "native request source differs"
                );
            }
            let mut required_views = Vec::new();
            for role in &roles {
                let view = at(at(clock, "views")?, role)?;
                let object = pin(view)?;
                ensure!(
                    pin(at(view, "request")?)? == request_pin
                        && at(view, "job")? == at(proof, "job")?,
                    "view/native proof differs"
                );
                let dimensions = policy.role_dimensions[role];
                ensure!(
                    at(view, "dimensions")? == &json!(dimensions),
                    "declared native view dimensions differ"
                );
                if let Some(existing) = images.get(&key(&object)) {
                    ensure!(
                        existing.dimensions.is_none_or(|d| d == dimensions),
                        "one image cannot have conflicting role dimensions"
                    );
                }
                images.insert(
                    key(&object),
                    ImageSpec {
                        object: object.clone(),
                        dimensions: Some(dimensions),
                        rgb8: true,
                    },
                );
                required_views.push(SnapshotView {
                    slot: text(view, "native_output_role")?.to_owned(),
                    object,
                    sample_id: text(at(clock, "sample")?, "sample_id")?.to_owned(),
                    frame: integer(at(clock, "sample")?, "frame")?,
                });
            }
            validate_retired_snapshot(RetainedSnapshot {
                request: &request_pin,
                request_bytes: &request,
                result: &result_pin,
                result_bytes: &result,
                compressed: &compressed_pin,
                compressed_bytes: &compressed,
                root: &root,
                required_views: &required_views,
            })?;
        }
        let mut session = Self {
            profile,
            policy,
            cache,
            task,
            images,
        };
        let specs = session.images.values().cloned().collect::<Vec<_>>();
        for spec in &specs {
            session.verify_image(&json!(spec.object))?;
        }
        Ok(session)
    }
    fn verify_image(&mut self, value: &Value) -> Result<()> {
        let object = pin(value)?;
        let spec = self
            .images
            .get(&key(&object))
            .ok_or_else(|| anyhow!("image outside admitted scope"))?;
        let mut held = self.cache.open_verified(&object)?;
        let mut header = [0_u8; 33];
        held.read_exact(&mut header)?;
        let dimensions = spec.dimensions.unwrap_or([
            u32::from_be_bytes(header[16..20].try_into()?),
            u32::from_be_bytes(header[20..24].try_into()?),
        ]);
        ensure!(
            dimensions.iter().all(|d| (1..=32768).contains(d)),
            "reference dimensions exceed bound"
        );
        validate_png_header(&header, dimensions, spec.rgb8)
    }
    fn validate_report(&mut self, report: &Value) -> Result<()> {
        let task = self.task.clone();
        let task_object = self.profile.task.clone();
        let policy = self.policy.observations.clone();
        validate_subreport(&task, &task_object, report, &policy, &mut |image| {
            self.verify_image(image)
        })
    }
    fn counted(&mut self) -> Result<Option<Value>> {
        let ScopeState::Counted { report } = &self.profile.state else {
            return Ok(None);
        };
        let report_pin = report.clone();
        let report = metadata(&mut self.cache, &report_pin)?;
        self.validate_report(&report)?;
        Ok(Some(
            json!({"state":"completed-validated-skip","task":self.profile.task,"report":report_pin,"new_image_views":0,"new_receipts":0,"human_approval":false}),
        ))
    }
    fn binding(&self) -> Result<Object> {
        let receipt = self
            .profile
            .receipt
            .clone()
            .ok_or_else(|| anyhow!("exact receipt pin required for recovery/submission"))?;
        receipts::verify_binding(
            &self.profile.control_root,
            &self.profile.task,
            &self.profile.method,
            &self.profile.policy,
            &receipt,
        )?;
        Ok(receipt)
    }
}

pub fn inspect(profile: &Path) -> Result<Value> {
    let mut session = Session::open(profile)?;
    if let Some(done) = session.counted()? {
        return Ok(done);
    }
    Ok(
        json!({"state":"metadata-verified-unreserved","task":session.profile.task,"method":session.profile.method,"policy":session.profile.policy,"clock_count":session.task["clock_count"],"image_occurrences":session.task["image_occurrences"],"verified_unique_images":session.images.len(),"new_image_views":0,"human_approval":false}),
    )
}
pub fn reserve(profile: &Path, owner: &str) -> Result<Value> {
    let mut session = Session::open(profile)?;
    if let Some(done) = session.counted()? {
        return Ok(done);
    }
    ensure!(
        session.profile.receipt.is_none() && session.profile.evidence.is_none(),
        "reservation cannot replace existing receipt/evidence"
    );
    let reserved = receipts::reserve(
        &session.profile.control_root,
        &session.profile.task,
        &session.profile.method,
        &session.profile.policy,
        owner,
    )?;
    Ok(
        json!({"state":"reserved-before-views","task":session.profile.task,"receipt":reserved.receipt,"directory":reserved.directory,"clock_count":session.task["clock_count"],"image_occurrences":session.task["image_occurrences"],"new_image_views":0,"human_approval":false}),
    )
}
pub fn resume(profile: &Path) -> Result<Value> {
    let mut session = Session::open(profile)?;
    if let Some(done) = session.counted()? {
        return Ok(done);
    }
    let receipt = session.binding()?;
    let root = session.profile.control_root.clone();
    let task = session.profile.task.clone();
    let recovered = receipts::recover(&root, &task, &receipt, &mut |report| {
        session.validate_report(report)
    })?;
    Ok(match recovered {
        receipts::Recovery::Pending => {
            json!({"state":"reserved-pending-explicit-recovery","task":task,"receipt":receipt,"new_image_views":0,"automatic_view_permission":false,"human_approval":false})
        }
        receipts::Recovery::Completed { report } => {
            json!({"state":"completed-validated-skip","task":task,"receipt":receipt,"report":report,"new_image_views":0,"human_approval":false})
        }
    })
}
pub fn submit(profile: &Path) -> Result<Value> {
    let mut session = Session::open(profile)?;
    if let Some(done) = session.counted()? {
        return Ok(done);
    }
    let receipt = session.binding()?;
    let root = session.profile.control_root.clone();
    let task_pin = session.profile.task.clone();
    if let receipts::Recovery::Completed { report } =
        receipts::recover(&root, &task_pin, &receipt, &mut |value| {
            session.validate_report(value)
        })?
    {
        return Ok(
            json!({"state":"completed-validated-skip","task":task_pin,"receipt":receipt,"report":report,"new_image_views":0,"human_approval":false}),
        );
    }
    let evidence_pin = session
        .profile
        .evidence
        .clone()
        .ok_or_else(|| anyhow!("exact retained evidence pin required"))?;
    let evidence = metadata(&mut session.cache, &evidence_pin)?;
    let task = session.task.clone();
    let task_pin = session.profile.task.clone();
    let policy = session.policy.observations.clone();
    let report = build_subreport(&task, &task_pin, &evidence, &policy, &mut |image| {
        session.verify_image(image)
    })?;
    let root = session.profile.control_root.clone();
    let report_pin = receipts::complete(&root, &task_pin, &receipt, &report, &mut |value| {
        session.validate_report(value)
    })?;
    Ok(
        json!({"state":"observations-validated-report-saved","task":task_pin,"receipt":receipt,"report":report_pin,"report_path":root.join(task_pin.sha256.as_str()).join("report.json"),"finite_scope_qualified_with_holds":report["finite_scope_qualified_with_holds"],"all_scope_qualification_pending":true,"human_approval":false,"publication_pending":true}),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AggregationProfile {
    schema: String,
    method: Object,
    policy: Object,
    group: Object,
    control_root: PathBuf,
    byte_source: ByteProfile,
    parts: Vec<CountedPart>,
    require_complete: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CountedPart {
    task: Object,
    report: Object,
}

/// Reopen exact retained parts with one shared read budget, validate all native
/// proofs and observations, then save an explicit disjoint aggregate. No new views.
pub fn aggregate(profile: &Path, output: &Path) -> Result<Value> {
    use std::io::Write;
    let config: AggregationProfile =
        serde_json::from_value(parse_json(&file(profile, PROFILE_MAX)?)?)?;
    ensure!(
        config.schema == "reel.review-aggregation-profile.experimental-1"
            && !config.parts.is_empty()
            && config.parts.len() <= 64,
        "invalid aggregation profile"
    );
    ensure!(
        output.is_absolute()
            && output.parent().is_some_and(
                |p| fs::canonicalize(p).ok() == fs::canonicalize(&config.control_root).ok()
            ),
        "aggregate must be an explicit file in the existing control root"
    );
    let mut last: Option<Session> = None;
    let mut retained = Vec::new();
    for part in &config.parts {
        let (cache, previous_images) = match last.take() {
            Some(session) => (Some(session.cache), session.images),
            None => (None, BTreeMap::new()),
        };
        let part_profile = Profile {
            schema: "reel.review-scope-profile.experimental-1".into(),
            task: part.task.clone(),
            method: config.method.clone(),
            policy: config.policy.clone(),
            control_root: config.control_root.clone(),
            byte_source: config.byte_source.clone(),
            state: ScopeState::Counted {
                report: part.report.clone(),
            },
            receipt: None,
            evidence: None,
        };
        let mut session = Session::open_with_cache(part_profile, cache)?;
        ensure!(
            pin(at(&session.task, "original_frozen_group")?)? == config.group,
            "aggregate part group differs"
        );
        let report = metadata(&mut session.cache, &part.report)?;
        session.validate_report(&report)?;
        retained.push((part.task.clone(), session.task.clone(), report));
        for (key, previous) in previous_images {
            if let Some(current) = session.images.get(&key) {
                ensure!(
                    previous.dimensions.is_none()
                        || current.dimensions.is_none()
                        || previous.dimensions == current.dimensions,
                    "aggregate image geometry conflicts"
                );
                if previous.rgb8 && !current.rgb8 {
                    session.images.insert(key, previous);
                }
            } else {
                session.images.insert(key, previous);
            }
        }
        last = Some(session);
    }
    let mut session = last.ok_or_else(|| anyhow!("missing aggregate session"))?;
    let group = metadata(&mut session.cache, &config.group)?;
    let policy = session.policy.observations.clone();
    let parts = retained
        .iter()
        .map(|(task_object, task, report)| ReviewPart {
            task_object,
            task,
            report,
        })
        .collect::<Vec<_>>();
    let mut report = aggregate_parts(
        &group,
        &config.group,
        &parts,
        &policy,
        config.require_complete,
        &mut |image| session.verify_image(image),
    )?;
    for field in [
        "episode",
        "language",
        "group_number",
        "actual_programme",
        "actual_Review",
        "current_ASS",
    ] {
        if let Some(value) = group.get(field) {
            report[field] = value.clone();
        }
    }
    report["frames"] = json!(
        array(&report, "observations")?
            .iter()
            .map(|o| o["frame"].clone())
            .collect::<Vec<_>>()
    );
    report["source_subreports"] = json!(config.parts.iter().map(|p| &p.report).collect::<Vec<_>>());
    let encoded = serde_json::to_vec_pretty(&report)?;
    ensure!(encoded.len() <= META_MAX, "aggregate report exceeds bound");
    let object = Object::from_bytes(&encoded);
    if output.try_exists()? {
        let meta = fs::symlink_metadata(output)?;
        ensure!(
            meta.is_file() && !meta.file_type().is_symlink() && file(output, META_MAX)? == encoded,
            "existing aggregate differs; rewrite refused"
        );
    } else {
        let mut temp = tempfile::NamedTempFile::new_in(&config.control_root)?;
        temp.write_all(&encoded)?;
        temp.as_file_mut().sync_all()?;
        temp.persist_noclobber(output)
            .map_err(|_| anyhow!("exclusive aggregate save refused"))?;
        ensure!(
            Object::from_bytes(&file(output, META_MAX)?) == object,
            "aggregate readback differs"
        );
    }
    Ok(
        json!({"state":"disjoint-aggregate-validated","report":object,"report_path":output,"group_coverage_complete":report["group_coverage_complete"],"observed_clock_count":report["observed_clock_count"],"all_scope_qualification_pending":true,"new_image_views":0,"human_approval":false,"publication_pending":true}),
    )
}
