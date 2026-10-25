//! Metadata-only score choices from native episode roles and selected scopes.
use crate::Result;
use reel_assembly::scene_authoring as native;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Serialize)]
pub struct Choice {
    pub role: String,
    pub asset_binding: String,
    pub asset: native::AssetRef,
}
#[derive(Serialize)]
pub struct HeldRole {
    pub role: String,
    pub reason: String,
}
#[derive(Default, Serialize)]
pub struct Palette {
    pub choices: Vec<Choice>,
    pub held_roles: Vec<HeldRole>,
}

/// Missing legacy palettes provide no choices. A declared palette must use the
/// native grammar. Unavailable bindings are held, never guessed or promoted.
pub fn choices(docs: &BTreeMap<String, Value>) -> Result<Palette> {
    let Some(value) = docs
        .get("episode.json")
        .and_then(|e| e.get("score_palette"))
    else {
        return Ok(Palette::default());
    };
    let palette: Vec<native::ScoreBinding> = serde_json::from_value(value.clone())?;
    if palette.len() > 128 {
        return Err("Score palette exceeds 128 roles".into());
    }
    let roles = native::score_palette_roles(&palette)?;
    if roles.is_empty() {
        return Ok(Palette::default());
    }
    let episode: native::Episode = serde_json::from_value(docs["episode.json"].clone())?;
    if episode.schema != native::EPISODE_SCHEMA
        || !matches!(
            episode.authoring_state.as_str(),
            "ready-for-private-build" | "scene-build-context"
        )
        || docs["scene.json"]["episode_id"].as_str() != Some(episode.episode_id.as_str())
        || !episode
            .scene_ids
            .iter()
            .any(|id| docs["scene.json"]["scene_id"].as_str() == Some(id.as_str()))
    {
        return Err("Score palette is outside the native episode build context".into());
    }
    let mut scopes = Vec::new();
    for (file, expected) in [
        ("scene-bindings.json", &docs["scene.json"]["scene_id"]),
        ("episode-bindings.json", &docs["episode.json"]["episode_id"]),
        ("season-bindings.json", &docs["episode.json"]["season_id"]),
    ] {
        let scope: native::ScopedBindings =
            serde_json::from_value(docs.get(file).ok_or("Missing score scope")?.clone())?;
        if scope.schema != native::BINDINGS_SCHEMA
            || expected.as_str() != Some(scope.scope_id.as_str())
        {
            return Err("Score bindings are outside native scene/episode/season scope".into());
        }
        scopes.push(scope);
    }
    let refs = scopes.iter().collect::<Vec<_>>();
    let mut result = Palette::default();
    for (role, key) in roles {
        if role.is_empty()
            || role.trim() != role
            || role.len() > 128
            || role.chars().any(char::is_control)
        {
            return Err("Invalid browser score role identity".into());
        }
        if key.trim().is_empty() || key.len() > 256 {
            result.held_roles.push(HeldRole {
                role,
                reason: "Score binding exceeds the browser identity budget".into(),
            });
            continue;
        }
        match native::resolve_selected_binding(&key, &refs) {
            Ok(asset)
                if asset.bytes <= 9_007_199_254_740_991
                    && !asset.logical_id.trim().is_empty()
                    && asset.logical_id.len() <= 256 =>
            {
                result.choices.push(Choice {
                    role,
                    asset_binding: key,
                    asset: asset.clone(),
                })
            }
            Ok(_) => result.held_roles.push(HeldRole {
                role,
                reason: "Asset exceeds the browser identity or integer budget".into(),
            }),
            Err(error) => result.held_roles.push(HeldRole {
                role,
                reason: error.to_string(),
            }),
        }
    }
    Ok(result)
}

/// Revalidate caller intent against locked controls, independently of the UI.
pub fn validate(docs: &BTreeMap<String, Value>, score: &native::ScoreUse) -> Result<()> {
    if let native::ScoreUse::Role { role } = score
        && !choices(docs)?
            .choices
            .iter()
            .any(|choice| &choice.role == role)
    {
        return Err("Score role lacks a unique selected native palette binding".into());
    }
    Ok(())
}
