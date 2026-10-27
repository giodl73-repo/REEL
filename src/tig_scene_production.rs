//! Optional local execution handoff; candidate claims are not publication/custody.
use crate::{
    scene_delivery::{self, Receipt},
    tig_inputs::{self, Source},
    tig_scene::{self, Request},
};
use anyhow::{Result, bail};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use tig_assets::graph::recipe::{
    Plan,
    produced::{BoundSubject, Claim, ContextPin},
};
use tig_core::{Digest, Object};
use tig_git::source::native::context::ProductionContext;

#[cfg(feature = "tig-publication")]
mod publication;
#[cfg(feature = "tig-publication")]
pub use publication::{Publication, PublishedCandidate};
#[cfg(feature = "tig-publication")]
pub mod jobs;

/// Explicit project mapping; no inference from filenames, language or status.
pub struct OutputBinding<'a> {
    pub output_name: &'a str,
    pub subject: &'a BoundSubject,
}

/// Constructed only by this module's native compile/render/independent check.
/// Private paths and receipt are not automatically serialized into public logs.
pub struct RenderedCandidate {
    recipe: Plan,
    context: ContextPin,
    receipt: Receipt,
    receipt_object: Object,
    output: PathBuf,
}
impl RenderedCandidate {
    pub fn recipe(&self) -> &Plan {
        &self.recipe
    }
    pub fn receipt(&self) -> &Receipt {
        &self.receipt
    }
    pub fn receipt_object(&self) -> &Object {
        &self.receipt_object
    }

    /// Reverify exact local bytes, then create an unsigned asset-side assertion.
    /// Caller retains/publishes the claim and bytes and manages output custody.
    /// Input guards ended with rendering; this is not a retention lease.
    pub fn claim(&self, bindings: &[OutputBinding<'_>]) -> Result<Claim> {
        if bindings.is_empty() || bindings.len() > 256 {
            bail!("rendered candidate output mapping exceeds budget");
        }
        if Object::from_bytes(&receipt_bytes(&self.output)?) != self.receipt_object {
            bail!("rendered candidate receipt changed");
        }
        let mut subjects = Vec::with_capacity(bindings.len());
        for binding in bindings {
            let item = self
                .receipt
                .outputs
                .get(binding.output_name)
                .ok_or_else(|| anyhow::anyhow!("unknown rendered candidate output"))?;
            let expected = Object {
                sha256: Digest::parse(&item.sha256)?,
                bytes: item.bytes,
            };
            if binding.subject.subject().object() != &expected {
                bail!("rendered output differs from admitted asset subject");
            }
            let path = self.output.join(&item.path);
            if !path.is_file()
                || fs::metadata(&path)?.len() != expected.bytes
                || crate::sha256_file(&path)? != expected.sha256.as_str()
                || fs::metadata(&path)?.len() != expected.bytes
            {
                bail!("rendered candidate output bytes changed");
            }
            subjects.push(binding.subject.subject().clone());
        }
        Ok(Claim::create(&self.recipe, self.context.clone(), subjects)?)
    }
}

fn receipt_bytes(output: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(output.join("receipt.json"))?
        .take(tig_inputs::JOB_BYTES_MAX as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > tig_inputs::JOB_BYTES_MAX {
        bail!("rendered candidate receipt exceeds budget");
    }
    Ok(bytes)
}

/// Caller first durably protects/adopts the verified context and admits resources.
/// No provider/journal/cache setup or output selection occurs in this function.
/// `producer` is explicit caller admission, not verified full tool/runtime closure.
pub fn render(
    source: &mut Source,
    request: &Request,
    context: &ProductionContext,
    producer: &Object,
    job_directory: &Path,
    output: &Path,
) -> Result<RenderedCandidate> {
    render_inner(
        source,
        request,
        context,
        producer,
        RenderOptions {
            programs: None,
            outputs: None,
        },
        job_directory,
        output,
    )
}

/// Render using a held, byte-verified host/tool manifest and absolute programs.
/// Declared runtime components are verified, not the complete OS/environment.
/// Caller retains the manifest/code and admits all job resources before this call.
pub fn render_with_programs(
    source: &mut Source,
    request: &Request,
    context: &ProductionContext,
    programs: &crate::media_runtime::BoundPrograms,
    job_directory: &Path,
    output: &Path,
) -> Result<RenderedCandidate> {
    render_with_programs_and_outputs(
        source,
        request,
        context,
        programs,
        job_directory,
        output,
        None,
    )
}
/// Verify the complete native output contract before rendering any media.
pub fn render_with_programs_and_outputs(
    source: &mut Source,
    request: &Request,
    context: &ProductionContext,
    programs: &crate::media_runtime::BoundPrograms,
    job_directory: &Path,
    output: &Path,
    expected_outputs: Option<&BTreeSet<String>>,
) -> Result<RenderedCandidate> {
    let identity = programs.identity();
    let producer = Object {
        sha256: Digest::parse(&identity.sha256)?,
        bytes: identity.bytes,
    };
    render_inner(
        source,
        request,
        context,
        &producer,
        RenderOptions {
            programs: Some(programs),
            outputs: expected_outputs,
        },
        job_directory,
        output,
    )
}

fn context_recipe(
    source: &mut Source,
    request: &Request,
    context: &ProductionContext,
    producer: &Object,
) -> Result<Plan> {
    let recipe = tig_scene::recipe(source, request, producer)?;
    let requirements = context.requirements();
    if recipe.checkpoint() != requirements.checkpoint()
        || recipe.admission() != requirements.admission()
        || recipe.binding() != requirements.binding_digest()
        || recipe.operation().parameters != Object::from_bytes(context.descriptor())
    {
        bail!("render source/request differs from frozen native context");
    }
    Ok(recipe)
}

struct RenderOptions<'a> {
    programs: Option<&'a crate::media_runtime::BoundPrograms>,
    outputs: Option<&'a BTreeSet<String>>,
}
fn render_inner(
    source: &mut Source,
    request: &Request,
    context: &ProductionContext,
    producer: &Object,
    options: RenderOptions<'_>,
    job_directory: &Path,
    output: &Path,
) -> Result<RenderedCandidate> {
    let recipe = context_recipe(source, request, context, producer)?;
    if job_directory.exists() || output.exists() {
        bail!("native candidate requires new job and output directories");
    }
    tig_scene::compile(source, request, job_directory)?;
    let job_path = job_directory.join("delivery/job.json").canonicalize()?;
    let job = tig_inputs::read_job(&job_path)?;
    let inputs = source.prepare(&job)?;
    let protected = options
        .programs
        .map(|programs| programs.with_inputs(&inputs));
    let execution: &dyn scene_delivery::InputFiles = match &protected {
        Some(bound) => bound,
        None => &inputs,
    };
    if let Some(expected) = options.outputs {
        let (job, plan) = scene_delivery::plan_with_inputs(&job_path, execution)?;
        let mut actual = scene_delivery::output_contract(&job, &plan)?.names;
        actual.insert("receipt.json".into());
        if &actual != expected {
            bail!("native worker output set differs from admission");
        }
    }
    let rendered = scene_delivery::render_with_inputs(&job_path, execution, output)?;
    let receipt = scene_delivery::check_with_inputs(&job_path, execution, output)?;
    let bytes = receipt_bytes(output)?;
    let reread: Receipt = serde_json::from_slice(&bytes)?;
    if serde_json::to_value(&rendered)? != serde_json::to_value(&receipt)?
        || serde_json::to_value(&reread)? != serde_json::to_value(&receipt)?
    {
        bail!("native rendered receipt changed during independent verification");
    }
    let receipt_object = Object::from_bytes(&bytes);
    Ok(RenderedCandidate {
        recipe,
        context: ContextPin {
            root: context.selection().root.clone(),
            identity: context.selection().context.clone(),
        },
        receipt,
        receipt_object,
        output: output.canonicalize()?,
    })
}
