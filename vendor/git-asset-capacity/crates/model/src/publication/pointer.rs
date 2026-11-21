use super::{Error, Result};
use serde_json::Value;

pub(super) fn parse(text: &str) -> Result<Vec<String>> {
    if text.len() > 1024 || text.chars().any(char::is_control) {
        return Err(Error::Selector);
    }
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let tail = text.strip_prefix('/').ok_or(Error::Selector)?;
    let mut parts = Vec::new();
    for part in tail.split('/') {
        let mut decoded = String::new();
        let mut chars = part.chars();
        while let Some(c) = chars.next() {
            decoded.push(if c == '~' {
                match chars.next() {
                    Some('0') => '~',
                    Some('1') => '/',
                    _ => return Err(Error::Selector),
                }
            } else {
                c
            });
        }
        parts.push(decoded);
    }
    if parts.len() > 32 {
        return Err(Error::Selector);
    }
    Ok(parts)
}

pub(super) fn index(text: &str) -> Result<usize> {
    if text.is_empty()
        || (text.len() > 1 && text.starts_with('0'))
        || !text.bytes().all(|c| c.is_ascii_digit())
    {
        return Err(Error::Selector);
    }
    text.parse().map_err(|_| Error::Selector)
}

pub(super) fn resolve<'a>(mut value: &'a Value, parts: &[String]) -> Result<&'a Value> {
    for part in parts {
        value = match value {
            Value::Object(map) => map.get(part),
            Value::Array(items) => items.get(index(part)?),
            _ => None,
        }
        .ok_or(Error::Selector)?;
    }
    Ok(value)
}
