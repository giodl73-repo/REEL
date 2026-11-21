//! External evidence is never rewritten as native identity JSON.
use super::{Error, Limits, Result};
use serde::de::{self, DeserializeSeed};
use serde_json::Value;

struct Budget {
    remaining: usize,
    depth: usize,
    max_seen: usize,
}
struct Seed<'a> {
    budget: &'a mut Budget,
    depth: usize,
}
impl<'de> DeserializeSeed<'de> for Seed<'_> {
    type Value = Value;
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> std::result::Result<Value, D::Error> {
        if self.depth > self.budget.depth || self.budget.remaining == 0 {
            return Err(de::Error::custom("evidence structural budget"));
        }
        self.budget.remaining -= 1;
        self.budget.max_seen = self.budget.max_seen.max(self.depth);
        d.deserialize_any(self)
    }
}
impl<'de> de::Visitor<'de> for Seed<'_> {
    type Value = Value;
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("bounded unambiguous evidence JSON")
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> std::result::Result<Value, E> {
        Ok(v.into())
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<Value, E> {
        Ok(v.into())
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<Value, E> {
        Ok(v.into())
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> std::result::Result<Value, E> {
        serde_json::Number::from_f64(v)
            .map(Value::Number)
            .ok_or_else(|| E::custom("non-finite evidence number"))
    }
    fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Value, E> {
        Ok(v.into())
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Value, A::Error> {
        let mut out = Vec::new();
        while let Some(value) = seq.next_element_seed(Seed {
            budget: self.budget,
            depth: self.depth + 1,
        })? {
            out.push(value);
        }
        Ok(Value::Array(out))
    }
    fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> std::result::Result<Value, A::Error> {
        let mut out = serde_json::Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if out.contains_key(&key) {
                return Err(de::Error::custom("duplicate evidence key"));
            }
            let value = map.next_value_seed(Seed {
                budget: self.budget,
                depth: self.depth + 1,
            })?;
            out.insert(key, value);
        }
        Ok(Value::Object(out))
    }
}

pub(super) fn parse(bytes: &[u8], limits: &Limits) -> Result<(Value, usize, usize)> {
    if bytes.len() > limits.bytes {
        return Err(Error::Budget);
    }
    let mut budget = Budget {
        remaining: limits.nodes,
        depth: limits.depth,
        max_seen: 0,
    };
    let mut d = serde_json::Deserializer::from_slice(bytes);
    let value = Seed {
        budget: &mut budget,
        depth: 0,
    }
    .deserialize(&mut d)
    .map_err(|_| Error::Evidence)?;
    d.end().map_err(|_| Error::Evidence)?;
    Ok((value, limits.nodes - budget.remaining, budget.max_seen))
}
