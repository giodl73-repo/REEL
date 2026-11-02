//! Browser execution inside the existing admitted/retained native job lifecycle.
use super::*;
use crate::{media_runtime::BoundTool, scene_delivery::FileRef, tig_browser};
use tig_assets::graph::recipe::produced::{Claim, ContextPin};
use tig_git::source::native::context::ProductionContext;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Execution {
    pub program: FileRef,
    pub limits: tig_browser::render::Limits,
    pub clean_picture: bool,
}
impl Execution {
    pub(super) fn validate(&self, worker: &Worker) -> Result<()> {
        self.limits.validate()?;
        worker.request.browser()?.validate()?;
        let source: ArchiveProfile = serde_json::from_value(worker.source.clone())?;
        if source.byte_source.required != tig_store::existing::RequiredProtection::PreventMutation {
            bail!("browser execution requires mutation-preventing primary protection");
        }
        if worker.output_budget.is_some()
            || worker.outputs.len() != 2
            || worker.outputs.get("master.mkv") != Some(&self.limits.max_output_bytes)
            || worker.outputs.get("receipt.json") != Some(&65_536)
            || worker
                .record_outputs
                .iter()
                .any(|spec| spec.output != "master.mkv")
        {
            bail!("browser job requires exact master/receipt output bounds");
        }
        Ok(())
    }
}
pub(super) fn produce(
    ticket: &Ticket,
    source: &mut crate::tig_inputs::Source,
    context: &ProductionContext,
    plan: &JobPlan<'_>,
    programs: &BoundPrograms,
    browser: &BoundTool,
    templates: &records::Prepared,
) -> Result<Value> {
    let execution = ticket
        .admission
        .browser
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("browser enrollment missing"))?;
    execution.validate(&ticket.admission)?;
    let request = ticket.admission.request.browser()?;
    let prepared = tig_browser::lower(source, request)?;
    let files = prepared.inputs.values().cloned().collect::<Vec<_>>();
    let inputs = source.prepare_files(&files)?;
    let cwd = std::env::current_dir()?.canonicalize()?;
    let workspace = cwd.join("native");
    fs::create_dir(&workspace)?;
    let path = workspace.join("master.mkv");
    // The parent runner continuously checks the exact journal/source ownership
    // and owns this worker's whole process tree. No second fence or global lock.
    let rendered = tig_browser::render::native(
        &prepared,
        &inputs,
        programs,
        browser,
        &workspace,
        &path,
        &execution.limits,
        execution.clean_picture,
        || Ok(()),
    )?;
    if rendered.producer != plan.recipe().operation().producer
        || tig_browser::recipe(source, request, context, &rendered.producer)?.fingerprint()
            != plan.recipe().fingerprint()
    {
        bail!("browser result differs from frozen native recipe");
    }
    let outputs = BTreeMap::from([(
        "master.mkv".into(),
        FileRef {
            path: PathBuf::from("master.mkv"),
            sha256: rendered.output.sha256.as_str().into(),
            bytes: rendered.output.bytes,
        },
    )]);
    let receipt = identity::canonical_json(&json!({
        "schema":"reel.tig-browser-native-receipt.experimental-1",
        "recipe":plan.recipe().fingerprint(), "context":ticket.admission.context,
        "producer":rendered.producer,"outputs":outputs,"frames":rendered.frames,
        "samples_per_channel":rendered.samples_per_channel,"pcm_sha256":rendered.pcm_sha256,
        "capture": {"frames":rendered.capture.frames,"requests":rendered.capture.requests,
            "frame_sequence_sha256":rendered.capture.frame_sequence_sha256,"served_bytes":rendered.capture.served_bytes},
        "selected":false,"approved":false
    }))?;
    if receipt.len() > 65_536 {
        bail!("browser receipt exceeds admission");
    }
    let receipt_pin = Object::from_bytes(&receipt);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(workspace.join("receipt.json"))?;
    file.write_all(&receipt)?;
    file.sync_all()?;
    let exported = records::emit_candidate(templates, &outputs, &receipt_pin, |bindings| {
        read_bytes(&workspace.join("receipt.json"), 65_536, Some(&receipt_pin))?;
        if fs::metadata(&path)?.len() != rendered.output.bytes
            || crate::sha256_file(&path)? != rendered.output.sha256.as_str()
            || fs::metadata(&path)?.len() != rendered.output.bytes
        {
            bail!("browser candidate changed before claim");
        }
        let mut subjects = Vec::new();
        for binding in bindings {
            if binding.output_name != "master.mkv"
                || binding.subject.subject().object() != &rendered.output
            {
                bail!("browser claim output mapping differs");
            }
            subjects.push(binding.subject.subject().clone());
        }
        Ok(Claim::create(
            plan.recipe(),
            ContextPin {
                root: context.selection().root.clone(),
                identity: context.selection().context.clone(),
            },
            subjects,
        )?)
    })?;
    let result = json!({"schema":"reel.tig-native-worker-result.experimental-1",
        "context":ticket.admission.context,"recipe":plan.recipe().fingerprint(),"producer":ticket.producer,
        "receipt":receipt_pin,"record_candidates":exported,"production_claim":null});
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open("worker.json")?;
    file.write_all(&identity::canonical_json(&result)?)?;
    file.sync_all()?;
    Ok(result)
}
