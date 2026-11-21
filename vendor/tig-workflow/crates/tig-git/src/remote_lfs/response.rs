use super::*;
use serde_json::Value;
use ureq::http::{HeaderName, HeaderValue, Uri};
pub(super) struct Action {
    pub url: Uri,
    pub headers: Vec<(HeaderName, HeaderValue)>,
    pub deadline: Instant,
}
fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, SourceError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or(SourceError::Invalid)
}
fn expiry(config: &Config, action: &Value, started: Instant) -> Result<Instant, SourceError> {
    let limit = config.limits.action_ttl;
    let duration = if let Some(seconds) = action.get("expires_in") {
        let seconds = seconds
            .as_i64()
            .filter(|s| *s <= i32::MAX as i64)
            .ok_or(SourceError::Invalid)?;
        if seconds <= 0 {
            return Err(SourceError::Stale);
        }
        Duration::from_secs(seconds as u64).min(limit)
    } else if let Some(at) = action.get("expires_at") {
        let at = at.as_str().ok_or(SourceError::Invalid)?;
        let at = time::OffsetDateTime::parse(at, &time::format_description::well_known::Rfc3339)
            .map_err(|_| SourceError::Invalid)?
            .unix_timestamp_nanos();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| SourceError::Unknown)?
            .as_nanos();
        let remaining = at
            .checked_sub(i128::try_from(now).map_err(|_| SourceError::Unknown)?)
            .filter(|n| *n > 0)
            .ok_or(SourceError::Stale)?;
        Duration::from_nanos(
            u64::try_from(remaining.min(limit.as_nanos() as i128))
                .map_err(|_| SourceError::Invalid)?,
        )
    } else {
        limit
    };
    let cap = started.checked_add(limit).ok_or(SourceError::Invalid)?;
    let deadline = if action.get("expires_in").is_some() {
        started.checked_add(duration)
    } else {
        Instant::now().checked_add(duration)
    }
    .ok_or(SourceError::Invalid)?
    .min(cap);
    if Instant::now() >= deadline {
        return Err(SourceError::Stale);
    }
    Ok(deadline)
}
pub(super) fn parse(
    config: &Config,
    object: &Object,
    bytes: &[u8],
    started: Instant,
) -> Result<Action, SourceError> {
    let value = tig_core::identity::parse_json(bytes).map_err(|_| SourceError::Invalid)?;
    if value
        .get("transfer")
        .is_some_and(|v| v.as_str() != Some("basic"))
        || value
            .get("hash_algo")
            .is_some_and(|v| v.as_str() != Some("sha256"))
    {
        return Err(SourceError::Unsupported);
    }
    let objects = value
        .get("objects")
        .and_then(Value::as_array)
        .filter(|v| v.len() == 1)
        .ok_or(SourceError::Invalid)?;
    let value = &objects[0];
    if string(value, "oid")? != object.sha256.as_str()
        || value.get("size").and_then(Value::as_u64) != Some(object.bytes)
    {
        return Err(SourceError::Integrity);
    }
    if let Some(error) = value.get("error") {
        if value.get("actions").is_some() {
            return Err(SourceError::Invalid);
        }
        return match error.get("code").and_then(Value::as_u64) {
            Some(401 | 403) => Err(SourceError::Denied),
            Some(404 | 410) => Err(SourceError::Unavailable),
            _ => Err(SourceError::Unknown),
        };
    }
    let authenticated = match value.get("authenticated") {
        None => false,
        Some(v) => v.as_bool().ok_or(SourceError::Invalid)?,
    };
    let action = value
        .get("actions")
        .and_then(|v| v.get("download"))
        .filter(|v| v.is_object())
        .ok_or(SourceError::Invalid)?;
    let (url, origin) = config::target(string(action, "href")?, config.loopback)?;
    if !config.origins.contains(&origin) {
        return Err(SourceError::Denied);
    }
    let deadline = expiry(config, action, started)?;
    let mut headers = action_headers(config, action)?;
    if !authenticated
        && !headers
            .iter()
            .any(|(n, _)| n == ureq::http::header::AUTHORIZATION)
        && let Some(secret) = config.download_auth.get(&origin)
    {
        headers.push((ureq::http::header::AUTHORIZATION, secret.0.clone()));
    }
    Ok(Action {
        url,
        headers,
        deadline,
    })
}
fn action_headers(
    config: &Config,
    action: &Value,
) -> Result<Vec<(HeaderName, HeaderValue)>, SourceError> {
    let mut headers = Vec::new();
    if let Some(map) = action.get("header") {
        let map = map
            .as_object()
            .filter(|m| m.len() <= 16)
            .ok_or(SourceError::Budget)?;
        let mut used = 0usize;
        let mut names = std::collections::BTreeSet::new();
        for (name, value) in map {
            let name = config::allowed_header(name)?;
            if !names.insert(name.clone()) {
                return Err(SourceError::Invalid);
            }
            if !config.headers.contains(&name) {
                return Err(SourceError::Denied);
            }
            let value = value.as_str().ok_or(SourceError::Invalid)?;
            used = used
                .checked_add(name.len() + value.len())
                .filter(|n| *n <= 16_384)
                .ok_or(SourceError::Budget)?;
            let value = Secret::new(value)?;
            headers.push((
                HeaderName::from_bytes(name.as_bytes()).map_err(|_| SourceError::Invalid)?,
                value.0,
            ));
        }
    }
    Ok(headers)
}
