//! Resume only an already recorded prepare. Never run tools or retire scratch.
use super::*;
use anyhow::Context;
use serde_json::{Value, json};
use std::io::{Read, Seek, SeekFrom};
use tig_store::{
    existing::{ByteSource, ReadCache, RequiredProtection, VerifiedRead},
    source_claims::PreviewDisposition,
};
pub(super) mod cli;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryBudget {
    pub max_objects: u32,
    pub max_verify_bytes: u64,
    pub verify_millis: u64,
}
fn bytes(guard: &mut VerifiedRead, max: usize) -> Result<Vec<u8>> {
    if guard.object().bytes > max as u64 {
        bail!("recovery control exceeds bound");
    }
    guard.seek(SeekFrom::Start(0))?;
    let mut raw = Vec::new();
    guard.take(max as u64 + 1).read_to_end(&mut raw)?;
    if raw.len() > max {
        bail!("recovery control exceeds bound");
    }
    Ok(raw)
}
fn get<'a>(entries: &'a BTreeMap<LogicalPath, Object>, name: &str) -> Result<&'a Object> {
    entries
        .get(&LogicalPath::parse(name)?)
        .context("prepared result reference missing")
}

/// Complete the exact pending result transition, or verify its idempotent replay.
/// The original input context, source/owner, operation, job ID and budgets are
/// caller admissions. Header/result booleans remain technical observations, not
/// producer claims. This does not recover a missing prepare or authorize scratch
/// removal; unretained workspaces and child-owner handoff remain separate work.
pub fn adopt_prepared_result(
    journal_root: &Path,
    job_id: &str,
    resources: &JobContext,
    journal: &mut SourceJournal,
    profile: &Profile<'_>,
    budget: &RecoveryBudget,
) -> Result<Value> {
    if !resources.of(Role::ClaimedOutput).is_empty() {
        bail!("prepared recovery requires an input context");
    }
    adopt_selection(
        journal_root,
        job_id,
        resources.selection(),
        journal,
        profile,
        budget,
    )
}

fn adopt_selection(
    journal_root: &Path,
    job_id: &str,
    original_input: &Selection,
    journal: &mut SourceJournal,
    profile: &Profile<'_>,
    budget: &RecoveryBudget,
) -> Result<Value> {
    if !job_id.starts_with("job-")
        || job_id.len() > 100
        || !job_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        || !(2..=1024).contains(&budget.max_objects)
        || budget.max_verify_bytes == 0
        || !(1..=300000).contains(&budget.verify_millis)
    {
        bail!("invalid prepared retention recovery admission");
    }
    let _fence = fence(profile.coordination, journal, &profile.identity)?;
    let before = journal
        .source(&profile.identity.source)?
        .context("recovery source missing")?;
    let prepared_revision = profile
        .expected_revision
        .checked_add(1)
        .context("recovery revision overflow")?;
    let completed_revision = prepared_revision
        .checked_add(1)
        .context("recovery revision overflow")?;
    if before.owner != profile.identity.owner {
        bail!("recovery source owner differs");
    }
    let selection = if before.revision == prepared_revision {
        if before.current.as_ref().is_none_or(|c| {
            c.revision != profile.expected_revision || c.selection != *original_input
        }) {
            bail!("recovery original input ownership differs");
        }
        before
            .pending
            .as_ref()
            .context("prepared retention missing")?
            .selection
            .clone()
    } else if before.revision == completed_revision && before.pending.is_none() {
        if !before
            .predecessors
            .iter()
            .any(|p| p.revision == profile.expected_revision && p.selection == *original_input)
        {
            bail!("recovery input predecessor differs");
        }
        let current = before
            .current
            .as_ref()
            .context("completed retention missing")?;
        if current.revision != completed_revision {
            bail!("completed retention version differs");
        }
        current.selection.clone()
    } else {
        bail!("no matching prepared retention; preserve workspace");
    };
    let command = Command::Prepare {
        source: profile.identity.source.clone(),
        owner: profile.identity.owner.clone(),
        expected_revision: profile.expected_revision,
        selection: selection.clone(),
    };
    // Preview must identify the original exact recorded command; never create a
    // new prepare to make a different operation look resumable.
    let PreviewDisposition::Recorded { receipt: prepared } = SourceJournal::preview(
        journal_root,
        &journal.checkpoint(),
        retention_phase(profile, "prepare")?,
        command,
    )?
    .disposition
    else {
        bail!("retention prepare was not recorded by this operation");
    };
    if prepared.source_revision != prepared_revision
        || before
            .pending
            .as_ref()
            .is_some_and(|p| p.reference != prepared.reference)
    {
        bail!("retention prepare receipt differs");
    }
    let mut reader = MetadataReader::open(profile.metadata_root, profile.metadata_minimum)?;
    let entries = Tree::from_root(selection.root.clone())
        .entries(&mut reader, profile.tree_limits)?
        .0;
    let header = get(&entries, "retention/identity")?;
    if header.sha256 != selection.context
        || header.bytes > 65536
        || get(&entries, "source/context")? != &original_input.root
    {
        bail!("prepared retention identity differs");
    }
    let header = identity::parse_json(&reader.get(header)?.context("retention header missing")?)?;
    let succeeded = header["succeeded"]
        .as_bool()
        .context("retention outcome missing")?;
    if header["schema"] != "reel.tig-runner-retention.experimental-1"
        || header["job_id"] != job_id
        || header["source"] != serde_json::to_value(original_input)?
        || header["journal"] != serde_json::to_value(journal.identity())?
        || header["owner"] != profile.identity.owner.as_str()
        || header["source_id"] != profile.identity.source.as_str()
        || header["revision"].as_u64() != Some(profile.expected_revision)
        || header.get("production_claim") != Some(&Value::Null)
        || header["outputs"].as_u64() != entries.len().checked_sub(4).map(|n| n as u64)
    {
        bail!("prepared retention scope differs");
    }
    let mut objects = BTreeMap::new();
    for (name, object) in &entries {
        if matches!(name.as_str(), "source/context" | "retention/identity") {
            continue;
        }
        if name.as_str() != "result/request"
            && name.as_str() != "result/evidence"
            && !name.as_str().starts_with("outputs/")
        {
            bail!("unexpected prepared result role");
        }
        if objects
            .insert(object.sha256.clone(), object.clone())
            .is_some_and(|old| old != *object)
        {
            bail!("conflicting prepared result lengths");
        }
    }
    if objects.len() > budget.max_objects as usize {
        bail!("prepared result count exceeds budget");
    }
    let mut cache = ReadCache::new(serde_json::from_value(json!({
        "places":profile.config.cache_roots.iter().map(|root| json!({"root":root,"layout":"reel-cas"})).collect::<Vec<_>>(),
        "required":RequiredProtection::PreventMutation,
        "limits":{"object_bytes":budget.max_verify_bytes,"total_bytes":budget.max_verify_bytes,"requests":budget.max_objects,
            "live_readers":budget.max_objects,"verify_millis":budget.verify_millis},
    }))?)?;
    let mut guards = objects
        .iter()
        .map(|(id, object)| Ok((id.clone(), cache.open_verified(object)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let request = get(&entries, "result/request")?;
    let raw = bytes(
        guards
            .get_mut(&request.sha256)
            .context("request guard missing")?,
        256 * 1024,
    )?;
    jobs::check_request(&raw).map_err(|error| anyhow::anyhow!("{error}"))?;
    let document = identity::parse_json(&raw)?;
    let evidence = get(&entries, "result/evidence")?;
    let compressed = bytes(
        guards
            .get_mut(&evidence.sha256)
            .context("evidence guard missing")?,
        3 * 1024 * 1024,
    )?;
    let mut decoded = Vec::new();
    flate2::read::GzDecoder::new(compressed.as_slice())
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut decoded)?;
    if decoded.len() > 2 * 1024 * 1024 {
        bail!("retention evidence exceeds decoded bound");
    }
    let evidence = identity::parse_json(&decoded)?;
    let outputs = evidence["declared_retained_outputs"]
        .as_object()
        .context("retention output declarations missing")?;
    if evidence["schema"] != "reel.job-retirement-evidence.v1"
        || evidence["request_utf8"].as_str().map(str::as_bytes) != Some(raw.as_slice())
        || evidence["result"]["job_id"] != job_id
        || evidence["result"]["manifest_sha256"] != request.sha256.as_str()
        || evidence["result"]["schema"]
            != if succeeded {
                "caminos.production-job-result.v1"
            } else {
                "reel.failed-job-result.v1"
            }
        || entries.len() != outputs.len() + 4
        || (succeeded && evidence["result"]["outputs"] != evidence["declared_retained_outputs"])
        || (succeeded
            && document["outputs"].as_object().map(serde_json::Map::len) != Some(outputs.len()))
    {
        bail!("retained request or evidence differs");
    }
    for (name, value) in outputs {
        let limit = document["outputs"][name]["max_bytes"]
            .as_u64()
            .context("retained output outside request")?;
        let object = get(
            &entries,
            &format!("outputs/{}", Digest::blob(name.as_bytes()).as_str()),
        )?;
        if value["sha256"] != object.sha256.as_str()
            || value["bytes"].as_u64() != Some(object.bytes)
            || object.bytes > limit
        {
            bail!("retained output differs from admitted pin or bound");
        }
    }
    if journal.source(&profile.identity.source)? != Some(before) {
        bail!("recovery source changed while verifying");
    }
    let receipt = journal.apply(
        retention_phase(profile, "adopt")?,
        Command::Adopt {
            source: profile.identity.source.clone(),
            owner: profile.identity.owner.clone(),
            expected_revision: prepared_revision,
            prepared: prepared.reference,
        },
    )?;
    let current = journal
        .source(&profile.identity.source)?
        .context("recovered source missing")?;
    if current.owner != profile.identity.owner
        || current.revision != completed_revision
        || current.pending.is_some()
        || current
            .current
            .as_ref()
            .is_none_or(|c| c.revision != completed_revision || c.selection != selection)
    {
        bail!("recovered retention is no longer current");
    }
    Ok(
        json!({"schema":"reel.tig-prepared-retention-recovery.experimental-1","job_id":job_id,
        "selection":selection,"source_revision":receipt.source_revision,"replayed":receipt.replayed,"succeeded":succeeded,
        "staging_retired":false,"authorizes_workspace_removal":false,"production_claim":null,"approval":false,
        "journal_checkpoint":journal.checkpoint()}),
    )
}
