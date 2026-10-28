//! Explicit shared aliases and per-language overrides; never infer variants.
use super::*;
use serde_json::Map;

pub(super) struct Catalog {
    pub effective: Map<String, Value>,
    pub ids: BTreeSet<String>,
    pub languages: BTreeSet<String>,
    pub rules: Vec<Rule>,
}
fn references(value: &Value) -> Result<&Map<String, Value>> {
    let refs = value.as_object().context("binding map must be an object")?;
    if refs.is_empty() || refs.len() > 4096 {
        bail!("native binding count exceeds budget");
    }
    for (key, value) in refs {
        let reference = value
            .as_object()
            .context("binding reference must be an object")?;
        if key.is_empty()
            || key.len() > 256
            || key.chars().any(char::is_control)
            || reference.len() != 1
            || reference
                .get("id")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
        {
            bail!("unsupported native binding reference");
        }
    }
    Ok(refs)
}
fn rule(pointer: String) -> Rule {
    Rule {
        pointer,
        kind: EdgeKind::Production,
        collection: Collection::ObjectValues,
        optional: false,
        binding: Binding::SnapshotSelected,
    }
}
impl Catalog {
    pub fn read(authored: &Map<String, Value>, language: &str) -> Result<Self> {
        let base = references(
            authored
                .get("binding_ids")
                .context("scene binding_ids missing")?,
        )?;
        let mut result = Self {
            effective: base.clone(),
            ids: base
                .values()
                .map(|r| r["id"].as_str().unwrap().to_owned())
                .collect(),
            languages: BTreeSet::new(),
            rules: vec![rule("/binding_ids".into())],
        };
        let Some(variants) = authored.get("language_binding_ids") else {
            return Ok(result);
        };
        let variants = variants
            .as_object()
            .context("language binding map must be an object")?;
        if variants.is_empty() || variants.len() > 32 {
            bail!("language binding count exceeds budget");
        }
        let mut count = base.len();
        for (tag, value) in variants {
            if tag.is_empty()
                || tag.len() > 32
                || !tag.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            {
                bail!("unsupported language binding tag");
            }
            let refs = references(value)?;
            count = count
                .checked_add(refs.len())
                .context("binding count overflow")?;
            if count > 4096 || refs.keys().any(|key| !base.contains_key(key)) {
                bail!("variant binding exceeds catalog keys or budget");
            }
            result.languages.insert(tag.clone());
            result
                .rules
                .push(rule(format!("/language_binding_ids/{tag}")));
            for (key, reference) in refs {
                result
                    .ids
                    .insert(reference["id"].as_str().unwrap().to_owned());
                if tag == language {
                    result.effective.insert(key.clone(), reference.clone());
                }
            }
        }
        Ok(result)
    }
}
