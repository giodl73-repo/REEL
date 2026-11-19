//! Images API JSON generation/edit composition. No network, URL acquisition,
//! asset selection or creative approval follows from these checks.
use crate::{Config, error};
use base64::{Engine, engine::general_purpose::STANDARD};
use reel_workflow::{Artifact, ErrorKind, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const OUTPUTS: [(&str, &str); 2] = [
    ("candidate.png", "image/png"),
    ("image.json", "application/json"),
];
use crate::raster::{MAX_IMAGE, inspect_png as png_image};
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub schema: String,
    pub references: Vec<String>,
    pub mask: Option<String>,
    pub size: [u32; 2],
    pub quality: String,
    pub background: String,
}
pub struct Input<'a> {
    pub artifact: &'a Artifact,
    pub bytes: &'a [u8],
}
impl Spec {
    pub fn egress_inputs(&self) -> BTreeSet<String> {
        self.references
            .iter()
            .chain(self.mask.iter())
            .cloned()
            .collect()
    }
    pub fn check(&self, model: &str) -> Result<()> {
        if self.schema != "reel.openai-image.v1"
            || !model.starts_with("gpt-image-")
            || model.len() > 128
            || !model
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            || self.references.len() > 16
            || self.references.iter().collect::<BTreeSet<_>>().len() != self.references.len()
            || self
                .egress_inputs()
                .iter()
                .any(|id| id.is_empty() || id.len() > 512 || id.chars().any(char::is_control))
            || (self.mask.is_some() && self.references.is_empty())
            || self
                .size
                .iter()
                .any(|n| !(16..=4096).contains(n) || n % 16 != 0)
            || u64::from(self.size[0]) * u64::from(self.size[1]) > 8_294_400
            || self.size[0] > self.size[1] * 3
            || self.size[1] > self.size[0] * 3
            || !["auto", "low", "medium", "high"].contains(&self.quality.as_str())
            || !["auto", "opaque", "transparent"].contains(&self.background.as_str())
        {
            return Err(error(ErrorKind::Invalid, "openai-image-spec"));
        }
        Ok(())
    }
    pub fn check_endpoint(&self, http: &Config) -> Result<()> {
        let url = reqwest::Url::parse(&http.endpoint)
            .map_err(|_| error(ErrorKind::Invalid, "openai-image-endpoint"))?;
        let local = http.allow_loopback_http
            && url.scheme() == "http"
            && url
                .host_str()
                .and_then(|h| h.trim_matches(['[', ']']).parse::<std::net::IpAddr>().ok())
                .is_some_and(|ip| ip.is_loopback());
        let path = if self.references.is_empty() {
            "/v1/images/generations"
        } else {
            "/v1/images/edits"
        };
        if (!local
            && (url.scheme() != "https"
                || url.host_str() != Some("api.openai.com")
                || url.port_or_known_default() != Some(443)))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != path
        {
            return Err(error(ErrorKind::Denied, "openai-image-endpoint-policy"));
        }
        if !local
            && !http.secret_headers.iter().any(|(name, value)| {
                name.eq_ignore_ascii_case("authorization") && value.prefix == "Bearer "
            })
        {
            return Err(error(ErrorKind::Denied, "openai-image-authorization"));
        }
        Ok(())
    }
    pub fn request(
        &self,
        model: &str,
        prompt: &str,
        inputs: &BTreeMap<String, Input<'_>>,
        limit: u64,
    ) -> Result<Vec<u8>> {
        self.check(model)?;
        if prompt.trim().is_empty()
            || prompt.len() > 32000
            || !(1..=128 * 1024 * 1024).contains(&limit)
        {
            return Err(error(ErrorKind::Budget, "openai-image-request-bounds"));
        }
        let mut body = serde_json::json!({"model":model,"prompt":prompt,"n":1,"stream":false,"size":format!("{}x{}",self.size[0],self.size[1]),"quality":self.quality,"background":self.background,"output_format":"png"});
        let mut total = prompt.len() as u64;
        let mut urls = BTreeMap::new();
        let mut dimensions = BTreeMap::new();
        for id in self.egress_inputs() {
            let input = inputs
                .get(&id)
                .ok_or_else(|| error(ErrorKind::Missing, "openai-image-input"))?;
            if input.artifact.media_type != "image/png"
                || input.artifact.bytes != input.bytes.len() as u64
                || reel_workflow::digest(input.bytes) != input.artifact.sha256
            {
                return Err(error(ErrorKind::Integrity, "openai-image-input-binding"));
            }
            let length = (input.bytes.len() as u64).div_ceil(3) * 4 + 22;
            total = total
                .checked_add(length)
                .ok_or_else(|| error(ErrorKind::Budget, "openai-image-request-bytes"))?;
            if total > limit || length > 20_971_520 {
                return Err(error(ErrorKind::Budget, "openai-image-request-bytes"));
            }
            dimensions.insert(id.clone(), png_image(input.bytes)?);
            urls.insert(
                id,
                format!("data:image/png;base64,{}", STANDARD.encode(input.bytes)),
            );
        }
        if !self.references.is_empty() {
            body["images"] = serde_json::json!(
                self.references
                    .iter()
                    .map(|id| serde_json::json!({"image_url":urls[id]}))
                    .collect::<Vec<_>>()
            );
        }
        if let Some(mask) = &self.mask {
            let first = &dimensions[&self.references[0]];
            let m = &dimensions[mask];
            if (first.width, first.height) != (m.width, m.height) || !m.alpha {
                return Err(error(ErrorKind::Invalid, "openai-image-mask-layout"));
            }
            body["mask"] = serde_json::json!({"image_url":urls[mask]});
        }
        let raw = serde_json::to_vec(&body)
            .map_err(|_| error(ErrorKind::Invalid, "openai-image-request-json"))?;
        if raw.len() as u64 > limit {
            return Err(error(ErrorKind::Budget, "openai-image-request-bytes"));
        }
        Ok(raw)
    }
    pub fn normalize(
        &self,
        model: &str,
        prompt: &str,
        response: &[u8],
        maximum: u64,
    ) -> Result<BTreeMap<String, Vec<u8>>> {
        self.check(model)?;
        #[derive(Deserialize)]
        struct Item {
            b64_json: Option<String>,
        }
        #[derive(Deserialize)]
        struct Reply {
            data: Vec<Item>,
            created: Option<u64>,
            output_format: Option<String>,
            size: Option<String>,
            background: Option<String>,
        }
        if response.len() > 128 * 1024 * 1024 || maximum == 0 || maximum > MAX_IMAGE {
            return Err(error(ErrorKind::Budget, "openai-image-response-bounds"));
        }
        let reply: Reply = serde_json::from_slice(response)
            .map_err(|_| error(ErrorKind::Integrity, "openai-image-response-json"))?;
        if reply.data.len() != 1
            || reply.output_format.as_deref().is_some_and(|f| f != "png")
            || reply
                .size
                .as_ref()
                .is_some_and(|s| s != &format!("{}x{}", self.size[0], self.size[1]))
            || reply
                .background
                .as_ref()
                .is_some_and(|b| self.background != "auto" && *b != self.background)
        {
            return Err(error(ErrorKind::Integrity, "openai-image-response-context"));
        }
        let encoded = reply
            .data
            .into_iter()
            .next()
            .unwrap()
            .b64_json
            .ok_or_else(|| {
                error(
                    ErrorKind::Unsupported,
                    "openai-image-inline-output-required",
                )
            })?;
        if encoded.len() as u64 > maximum.saturating_mul(4).div_ceil(3) + 8 {
            return Err(error(ErrorKind::Budget, "openai-image-output-bytes"));
        }
        let raw = STANDARD
            .decode(encoded)
            .map_err(|_| error(ErrorKind::Integrity, "openai-image-base64"))?;
        if raw.len() as u64 > maximum {
            return Err(error(ErrorKind::Budget, "openai-image-output-bytes"));
        }
        let image = png_image(&raw)?;
        if [image.width, image.height] != self.size
            || (self.background == "transparent" && !image.alpha)
            || (self.background == "opaque" && image.transparent)
        {
            return Err(error(ErrorKind::Integrity, "openai-image-output-layout"));
        }
        let metadata=serde_json::to_vec(&serde_json::json!({"schema":"reel.openai-image-candidate.v1","provider":"openai","requested_model":model,"prompt_sha256":reel_workflow::digest(prompt.as_bytes()),"spec_sha256":reel_workflow::canonical_digest(self)?,"response_sha256":reel_workflow::digest(response),"created":reply.created,"width":image.width,"height":image.height,"alpha_channel":image.alpha,"transparent_pixels":image.transparent,"image_sha256":reel_workflow::digest(&raw),"selected":false,"approved":false})).map_err(|_|error(ErrorKind::Invalid,"openai-image-metadata"))?;
        Ok(BTreeMap::from([
            ("candidate.png".into(), raw),
            ("image.json".into(), metadata),
        ]))
    }
}
