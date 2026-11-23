//! Pure proposal over explicit admitted metadata; no authority/provider is opened.
use super::{codec, model};
use serde::Deserialize;
use tig_core::retirement::review::{
    Review,
    routine_policy::{Policy, Run, compile},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    schema: String,
    policy: serde_json::Value,
    run: serde_json::Value,
    review: serde_json::Value,
}
pub fn propose(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() > model::MAX_INPUT {
        return Err("routine proposal input byte budget".into());
    }
    let input: Input = codec::decode(bytes)?;
    if input.schema != "tig.routine-policy-proposal-profile.experimental-1" {
        return Err("unsupported routine proposal profile".into());
    }
    let policy = Policy::from_json(&codec::encode(&input.policy)?)
        .map_err(|e| format!("routine policy refused ({e:?})"))?;
    let run = Run::from_json(&codec::encode(&input.run)?)
        .map_err(|e| format!("routine run scope refused ({e:?})"))?;
    let review = Review::from_json(&codec::encode(&input.review)?)
        .map_err(|_| "invalid exact routine review")?;
    compile(&policy, &run, &review)
        .map_err(|e| format!("routine proposal refused ({e:?})"))?
        .to_json()
        .map_err(|_| "cannot encode bounded routine proposal".into())
}
