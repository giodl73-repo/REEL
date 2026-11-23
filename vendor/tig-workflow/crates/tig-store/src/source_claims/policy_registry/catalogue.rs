use super::*;

const SCHEMA: &str = "tig.native-policy-provider-catalogue.experimental-1";

/// Owner-admitted domain mapping, not measured failure independence or free
/// capacity. A native executor must also check its actual capacity binding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaceScope {
    pub capacity_domain: Digest,
    pub failure_domain: Key,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProviderCatalogue {
    schema: &'static str,
    profiles: BTreeMap<Digest, Profile>,
    places: BTreeMap<Digest, PlaceScope>,
}
impl ProviderCatalogue {
    pub fn new(
        profiles: BTreeMap<Digest, Profile>,
        places: BTreeMap<Digest, PlaceScope>,
    ) -> RegistryResult<Self> {
        if profiles.is_empty() || profiles.len() > 16 || places.is_empty() || places.len() > 16 {
            return Err(RegistryError::Scope);
        }
        for (id, p) in &profiles {
            if p.fingerprint().map_err(|_| RegistryError::Encoding)? != *id
                || !places.contains_key(&p.binding.place)
            {
                return Err(RegistryError::Scope);
            }
        }
        let value = Self {
            schema: SCHEMA,
            profiles,
            places,
        };
        value.to_json()?;
        Ok(value)
    }
    pub fn profiles(&self) -> &BTreeMap<Digest, Profile> {
        &self.profiles
    }
    pub fn places(&self) -> &BTreeMap<Digest, PlaceScope> {
        &self.places
    }
    pub fn to_json(&self) -> RegistryResult<Vec<u8>> {
        let bytes = encode(self).map_err(RegistryError::Journal)?;
        if bytes.len() > 65_536 {
            return Err(RegistryError::Encoding);
        }
        Ok(bytes)
    }
    pub fn from_json(bytes: &[u8]) -> RegistryResult<Self> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Record {
            schema: String,
            profiles: BTreeMap<Digest, Profile>,
            places: BTreeMap<Digest, PlaceScope>,
        }
        let record: Record = serde_json::from_value(
            identity::parse_json(bytes).map_err(|_| RegistryError::Encoding)?,
        )
        .map_err(|_| RegistryError::Encoding)?;
        if record.schema != SCHEMA {
            return Err(RegistryError::Encoding);
        }
        let value = Self::new(record.profiles, record.places)?;
        if value.to_json()? != bytes {
            return Err(RegistryError::Encoding);
        }
        Ok(value)
    }
}
