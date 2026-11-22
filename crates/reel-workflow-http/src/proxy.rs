//! Pure workflow proxy wire contract. No network, storage admission or approval.
use crate::error;
use base64::{Engine, engine::general_purpose::STANDARD};
use reel_workflow::{
    Artifact, ErrorKind, Mode, Operation, Packet, Provenance, Result, ReturnedResult,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub packet: Packet,
    pub attempt_id: String,
    pub provider: String,
    pub model: String,
}
impl Spec {
    pub fn check(&self) -> Result<()> {
        reel_workflow::check_packet(&self.packet)?;
        if !name(&self.attempt_id) || !name(&self.provider) || !name(&self.model) {
            return Err(error(ErrorKind::Invalid, "proxy-context"));
        }
        if matches!(
            self.packet.task.operation,
            Operation::DeterministicTool
                | Operation::ExternalSourceIntake
                | Operation::SpeechCandidate
        ) {
            return Err(error(
                ErrorKind::Unsupported,
                "proxy-operation-adapter-required",
            ));
        }
        let mut names = BTreeSet::new();
        for name in self.packet.task.outputs.keys() {
            if !filename(name) || !names.insert(name.to_ascii_lowercase()) {
                return Err(error(ErrorKind::Invalid, "proxy-flat-output-names"));
            }
        }
        Ok(())
    }
    /// Independent of provider/config/body: changing those cannot silently make
    /// a second call under the same task attempt in the original registry.
    pub fn operation_id(&self) -> Result<String> {
        self.check()?;
        reel_workflow::canonical_digest(
            &serde_json::json!({"schema":"reel.workflow-proxy-operation.v1","task_key":self.packet.task_key,"attempt_id":self.attempt_id}),
        )
    }
    pub fn compose(&self, inputs: &BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>> {
        self.check()?;
        let mut encoded = BTreeMap::new();
        let mut total = 0u64;
        for (id, bytes) in inputs {
            let asset = self
                .packet
                .task
                .inputs
                .iter()
                .find(|a| &a.logical_id == id)
                .ok_or_else(|| error(ErrorKind::Denied, "proxy-undeclared-input"))?;
            if asset.bytes != bytes.len() as u64 || asset.sha256 != reel_workflow::digest(bytes) {
                return Err(error(ErrorKind::Integrity, "proxy-input-content"));
            }
            total = total
                .checked_add(asset.bytes)
                .filter(|n| *n <= crate::MAX_BYTES)
                .ok_or_else(|| error(ErrorKind::Budget, "proxy-input-bytes"))?;
            encoded.insert(
                id,
                serde_json::json!({"artifact":asset,"base64":STANDARD.encode(bytes)}),
            );
        }
        serde_json::to_vec(&serde_json::json!({"schema":"reel.workflow-proxy-request.v1","operation_id":self.operation_id()?,"provider":self.provider,"model":self.model,"attempt_id":self.attempt_id,"packet":self.packet,"inputs":encoded}))
            .map_err(|_| error(ErrorKind::Invalid, "proxy-request-json"))
    }
    pub fn normalize(&self, response: &[u8]) -> Result<Candidate> {
        self.check()?;
        if response.len() as u64 > crate::MAX_BYTES {
            return Err(error(ErrorKind::Budget, "proxy-response-bytes"));
        }
        let reply: Reply = serde_json::from_slice(response)
            .map_err(|_| error(ErrorKind::Integrity, "proxy-response-json"))?;
        if reply.schema != "reel.workflow-proxy-response.v1"
            || reply.operation_id != self.operation_id()?
            || reply.provider != self.provider
            || reply.model != self.model
        {
            return Err(error(ErrorKind::Conflict, "proxy-response-context"));
        }
        if reply
            .outputs
            .keys()
            .any(|n| !self.packet.task.outputs.contains_key(n))
            || self
                .packet
                .task
                .outputs
                .iter()
                .any(|(n, c)| c.required && !reply.outputs.contains_key(n))
        {
            return Err(error(ErrorKind::Conflict, "proxy-output-set"));
        }
        let mut files = BTreeMap::new();
        let mut artifacts = BTreeMap::new();
        let mut total = 0u64;
        for (name, encoded) in reply.outputs {
            let contract = &self.packet.task.outputs[&name];
            if encoded.media_type != contract.media_type {
                return Err(error(ErrorKind::Conflict, "proxy-output-media-type"));
            }
            if encoded.base64.len() as u64
                > contract
                    .max_bytes
                    .min(crate::MAX_BYTES)
                    .div_ceil(3)
                    .saturating_mul(4)
            {
                return Err(error(ErrorKind::Budget, "proxy-output-contract"));
            }
            let bytes = STANDARD
                .decode(encoded.base64)
                .map_err(|_| error(ErrorKind::Integrity, "proxy-output-base64"))?;
            total = total
                .checked_add(bytes.len() as u64)
                .filter(|n| *n <= crate::MAX_BYTES)
                .ok_or_else(|| error(ErrorKind::Budget, "proxy-output-bytes"))?;
            if bytes.len() as u64 > contract.max_bytes {
                return Err(error(ErrorKind::Budget, "proxy-output-bytes"));
            }
            artifacts.insert(
                name.clone(),
                Artifact {
                    logical_id: format!("workflow/provider/{}/{}", self.operation_id()?, name),
                    sha256: reel_workflow::digest(&bytes),
                    bytes: bytes.len() as u64,
                    media_type: encoded.media_type,
                },
            );
            files.insert(name, bytes);
        }
        let result = ReturnedResult {
            schema: reel_workflow::RESULT_SCHEMA.into(),
            task_id: self.packet.task.id.clone(),
            task_key: self.packet.task_key.clone(),
            attempt_id: self.attempt_id.clone(),
            artifacts,
            provenance: Provenance {
                mode: Mode::Direct,
                provider: Some(self.provider.clone()),
                model: Some(self.model.clone()),
                executable: None,
            },
            worker_receipt: None,
        };
        reel_workflow::check_result(&self.packet, &result)?;
        Ok(Candidate { result, files })
    }
}
pub struct Candidate {
    pub result: ReturnedResult,
    pub files: BTreeMap<String, Vec<u8>>,
}
fn name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s != "."
        && s != ".."
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
fn filename(s: &str) -> bool {
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    name(s)
        && !s.ends_with('.')
        && !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Encoded {
    media_type: String,
    base64: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    schema: String,
    operation_id: String,
    provider: String,
    model: String,
    #[serde(deserialize_with = "unique_outputs")]
    outputs: BTreeMap<String, Encoded>,
}
fn unique_outputs<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<BTreeMap<String, Encoded>, D::Error> {
    struct Unique;
    impl<'de> serde::de::Visitor<'de> for Unique {
        type Value = BTreeMap<String, Encoded>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("bounded unique output names")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut map: M,
        ) -> std::result::Result<Self::Value, M::Error> {
            let mut outputs = BTreeMap::new();
            while let Some((key, value)) = map.next_entry::<String, Encoded>()? {
                if outputs.len() >= 128 || outputs.insert(key, value).is_some() {
                    return Err(serde::de::Error::custom("duplicate or excessive outputs"));
                }
            }
            Ok(outputs)
        }
    }
    deserializer.deserialize_map(Unique)
}
