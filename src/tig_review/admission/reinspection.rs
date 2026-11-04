//! Explicit additional observations of an unchanged, previously held scope.
//! The original report and its planned-clock accounting are never rewritten.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReinspectionProfile {
    schema: String,
    scope: Profile,
    reason: String,
    /// Producer-admitted context; opening bytes does not certify their meaning.
    prerequisites: BTreeMap<String, Prerequisite>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
enum Origin {
    Task,
    FeatureNotes,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Prerequisite {
    origin: Origin,
    pointer: String,
    object: Object,
}

struct Reinspection {
    session: Session,
    declaration: Value,
    prior: Object,
    namespace: PathBuf,
}

impl Reinspection {
    fn open(path: &Path) -> Result<Self> {
        let profile: ReinspectionProfile =
            serde_json::from_value(parse_json(&file(path, PROFILE_MAX)?)?)?;
        ensure!(
            profile.schema == "reel.review-reinspection-profile.experimental-1",
            "unsupported reinspection profile"
        );
        let prior = match &profile.scope.state {
            ScopeState::Counted { report } => report.clone(),
            ScopeState::Unseen => bail!("reinspection requires an exact counted prior report"),
        };
        ensure!(
            !profile.reason.trim().is_empty() && profile.reason.len() <= 4096,
            "explicit bounded reinspection reason required"
        );
        ensure!(
            !profile.prerequisites.is_empty() && profile.prerequisites.len() <= 32,
            "explicit bounded reinspection prerequisites required"
        );
        let mut session = Session::open_with_cache(profile.scope, None)?;
        let original = metadata(&mut session.cache, &prior)?;
        session.validate_report(&original)?;
        ensure!(
            !boolean(&original, "finite_scope_qualified_with_holds")?,
            "qualified prior scopes cannot be scheduled for reinspection"
        );
        ensure!(
            original.get("reinspection").is_none(),
            "nested reinspection refused"
        );
        let features = metadata(
            &mut session.cache,
            &pin(at(&session.task, "raw_feature_source")?)?,
        )?;
        let mut declared_inputs = BTreeSet::new();
        for (name, input) in &profile.prerequisites {
            ensure!(
                !name.trim().is_empty() && name.len() <= 128,
                "invalid prerequisite name"
            );
            ensure!(
                input.pointer.starts_with('/') && input.pointer.len() <= 2048,
                "invalid prerequisite source pointer"
            );
            ensure!(
                declared_inputs.insert((input.origin, input.pointer.clone())),
                "duplicate prerequisite source declaration"
            );
            let source = match input.origin {
                Origin::Task => &session.task,
                Origin::FeatureNotes => &features,
            };
            let declared = source
                .pointer(&input.pointer)
                .ok_or_else(|| anyhow!("prerequisite source declaration absent"))?;
            ensure!(
                pin(declared)? == input.object,
                "prerequisite source pin differs"
            );
            bytes(&mut session.cache, &input.object, META_MAX)?;
        }
        let namespace = session
            .profile
            .control_root
            .join("reinspections")
            .join(prior.sha256.as_str());
        let declaration = json!({
            "schema":"reel.review-reinspection-context.experimental-1",
            "original_task":session.profile.task,"prior_report":prior,
            "method":session.profile.method,"policy":session.profile.policy,
            "reason":profile.reason,"prerequisites":profile.prerequisites,
            "prior_finite_scope_qualified_with_holds":false,
            "prior_holds_preserved":at(&original,"holds_preserved")?,
            "new_planned_clocks":0,"human_approval":false
        });
        Ok(Self {
            session,
            declaration,
            prior,
            namespace,
        })
    }

    fn context_bytes(&self) -> Result<Vec<u8>> {
        let encoded = serde_json::to_vec_pretty(&self.declaration)?;
        ensure!(
            encoded.len() <= PROFILE_MAX,
            "reinspection context exceeds bound"
        );
        Ok(encoded)
    }

    /// Only explicit reservation creates coordination directories. The prior
    /// report determines the namespace, so changing policy/context cannot retry.
    fn activate(&mut self, create: bool) -> Result<Object> {
        let parent = self
            .namespace
            .parent()
            .ok_or_else(|| anyhow!("namespace parent absent"))?;
        let encoded = self.context_bytes()?;
        if create {
            for dir in [parent, self.namespace.as_path()] {
                match fs::create_dir(dir) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error.into()),
                }
                receipts::directory(dir)?;
            }
            let _guard = reel_assets::admission::lock(&self.namespace)
                .map_err(|error| anyhow!("reinspection metadata lock refused: {error}"))?;
            let path = self.namespace.join("context.json");
            if path.try_exists()? {
                ensure!(
                    receipts::read(&path, PROFILE_MAX)? == encoded,
                    "reinspection context changed"
                );
            } else {
                receipts::exclusive(&path, &encoded)?;
            }
        } else {
            receipts::directory(parent)?;
            receipts::directory(&self.namespace)?;
        }
        ensure!(
            receipts::read(&self.namespace.join("context.json"), PROFILE_MAX)? == encoded,
            "reinspection context binding differs"
        );
        self.session.profile.control_root = self.namespace.clone();
        Ok(Object::from_bytes(&encoded))
    }

    fn validate_report(&mut self, report: &Value) -> Result<()> {
        ensure!(
            at(report, "reinspection")? == &self.declaration,
            "reinspection lineage differs"
        );
        self.session.validate_report(report)
    }

    fn summary(&self, mut value: Value) -> Value {
        value["review_purpose"] = json!("reinspection");
        value["prior_report"] = json!(self.prior);
        value["new_planned_clocks"] = json!(0);
        value["human_approval"] = json!(false);
        value
    }
}

pub fn inspect(profile: &Path) -> Result<Value> {
    let review = Reinspection::open(profile)?;
    let context = Object::from_bytes(&review.context_bytes()?);
    Ok(
        review.summary(json!({"state":"metadata-verified-for-reinspection",
        "task":review.session.profile.task,"context":context,
        "clock_count":review.session.task["clock_count"],
        "image_occurrences":review.session.task["image_occurrences"],"new_image_views":0})),
    )
}

pub fn reserve(profile: &Path, owner: &str) -> Result<Value> {
    let mut review = Reinspection::open(profile)?;
    ensure!(
        !owner.trim().is_empty() && owner.len() <= 256,
        "invalid receipt owner"
    );
    ensure!(
        review.session.profile.receipt.is_none() && review.session.profile.evidence.is_none(),
        "reinspection reservation cannot replace receipt/evidence"
    );
    let context = review.activate(true)?;
    let p = &review.session.profile;
    let reserved = receipts::reserve(&p.control_root, &p.task, &p.method, &p.policy, owner)?;
    Ok(
        review.summary(json!({"state":"reserved-before-views","task":p.task,
        "receipt":reserved.receipt,"directory":reserved.directory,"context":context,
        "context_path":p.control_root.join("context.json"),
        "clock_count":review.session.task["clock_count"],
        "image_occurrences":review.session.task["image_occurrences"],"new_image_views":0})),
    )
}

pub fn resume(profile: &Path) -> Result<Value> {
    let mut review = Reinspection::open(profile)?;
    review.activate(false)?;
    let receipt = review.session.binding()?;
    let p = &review.session.profile;
    let root = p.control_root.clone();
    let task = p.task.clone();
    let recovered = receipts::recover(&root, &task, &receipt, &mut |report| {
        review.validate_report(report)
    })?;
    Ok(review.summary(match recovered {
        receipts::Recovery::Pending => json!({"state":"reserved-pending-explicit-recovery",
            "task":task,"receipt":receipt,"new_image_views":0,"automatic_view_permission":false}),
        receipts::Recovery::Completed { report } => json!({"state":"completed-validated-skip",
            "task":task,"receipt":receipt,"report":report,"new_image_views":0}),
    }))
}

pub fn submit(profile: &Path) -> Result<Value> {
    let mut review = Reinspection::open(profile)?;
    let context = review.activate(false)?;
    let receipt = review.session.binding()?;
    let root = review.session.profile.control_root.clone();
    let task_pin = review.session.profile.task.clone();
    if let receipts::Recovery::Completed { report } =
        receipts::recover(&root, &task_pin, &receipt, &mut |value| {
            review.validate_report(value)
        })?
    {
        return Ok(review.summary(json!({"state":"completed-validated-skip",
            "task":task_pin,"receipt":receipt,"report":report,"new_image_views":0})));
    }
    let evidence_pin = review
        .session
        .profile
        .evidence
        .clone()
        .ok_or_else(|| anyhow!("retained fresh reinspection evidence required"))?;
    let evidence = metadata(&mut review.session.cache, &evidence_pin)?;
    ensure!(
        pin(at(&evidence, "reinspection_context")?)? == context,
        "fresh evidence must bind the pre-view reinspection context"
    );
    let task = review.session.task.clone();
    let policy = review.session.policy.observations.clone();
    let mut report = build_subreport(&task, &task_pin, &evidence, &policy, &mut |image| {
        review.session.verify_image(image)
    })?;
    report["reinspection"] = review.declaration.clone();
    let report_pin = receipts::complete(&root, &task_pin, &receipt, &report, &mut |value| {
        review.validate_report(value)
    })?;
    Ok(
        review.summary(json!({"state":"observations-validated-report-saved",
        "task":task_pin,"receipt":receipt,"report":report_pin,
        "report_path":root.join(task_pin.sha256.as_str()).join("report.json"),
        "finite_scope_qualified_with_holds":report["finite_scope_qualified_with_holds"],
        "all_scope_qualification_pending":true,"publication_pending":true})),
    )
}
