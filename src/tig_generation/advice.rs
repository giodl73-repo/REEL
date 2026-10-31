//! Bounded catch-up ordering for one exact source view. Advice never grants
//! execution, selection or eviction. Direct basis truth is preserved separately.
use super::*;
use tig_assets::graph::recipe::assessment::State;
use tig_core::Digest;

pub struct Node {
    pub asset_id: String,
    pub checkpoint: Digest,
    /// None means unavailable, never absence or a matching basis.
    pub basis: Option<State>,
    pub upstream: BTreeSet<String>,
}
pub struct Entry {
    pub asset_id: String,
    pub basis: Option<State>,
    pub advice: State,
}

/// Missing/unproven/stale upstream bases block downstream work. Once an upstream
/// is selected and its declared basis matches, recompute all nodes at the NEW
/// source checkpoint. Cycles and mixed views refuse, rather than guessing order.
pub fn order(checkpoint: &Digest, nodes: &[Node], maximum: usize) -> Result<Vec<Entry>> {
    if !(1..=4096).contains(&maximum) || nodes.is_empty() || nodes.len() > maximum {
        bail!("generation advice scope exceeds bound");
    }
    let mut known = BTreeMap::new();
    for node in nodes {
        if !token(&node.asset_id)
            || &node.checkpoint != checkpoint
            || node.upstream.len() > 64
            || node.upstream.iter().any(|id| !token(id))
            || known.insert(node.asset_id.as_str(), node).is_some()
        {
            bail!("invalid generation advice view");
        }
    }
    let mut incoming = BTreeMap::new();
    let mut children: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut ready = BTreeSet::new();
    for node in nodes {
        let mut count = 0;
        for id in &node.upstream {
            if known.contains_key(id.as_str()) {
                count += 1;
                children.entry(id).or_default().push(&node.asset_id);
            }
        }
        incoming.insert(node.asset_id.as_str(), count);
        if count == 0 {
            ready.insert(node.asset_id.as_str());
        }
    }
    let mut evaluated = BTreeMap::new();
    let mut result = Vec::new();
    while let Some(id) = ready.pop_first() {
        let node = known[id];
        let advice = if node
            .upstream
            .iter()
            .any(|p| evaluated.get(p.as_str()) != Some(&State::ClaimedRecipeMatch))
        {
            State::Blocked
        } else {
            node.basis.unwrap_or(State::Blocked)
        };
        evaluated.insert(id, advice);
        result.push(Entry {
            asset_id: id.into(),
            basis: node.basis,
            advice,
        });
        for child in children.get(id).into_iter().flatten() {
            let count = incoming.get_mut(child).unwrap();
            *count -= 1;
            if *count == 0 {
                ready.insert(child);
            }
        }
    }
    if result.len() != nodes.len() {
        bail!("generation advice cycle");
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn node(id: &str, state: Option<State>, parents: &[&str]) -> Node {
        Node {
            asset_id: id.into(),
            checkpoint: Object::from_bytes(b"source").sha256,
            basis: state,
            upstream: parents.iter().map(|s| (*s).into()).collect(),
        }
    }
    #[test]
    fn stale_translation_blocks_a_matching_cue_until_translation_is_updated() {
        let checkpoint = Object::from_bytes(b"source").sha256;
        let nodes = [
            node("cue", Some(State::ClaimedRecipeMatch), &["translation"]),
            node("translation", Some(State::StaleRecipe), &[]),
        ];
        let result = order(&checkpoint, &nodes, 4).unwrap();
        assert_eq!(result[0].asset_id, "translation");
        assert_eq!(result[0].advice, State::StaleRecipe);
        assert_eq!(result[1].basis, Some(State::ClaimedRecipeMatch));
        assert_eq!(result[1].advice, State::Blocked);
        let ready = [
            node("translation", Some(State::ClaimedRecipeMatch), &[]),
            node("cue", Some(State::StaleRecipe), &["translation"]),
        ];
        let result = order(&checkpoint, &ready, 4).unwrap();
        assert_eq!(result[1].advice, State::StaleRecipe);
        assert!(result[1].advice.catch_up_candidate());
    }
    #[test]
    fn unknown_unproven_cycles_duplicates_mixed_views_and_bounds_never_become_ready() {
        let checkpoint = Object::from_bytes(b"source").sha256;
        for state in [
            None,
            Some(State::Unproven),
            Some(State::Unbuilt),
            Some(State::Blocked),
        ] {
            let result = order(
                &checkpoint,
                &[
                    node("a", state, &[]),
                    node("b", Some(State::Unbuilt), &["a"]),
                ],
                2,
            )
            .unwrap();
            assert_eq!(result[1].advice, State::Blocked);
        }
        assert_eq!(
            order(
                &checkpoint,
                &[node("b", Some(State::Unbuilt), &["missing"])],
                1
            )
            .unwrap()[0]
                .advice,
            State::Blocked
        );
        assert!(
            order(
                &checkpoint,
                &[node("a", None, &["b"]), node("b", None, &["a"])],
                2
            )
            .is_err()
        );
        assert!(
            order(
                &checkpoint,
                &[node("a", None, &[]), node("a", None, &[])],
                2
            )
            .is_err()
        );
        assert!(
            order(
                &Object::from_bytes(b"other").sha256,
                &[node("a", None, &[])],
                1
            )
            .is_err()
        );
        assert!(order(&checkpoint, &[node("a", None, &[])], 0).is_err());
    }
}
