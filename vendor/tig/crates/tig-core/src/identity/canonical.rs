use super::{Error, Result, require};
use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use std::{collections::BTreeMap, fmt};

pub const MAX_BYTES: usize = 65_536;
const MAX_INTEGER: u64 = 9_007_199_254_740_991;

/// Lowercase SHA-256 identity. Domain separation belongs to the hashing operation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct Digest(String);

impl Digest {
    pub fn parse(text: &str) -> Result<Self> {
        require(
            text.len() == 64
                && text
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "expected lowercase SHA-256",
        )?;
        Ok(Self(text.into()))
    }

    /// Exact blob bytes; deliberately distinct from every ART semantic hash.
    pub fn blob(bytes: &[u8]) -> Self {
        Self(hex(&Sha256::digest(bytes)))
    }

    /// Formats a completed raw SHA-256 result; does not verify payload custody.
    pub fn from_sha256(bytes: [u8; 32]) -> Self {
        Self(hex(&bytes))
    }

    /// Legacy ART prototype framing (preserved for compatibility): prefix, domain byte length, domain, payload length, payload.
    pub fn semantic(domain: &str, value: &Value) -> Result<Self> {
        require(
            !domain.is_empty() && domain.len() <= 128 && domain.is_ascii(),
            "invalid hash domain",
        )?;
        let bytes = canonical_json(value)?;
        let mut hash = Sha256::new();
        hash.update(b"ART\0prototype-1\0");
        hash.update((domain.len() as u64).to_be_bytes());
        hash.update(domain.as_bytes());
        hash.update((bytes.len() as u64).to_be_bytes());
        hash.update(bytes);
        Ok(Self(hex(&hash.finalize())))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(DIGITS[(byte >> 4) as usize] as char);
        text.push(DIGITS[(byte & 15) as usize] as char);
    }
    text
}

impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Self::parse(&String::deserialize(d)?).map_err(de::Error::custom)
    }
}

struct Strict(Value);

impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct Visitor;
        impl<'de> de::Visitor<'de> for Visitor {
            type Value = Strict;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("bounded ART JSON")
            }
            fn visit_unit<E: de::Error>(self) -> std::result::Result<Strict, E> {
                Ok(Strict(Value::Null))
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<Strict, E> {
                if v.unsigned_abs() > MAX_INTEGER {
                    return Err(E::custom("unsafe integer"));
                }
                Ok(Strict(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<Strict, E> {
                if v > MAX_INTEGER {
                    return Err(E::custom("unsafe integer"));
                }
                Ok(Strict(v.into()))
            }
            fn visit_f64<E: de::Error>(self, _: f64) -> std::result::Result<Strict, E> {
                Err(E::custom(
                    "floating, exponent and negative-zero numbers unsupported",
                ))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_seq<A: de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Strict, A::Error> {
                let mut out = Vec::new();
                while let Some(Strict(v)) = seq.next_element()? {
                    out.push(v);
                }
                Ok(Strict(out.into()))
            }
            fn visit_map<A: de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Strict, A::Error> {
                let mut out = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if out.contains_key(&key) {
                        return Err(de::Error::custom("duplicate object key"));
                    }
                    let Strict(value) = map.next_value()?;
                    out.insert(key, value);
                }
                Ok(Strict(Value::Object(out)))
            }
        }
        d.deserialize_any(Visitor)
    }
}

/// Reject duplicate keys before normalization. Does not read files or resolve blobs.
pub fn parse_json(bytes: &[u8]) -> Result<Value> {
    require(bytes.len() <= MAX_BYTES, "JSON byte budget exceeded")?;
    let Strict(value) = serde_json::from_slice(bytes).map_err(|e| Error(e.to_string()))?;
    canonical_json(&value)?;
    Ok(value)
}

/// Prototype subset, not RFC 8785: UTF-8 key order, safe integers, no Unicode normalization.
pub fn canonical_json(value: &Value) -> Result<Vec<u8>> {
    fn walk(v: &Value, depth: usize, nodes: &mut usize, out: &mut Vec<u8>) -> Result<()> {
        *nodes += 1;
        require(
            depth <= 32 && *nodes <= 4096,
            "JSON structural budget exceeded",
        )?;
        match v {
            Value::Object(map) => {
                out.push(b'{');
                for (i, (key, value)) in map
                    .iter()
                    .collect::<BTreeMap<_, _>>()
                    .into_iter()
                    .enumerate()
                {
                    if i != 0 {
                        out.push(b',');
                    }
                    out.extend(serde_json::to_vec(key).map_err(|e| Error(e.to_string()))?);
                    out.push(b':');
                    walk(value, depth + 1, nodes, out)?;
                }
                out.push(b'}');
            }
            Value::Array(array) => {
                out.push(b'[');
                for (i, value) in array.iter().enumerate() {
                    if i != 0 {
                        out.push(b',');
                    }
                    walk(value, depth + 1, nodes, out)?;
                }
                out.push(b']');
            }
            Value::Number(n) => {
                let valid = n.as_u64().is_some_and(|n| n <= MAX_INTEGER)
                    || n.as_i64().is_some_and(|n| n.unsigned_abs() <= MAX_INTEGER);
                require(valid, "only safe integers supported")?;
                out.extend(n.to_string().bytes());
            }
            _ => out.extend(serde_json::to_vec(v).map_err(|e| Error(e.to_string()))?),
        }
        require(out.len() <= MAX_BYTES, "canonical byte budget exceeded")
    }
    let mut out = Vec::new();
    walk(value, 0, &mut 0, &mut out)?;
    Ok(out)
}
