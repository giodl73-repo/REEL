//! Caller-admitted scope precedence, lowest ancestor first, local aliases last.
use super::*;

pub(super) fn merge(
    ancestors: &[BindingSource],
    package: &Package,
    local: &mut Bindings,
    source: &mut impl ByteSource,
    remaining: &mut u64,
) -> Result<Vec<Value>> {
    if ancestors.len() > 8 {
        bail!("binding ancestor count exceeds budget");
    }
    let mut assets = BTreeMap::new();
    let mut provenance = Vec::new();
    for ancestor in ancestors {
        if ancestor.scope_id.is_empty() || ancestor.scope_id.len() > 256 {
            bail!("invalid binding ancestor scope");
        }
        let pin = package
            .assets
            .get(&ancestor.asset)
            .context("binding ancestor package role missing")?
            .object()?;
        let bindings: Bindings = serde_json::from_slice(&read_metadata(source, &pin, remaining)?)?;
        if bindings.schema != scene_authoring::BINDINGS_SCHEMA
            || bindings.scope_id != ancestor.scope_id
        {
            bail!("binding ancestor scope differs");
        }
        assets.extend(bindings.assets);
        if assets.len() > 4096 {
            bail!("inherited binding count exceeds budget");
        }
        provenance.push(json!({"asset":ancestor.asset,"scope_id":ancestor.scope_id,"object":pin}));
    }
    if !ancestors.is_empty() {
        assets.extend(std::mem::take(&mut local.assets));
        if assets.len() > 4096 {
            bail!("combined binding count exceeds budget");
        }
        local.assets = assets;
    }
    Ok(provenance)
}
