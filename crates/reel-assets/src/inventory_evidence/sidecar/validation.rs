//! Explicit whole-index verification for protection handoff, not ordinary lookup.
use super::*;

/// Private exact descriptor set; no automatic evidence export.
pub struct ValidatedIndex {
    pub nodes_read: usize,
    pub bytes_read: u64,
    pub objects: usize,
    pub copies: usize,
    pub(super) descriptors: Vec<Blob>,
}
/// Walk the complete tree. `limits.rows` is the TOTAL object budget here.
/// Still requires a caller-admitted exact root and source; this is consistency
/// and integrity checking, not independent source inclusion or authority proof.
pub fn validate(
    store: &mut impl Store,
    root: &Blob,
    expected: &Source,
    limits: ReadLimits,
) -> Result<ValidatedIndex> {
    check_node_limit(limits.node_bytes)?;
    if expected.schema != SOURCE_SCHEMA
        || expected.adapter_version != ADAPTER_VERSION
        || !valid_oid(&expected.encoded.sha256)
        || !valid_oid(&expected.decoded.sha256)
        || expected.project_id.trim().is_empty()
        || expected.authority_namespace.trim().is_empty()
        || expected.project_id.len() > 256
        || expected.authority_namespace.len() > 256
    {
        return Err("Invalid expected inventory source".into());
    }
    let mut meter = Meter {
        limits,
        nodes: 0,
        bytes: 0,
    };
    let header: Header = decode(&meter.read(store, root)?)?;
    if header.schema != HEADER
        || header.source != source_key(expected)
        || !valid_oid(&header.profile)
        || header.objects > 1_000_000
        || header.objects > limits.rows
        || header.copies > 4_000_000
        || header.tiers.len() > MAX_TIERS
        || header.capture.is_some_and(|(s, e)| s > e)
    {
        return Err("Invalid inventory closure header/scope/budget".into());
    }
    let mut tiers = BTreeMap::new();
    for t in header.tiers {
        if t.root.trim().is_empty() || tiers.contains_key(&t.tier) {
            return Err("Invalid inventory closure tiers".into());
        }
        tiers.insert(
            t.tier,
            TierObservation {
                tier: t.tier,
                root: t.root,
                volume_id: t.volume_id,
            },
        );
    }
    let mut descriptors = BTreeMap::from([(root.sha256.clone(), root.clone())]);
    let mut pending = vec![(String::new(), header.tree.blob())];
    let mut ordinals = vec![false; header.objects];
    let mut objects = 0usize;
    let mut copies = 0usize;
    while let Some((prefix, blob)) = pending.pop() {
        if descriptors
            .insert(blob.sha256.clone(), blob.clone())
            .is_some()
        {
            return Err("Repeated/cyclic inventory closure node".into());
        }
        let node: Node = decode(&meter.read(store, &blob)?)?;
        if node.schema != NODE || node.prefix != prefix {
            return Err("Invalid inventory closure routing/schema".into());
        }
        match node.body {
            Body::Branch { children } => {
                if prefix.len() >= 64 || children.is_empty() || children.len() > 16 {
                    return Err("Invalid closure branch".into());
                }
                for (key, child) in children {
                    if key.len() != prefix.len() + 1
                        || !key.starts_with(&prefix)
                        || !key
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    {
                        return Err("Invalid closure child routing".into());
                    }
                    pending.push((key, child.blob()));
                }
            }
            Body::Leaf { rows } => {
                if rows.len() > 4096
                    || (rows.is_empty() && (header.objects != 0 || !prefix.is_empty()))
                {
                    return Err("Invalid closure leaf size".into());
                }
                objects = objects
                    .checked_add(rows.len())
                    .ok_or("Closure object overflow")?;
                if objects > header.objects || objects > limits.rows {
                    return Err("Closure object budget exceeded".into());
                }
                let mut last = String::new();
                for wire in rows {
                    let row: ObservedObject = serde_json::from_value(wire.observation.clone())?;
                    if wire.observation != observation(&row)
                        || row.sha256 <= last
                        || !row.sha256.starts_with(&prefix)
                        || wire.record >= header.objects
                        || ordinals[wire.record]
                    {
                        return Err("Invalid closure row/ordinal".into());
                    }
                    validate_observation(&row, &tiers)?;
                    ordinals[wire.record] = true;
                    last = row.sha256;
                    copies = copies
                        .checked_add(row.copies.len())
                        .ok_or("Closure copy overflow")?;
                    if copies > header.copies {
                        return Err("Closure copy count exceeded".into());
                    }
                }
            }
        }
    }
    if objects != header.objects || copies != header.copies {
        return Err("Incomplete inventory closure counts".into());
    }
    Ok(ValidatedIndex {
        nodes_read: meter.nodes,
        bytes_read: meter.bytes,
        objects,
        copies,
        descriptors: descriptors.into_values().collect(),
    })
}
