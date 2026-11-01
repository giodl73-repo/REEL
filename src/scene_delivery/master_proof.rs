//! Retained full-check witnesses for master-only consumption. These are not
//! authenticated approvals or substitutes for sealed native producer proofs.
use super::*;

const PROOF_BYTES: u64 = 65_536;

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MasterProof {
    schema: String,
    full_check_passed: bool,
    validator_sha256: String,
    ffmpeg_sha256: String,
    ffprobe_sha256: String,
    job_sha256: String,
    plan_sha256: String,
    delivery_receipt_sha256: String,
    delivery_receipt_bytes: u64,
    master_sha256: String,
    master_bytes: u64,
}

/// Providers retain their own exact byte/tool guards through the call. The
/// validator is hashed as historical identity; it is never executed.
pub struct MasterProofFiles<'a> {
    pub inputs: &'a dyn InputFiles,
    pub outputs: &'a dyn InputFiles,
    pub evidence: &'a dyn InputFiles,
    pub receipt: &'a FileRef,
    pub proof: &'a FileRef,
    pub validator: Option<&'a FileRef>,
}

fn facts(
    receipt: &Receipt,
    receipt_pin: &FileRef,
    inputs: &(impl InputFiles + ?Sized),
    validator: Option<(&FileRef, &dyn InputFiles)>,
) -> Result<MasterProof> {
    let master = receipt
        .outputs
        .get("master.mkv")
        .context("receipt lacks master")?;
    if master.path != Path::new("master.mkv") {
        bail!("scene master proof has unexpected master path");
    }
    let validator_sha256 = if let Some((reference, evidence)) = validator {
        let path = evidence.verified_path(reference)?;
        let hash = crate::sha256_file(&path)?;
        if hash != reference.sha256 || fs::metadata(path)?.len() != reference.bytes {
            bail!("validator identity differs from admitted bytes");
        }
        hash
    } else {
        crate::sha256_file(&std::env::current_exe()?)?
    };
    let tool_hash = |program| -> Result<String> {
        let command = crate::episode_conform::command_with_inputs(inputs, program)?;
        crate::sha256_file(Path::new(command.get_program()))
    };
    Ok(MasterProof {
        schema: "reel.scene-master-proof.v1".into(),
        full_check_passed: true,
        validator_sha256,
        ffmpeg_sha256: tool_hash(MediaProgram::Ffmpeg)?,
        ffprobe_sha256: tool_hash(MediaProgram::Ffprobe)?,
        job_sha256: receipt.plan.job_sha256.clone(),
        plan_sha256: crate::production::sha256_bytes(&serde_json::to_vec(&receipt.plan)?),
        delivery_receipt_sha256: receipt_pin.sha256.clone(),
        delivery_receipt_bytes: receipt_pin.bytes,
        master_sha256: master.sha256.clone(),
        master_bytes: master.bytes,
    })
}

fn recheck(plan: Plan, files: &MasterProofFiles<'_>) -> Result<Receipt> {
    let receipt: Receipt = serde_json::from_slice(&read_control(files.outputs, files.receipt)?)?;
    let proof: MasterProof = serde_json::from_slice(&read_control(files.evidence, files.proof)?)?;
    if receipt.schema != "reel.scene-delivery-receipt.v0.1"
        || serde_json::to_value(&receipt.plan)? != serde_json::to_value(&plan)?
        || receipt.content_samples != plan.duration_samples
        || receipt.delivery_frames != plan.frame_count
        || proof
            != facts(
                &receipt,
                files.receipt,
                files.inputs,
                files.validator.map(|v| (v, files.evidence)),
            )?
    {
        bail!("scene master proof differs from current inputs, receipt or tools");
    }
    files.outputs.verified_path(
        receipt
            .outputs
            .get("master.mkv")
            .context("receipt lacks master")?,
    )?;
    Ok(receipt)
}

/// Replan exact primary inputs; read only the consumed master, receipt and
/// explicitly admitted witness. No sibling discovery or ambient byte fallback.
pub fn recheck_master_proof_with_output_files(
    job_path: &Path,
    files: &MasterProofFiles<'_>,
) -> Result<Receipt> {
    limits(files)?;
    let (_, plan) = plan_with_inputs(job_path, files.inputs)?;
    recheck(plan, files)
}

pub fn recheck_master_proof_with_all_files(
    job: &FileRef,
    production: &FileRef,
    controls: &(impl InputFiles + ?Sized),
    files: &MasterProofFiles<'_>,
) -> Result<Receipt> {
    limits(files)?;
    let (_, plan) = plan_with_control_files(job, production, files.inputs, controls)?;
    recheck(plan, files)
}
fn limits(files: &MasterProofFiles<'_>) -> Result<()> {
    if files.proof.bytes > PROOF_BYTES || files.receipt.bytes > 4 * 1024 * 1024 {
        bail!("master witness metadata exceeds budget");
    }
    Ok(())
}

fn local_pin(path: &Path, logical: &str) -> Result<FileRef> {
    Ok(FileRef {
        path: logical.into(),
        sha256: crate::sha256_file(path)?,
        bytes: fs::metadata(path)?.len(),
    })
}

struct LocalEvidence<'a> {
    proof: &'a Path,
    validator: Option<&'a Path>,
}
impl InputFiles for LocalEvidence<'_> {
    fn verified_path(&self, reference: &FileRef) -> Result<PathBuf> {
        let path = match reference.path.to_str() {
            Some("proof.json") => self.proof,
            Some("validator") => self.validator.context("validator not admitted")?,
            _ => bail!("evidence not admitted"),
        };
        if local_pin(path, reference.path.to_str().unwrap())? != *reference {
            bail!("evidence differs from pin");
        }
        Ok(path.to_owned())
    }
}

pub fn recheck_master_proof(
    job_path: &Path,
    asset_root: &Path,
    output: &Path,
    proof_path: &Path,
) -> Result<Receipt> {
    local_recheck(job_path, asset_root, output, proof_path, None)
}

pub fn recheck_master_proof_with_validator(
    job_path: &Path,
    asset_root: &Path,
    output: &Path,
    proof_path: &Path,
    validator: &Path,
) -> Result<Receipt> {
    local_recheck(job_path, asset_root, output, proof_path, Some(validator))
}

fn local_recheck(
    job_path: &Path,
    asset_root: &Path,
    output: &Path,
    proof_path: &Path,
    validator: Option<&Path>,
) -> Result<Receipt> {
    if fs::metadata(proof_path)?.len() > PROOF_BYTES
        || fs::metadata(output.join("receipt.json"))?.len() > 4 * 1024 * 1024
    {
        bail!("master witness metadata exceeds budget");
    }
    let evidence = LocalEvidence {
        proof: proof_path,
        validator,
    };
    let receipt = local_pin(&output.join("receipt.json"), "receipt.json")?;
    let proof = local_pin(proof_path, "proof.json")?;
    let validator = validator.map(|p| local_pin(p, "validator")).transpose()?;
    recheck_master_proof_with_output_files(
        job_path,
        &MasterProofFiles {
            inputs: &asset_root.to_path_buf(),
            outputs: &output.to_path_buf(),
            evidence: &evidence,
            receipt: &receipt,
            proof: &proof,
            validator: validator.as_ref(),
        },
    )
}

/// Issue only after the complete output set, including unconsumed outputs,
/// passes the current full checker. Atomic no-clobber publication.
pub fn prove_master(
    job_path: &Path,
    asset_root: &Path,
    output: &Path,
    proof_path: &Path,
) -> Result<()> {
    if proof_path.symlink_metadata().is_ok() {
        bail!("scene master proof output must be new");
    }
    if fs::metadata(output.join("receipt.json"))?.len() > 4 * 1024 * 1024 {
        bail!("receipt exceeds metadata budget");
    }
    let checked = check(job_path, asset_root, output)?;
    let receipt = local_pin(&output.join("receipt.json"), "receipt.json")?;
    let proof = facts(&checked, &receipt, asset_root, None)?;
    let parent = proof_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    std::io::Write::write_all(&mut staged, &serde_json::to_vec_pretty(&proof)?)?;
    staged.as_file().sync_all()?;
    staged.persist_noclobber(proof_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Never;
    impl InputFiles for Never {
        fn verified_path(&self, _: &FileRef) -> Result<PathBuf> {
            panic!("provider accessed")
        }
        fn control_bytes(&self, _: &FileRef) -> Result<Vec<u8>> {
            panic!("metadata provider accessed")
        }
        fn media_program(&self, _: MediaProgram) -> Result<PathBuf> {
            panic!("tool provider accessed")
        }
    }
    #[test]
    fn oversized_witness_metadata_refuses_before_planning_or_provider_access() {
        let small = FileRef {
            path: "absent".into(),
            sha256: "a".repeat(64),
            bytes: 1,
        };
        for (proof_bytes, receipt_bytes) in [(PROOF_BYTES + 1, 1), (1, 4 * 1024 * 1024 + 1)] {
            let mut proof = small.clone();
            proof.bytes = proof_bytes;
            let mut receipt = small.clone();
            receipt.bytes = receipt_bytes;
            let files = MasterProofFiles {
                inputs: &Never,
                outputs: &Never,
                evidence: &Never,
                receipt: &receipt,
                proof: &proof,
                validator: None,
            };
            assert!(
                recheck_master_proof_with_output_files(Path::new("absent"), &files)
                    .unwrap_err()
                    .to_string()
                    .contains("budget")
            );
            assert!(
                recheck_master_proof_with_all_files(&small, &small, &Never, &files)
                    .unwrap_err()
                    .to_string()
                    .contains("budget")
            );
        }
    }
}
