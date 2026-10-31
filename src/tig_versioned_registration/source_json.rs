//! Bounded external evidence JSON, distinct from native canonical identity JSON.
use anyhow::{Result, bail};
use serde::{Deserialize, Deserializer, de};
use serde_json::Value;

struct Source(Value);

impl<'de> Deserialize<'de> for Source {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> de::Visitor<'de> for Visitor {
            type Value = Source;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("external JSON without duplicate keys")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Source, E> {
                Ok(Source(v.into()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Source, E> {
                Ok(Source(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Source, E> {
                Ok(Source(v.into()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Source, E> {
                serde_json::Number::from_f64(v)
                    .map(|n| Source(Value::Number(n)))
                    .ok_or_else(|| E::custom("non-finite source number"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Source, E> {
                Ok(Source(v.into()))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Source, E> {
                Ok(Source(Value::Null))
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Source, A::Error> {
                let mut out = Vec::new();
                while let Some(Source(v)) = seq.next_element()? {
                    out.push(v);
                }
                Ok(Source(out.into()))
            }
            fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Source, A::Error> {
                let mut out = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if out.contains_key(&key) {
                        return Err(de::Error::custom("duplicate source key"));
                    }
                    let Source(v) = map.next_value()?;
                    out.insert(key, v);
                }
                Ok(Source(Value::Object(out)))
            }
        }
        d.deserialize_any(Visitor)
    }
}

pub(super) fn parse(bytes: &[u8]) -> Result<Value> {
    if bytes.len() > super::LIMIT {
        bail!("source JSON byte budget exceeded");
    }
    let Source(value) = serde_json::from_slice(bytes)?;
    fn bound(value: &Value, depth: usize, nodes: &mut usize) -> Result<()> {
        *nodes += 1;
        if depth > 32 || *nodes > 4096 {
            bail!("source JSON structural budget exceeded");
        }
        match value {
            Value::Array(items) => {
                for item in items {
                    bound(item, depth + 1, nodes)?;
                }
            }
            Value::Object(items) => {
                for item in items.values() {
                    bound(item, depth + 1, nodes)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    bound(&value, 0, &mut 0)?;
    Ok(value)
}
