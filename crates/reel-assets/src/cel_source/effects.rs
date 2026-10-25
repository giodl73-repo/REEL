//! Explicit native effect bindings, never inferred from media or another lane.
use super::{project::Project, sequence::Sequence};
use crate::Result;
use reel_assembly::scene_authoring as native;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Serialize)]
pub struct Choice {
    pub binding: String,
    pub asset: native::AssetRef,
}
#[derive(Serialize)]
pub struct Held {
    pub binding: String,
    pub reason: String,
}
#[derive(Default, Serialize)]
pub struct Palette {
    pub choices: Vec<Choice>,
    pub held: Vec<Held>,
}
#[derive(Default, Serialize)]
pub struct Effects {
    pub sonic: Palette,
    pub vfx: Palette,
}

fn scopes(docs: &BTreeMap<String, Value>) -> Result<Vec<native::ScopedBindings>> {
    let scene = docs.get("scene.json").ok_or("Missing native scene")?;
    let first: native::ScopedBindings = serde_json::from_value(
        docs.get("scene-bindings.json")
            .ok_or("Missing native scene bindings")?
            .clone(),
    )?;
    if first.schema != native::BINDINGS_SCHEMA
        || scene["scene_id"].as_str() != Some(first.scope_id.as_str())
    {
        return Err("Effect scene bindings are out of scope".into());
    }
    let mut result = vec![first];
    for (role, id) in [
        ("episode-bindings.json", "episode_id"),
        ("season-bindings.json", "season_id"),
    ] {
        let Some(value) = docs.get(role) else {
            continue;
        };
        // Opaque legacy controls are not declarations of additional scopes.
        if value.get("schema").is_none() && value.get("assets").is_none() {
            continue;
        }
        let episode = super::score::episode_context(docs)?;
        let expected = if id == "episode_id" {
            &episode.episode_id
        } else {
            &episode.season_id
        };
        let scope: native::ScopedBindings = serde_json::from_value(value.clone())?;
        if scope.schema != native::BINDINGS_SCHEMA || &scope.scope_id != expected {
            return Err("Effect bindings are outside native episode/season scope".into());
        }
        result.push(scope);
    }
    Ok(result)
}
fn palette(keys: BTreeSet<String>, scopes: &[native::ScopedBindings]) -> Result<Palette> {
    if keys.len() > 128 {
        return Err("Effect choices exceed 128 bindings per kind".into());
    }
    let refs = scopes.iter().collect::<Vec<_>>();
    let mut result = Palette::default();
    for key in keys {
        if key.trim() != key
            || key.is_empty()
            || key.len() > 256
            || key.chars().any(char::is_control)
        {
            return Err("Invalid effect binding identity".into());
        }
        match native::resolve_selected_binding(&key, &refs) {
            Ok(asset)
                if asset.bytes <= 9_007_199_254_740_991
                    && asset.logical_id.len() <= 256
                    && !asset.logical_id.trim().is_empty() =>
            {
                result.choices.push(Choice {
                    binding: key,
                    asset: asset.clone(),
                })
            }
            Ok(_) => result.held.push(Held {
                binding: key,
                reason: "Effect asset exceeds browser identity/integer budget".into(),
            }),
            Err(e) => result.held.push(Held {
                binding: key,
                reason: e.to_string(),
            }),
        }
    }
    Ok(result)
}
pub fn choices(
    docs: &BTreeMap<String, Value>,
    project: &Project,
    sequence: &Sequence,
) -> Result<Effects> {
    let scene: native::Scene = serde_json::from_value(
        docs.get("scene.json")
            .ok_or("Missing native scene")?
            .clone(),
    )?;
    let materialized = native::materialize_scene(&scene)?;
    let lane = materialized
        .languages
        .get(&sequence.language)
        .ok_or("Missing effect language scope")?;
    let mut sonic = project
        .sonic_candidates()
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut vfx = project
        .vfx_candidates()
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    for event in &lane.events {
        sonic.extend(event.sonic_bindings.iter().cloned());
        vfx.extend(event.vfx_bindings.iter().cloned());
    }
    if sonic.is_empty() && vfx.is_empty() {
        return Ok(Effects::default());
    }
    let scopes = scopes(docs)?;
    Ok(Effects {
        sonic: palette(sonic, &scopes)?,
        vfx: palette(vfx, &scopes)?,
    })
}
pub fn validate(
    docs: &BTreeMap<String, Value>,
    project: &Project,
    sequence: &Sequence,
    shared: &native::SharedEvent,
    local: &native::LanguageEventBinding,
) -> Result<()> {
    if shared.sonic_bindings.is_empty()
        && shared.vfx_bindings.is_empty()
        && local
            .sonic_bindings_override
            .as_ref()
            .is_none_or(Vec::is_empty)
        && local
            .vfx_bindings_override
            .as_ref()
            .is_none_or(Vec::is_empty)
    {
        return Ok(());
    }
    let choices = choices(docs, project, sequence)?;
    for (keys, palette) in [
        (&shared.sonic_bindings, &choices.sonic),
        (&shared.vfx_bindings, &choices.vfx),
    ]
    .into_iter()
    .chain(
        local
            .sonic_bindings_override
            .as_ref()
            .map(|k| (k, &choices.sonic)),
    )
    .chain(
        local
            .vfx_bindings_override
            .as_ref()
            .map(|k| (k, &choices.vfx)),
    ) {
        if keys.len() > 128
            || keys.iter().collect::<BTreeSet<_>>().len() != keys.len()
            || keys
                .iter()
                .any(|k| !palette.choices.iter().any(|c| &c.binding == k))
        {
            return Err("Effect selection lacks unique admitted native bindings".into());
        }
    }
    Ok(())
}
