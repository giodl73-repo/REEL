//! Stateless Responses API text/review composition and normalization. The native
//! caller owns admitted inputs and semantic review checks before publication.
use crate::{Config, error};
use base64::{Engine, engine::general_purpose::STANDARD};
use reel_workflow::{Artifact, ErrorKind, Result, review::Brief};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Format {
    Text,
    ReviewReport,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub schema: String,
    pub format: Format,
    pub output: String,
    pub max_output_tokens: u32,
}
pub struct Input<'a> {
    pub artifact: &'a Artifact,
    pub bytes: &'a [u8],
}
impl Spec {
    pub fn check(&self, model: &str) -> Result<()> {
        if self.schema != "reel.openai-text.v1"
            || model.is_empty()
            || model.len() > 128
            || !model
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            || !(1..=32768).contains(&self.max_output_tokens)
            || self.output.is_empty()
            || self.output.len() > 128
            || !self
                .output
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            || [
                ".",
                "..",
                "text.json",
                "http.json",
                "provider-request.json",
                "provider-response.json",
                "provider-config.json",
            ]
            .contains(&self.output.as_str())
        {
            return Err(error(ErrorKind::Invalid, "openai-text-spec"));
        }
        Ok(())
    }
    pub fn check_endpoint(&self, http: &Config) -> Result<()> {
        let url = reqwest::Url::parse(&http.endpoint)
            .map_err(|_| error(ErrorKind::Invalid, "openai-text-endpoint"))?;
        let local = http.allow_loopback_http
            && url.scheme() == "http"
            && url
                .host_str()
                .and_then(|h| h.trim_matches(['[', ']']).parse::<std::net::IpAddr>().ok())
                .is_some_and(|ip| ip.is_loopback());
        if (!local
            && (url.scheme() != "https"
                || url.host_str() != Some("api.openai.com")
                || url.port_or_known_default() != Some(443)))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/v1/responses"
        {
            return Err(error(ErrorKind::Denied, "openai-text-endpoint-policy"));
        }
        if !local
            && !http
                .secret_headers
                .iter()
                .any(|(n, v)| n.eq_ignore_ascii_case("authorization") && v.prefix == "Bearer ")
        {
            return Err(error(ErrorKind::Denied, "openai-text-authorization"));
        }
        Ok(())
    }
    pub fn media_type(&self) -> &'static str {
        if self.format == Format::Text {
            "text/plain"
        } else {
            "application/json"
        }
    }
    pub fn request(
        &self,
        model: &str,
        prompt: &str,
        inputs: &BTreeMap<String, Input<'_>>,
        brief: Option<&Brief>,
        limit: u64,
    ) -> Result<Vec<u8>> {
        self.check(model)?;
        if prompt.trim().is_empty()
            || prompt.len() > reel_workflow::MAX_CONTROL_BYTES
            || inputs.len() > 128
            || limit == 0
            || limit > 128 * 1024 * 1024
        {
            return Err(error(ErrorKind::Budget, "openai-text-request-bounds"));
        }
        let mut content = vec![];
        let format = match (self.format, brief) {
            (Format::Text, None) => json!({"type":"text"}),
            (Format::ReviewReport, Some(b))
                if b.schema == "reel.workflow-review-brief.v1"
                    && b.report_output == self.output =>
            {
                content.push(json!({"type":"input_text","text":serde_json::to_string(b).map_err(|_|error(ErrorKind::Invalid,"openai-text-brief"))?}));
                json!({"type":"json_schema","name":"reel_review_report","strict":true,"schema":report_schema(&b.empty_report.copy_sha256)})
            }
            _ => return Err(error(ErrorKind::Invalid, "openai-text-review-context")),
        };
        let mut total = prompt.len() as u64;
        for (id, input) in inputs {
            let a = input.artifact;
            total = total
                .checked_add(a.bytes)
                .filter(|n| *n <= limit)
                .ok_or_else(|| error(ErrorKind::Budget, "openai-text-input-bytes"))?;
            if id != &a.logical_id
                || a.bytes != input.bytes.len() as u64
                || reel_workflow::digest(input.bytes) != a.sha256
            {
                return Err(error(ErrorKind::Integrity, "openai-text-input-binding"));
            }
            match a.media_type.as_str() {
                "text/plain" | "application/json" => {
                    let text = std::str::from_utf8(input.bytes)
                        .map_err(|_| error(ErrorKind::Integrity, "openai-text-input-utf8"))?;
                    if a.media_type == "application/json" {
                        serde_json::from_slice::<Value>(input.bytes)
                            .map_err(|_| error(ErrorKind::Integrity, "openai-text-input-json"))?;
                    }
                    content.push(json!({"type":"input_text","text":serde_json::to_string(&json!({"artifact":a,"text":text})).map_err(|_|error(ErrorKind::Invalid,"openai-text-input-json"))?}));
                }
                "image/png" => {
                    crate::raster::inspect_png(input.bytes)?;
                    content.push(json!({"type":"input_text","text":serde_json::to_string(a).map_err(|_|error(ErrorKind::Invalid,"openai-text-input-json"))?}));
                    content.push(json!({"type":"input_image","image_url":format!("data:image/png;base64,{}",STANDARD.encode(input.bytes)),"detail":"high"}));
                }
                _ => return Err(error(ErrorKind::Unsupported, "openai-text-input-format")),
            }
        }
        if content.is_empty() {
            content.push(json!({"type":"input_text","text":prompt}));
        }
        let body = json!({"model":model,"instructions":prompt,"input":[{"role":"user","content":content}],"max_output_tokens":self.max_output_tokens,"store":false,"stream":false,"background":false,"tools":[],"tool_choice":"none","truncation":"disabled","text":{"format":format}});
        let raw = serde_json::to_vec(&body)
            .map_err(|_| error(ErrorKind::Invalid, "openai-text-request-json"))?;
        if raw.len() as u64 > limit {
            return Err(error(ErrorKind::Budget, "openai-text-request-bytes"));
        }
        Ok(raw)
    }
    pub fn normalize(
        &self,
        model: &str,
        request: &[u8],
        response: &[u8],
        maximum: u64,
    ) -> Result<BTreeMap<String, Vec<u8>>> {
        self.check(model)?;
        if response.len() > 128 * 1024 * 1024
            || maximum == 0
            || maximum > reel_workflow::MAX_CONTROL_BYTES as u64
        {
            return Err(error(ErrorKind::Budget, "openai-text-response-bounds"));
        }
        let r: Value = serde_json::from_slice(response)
            .map_err(|_| error(ErrorKind::Integrity, "openai-text-response-json"))?;
        let original: Value = serde_json::from_slice(request)
            .map_err(|_| error(ErrorKind::Integrity, "openai-text-original-request"))?;
        if original["model"] != model
            || original["text"]["format"]["type"]
                != if self.format == Format::Text {
                    "text"
                } else {
                    "json_schema"
                }
        {
            return Err(error(ErrorKind::Conflict, "openai-text-original-context"));
        }
        let id = r["id"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 256 && s.bytes().all(|b| b.is_ascii_graphic()))
            .ok_or_else(|| error(ErrorKind::Integrity, "openai-text-response-id"))?;
        if r["object"] != "response"
            || r["status"] != "completed"
            || !r["error"].is_null()
            || !r["incomplete_details"].is_null()
        {
            return Err(error(
                ErrorKind::NeedsRecovery,
                "openai-text-response-incomplete",
            ));
        }
        let output = r["output"]
            .as_array()
            .filter(|v| v.len() <= 128)
            .ok_or_else(|| error(ErrorKind::Integrity, "openai-text-output"))?;
        let mut text = String::new();
        let mut messages = 0;
        for item in output {
            match item["type"].as_str() {
                Some("reasoning") => (),
                Some("message") if item["role"] == "assistant" && item["status"] == "completed" => {
                    messages += 1;
                    let parts = item["content"]
                        .as_array()
                        .filter(|v| !v.is_empty() && v.len() <= 128)
                        .ok_or_else(|| error(ErrorKind::Integrity, "openai-text-content"))?;
                    for part in parts {
                        if part["type"] != "output_text" {
                            return Err(error(
                                ErrorKind::NeedsRecovery,
                                "openai-text-refusal-or-content",
                            ));
                        }
                        let chunk = part["text"]
                            .as_str()
                            .ok_or_else(|| error(ErrorKind::Integrity, "openai-text-content"))?;
                        if text.len() as u64 + chunk.len() as u64 > maximum {
                            return Err(error(ErrorKind::Budget, "openai-text-output-bytes"));
                        }
                        text.push_str(chunk);
                    }
                }
                _ => {
                    return Err(error(
                        ErrorKind::Unsupported,
                        "openai-text-unrequested-output",
                    ));
                }
            }
        }
        if messages != 1 || text.trim().is_empty() {
            return Err(error(ErrorKind::Integrity, "openai-text-message-count"));
        }
        if self.format == Format::ReviewReport {
            let report: reel_workflow::review::Report = reel_workflow::decode(text.as_bytes())?;
            if report.schema != "reel.workflow-review-report.v1"
                || original["text"]["format"]["schema"]["properties"]["copy_sha256"]["enum"][0]
                    != report.copy_sha256
            {
                return Err(error(ErrorKind::Stale, "openai-text-review-copy"));
            }
        }
        let meta=serde_json::to_vec(&json!({"schema":"reel.openai-text-result.v1","requested_model":model,"reported_model":r["model"],"response_id":id,"request_sha256":reel_workflow::digest(request),"response_sha256":reel_workflow::digest(response),"spec_sha256":reel_workflow::canonical_digest(self)?,"output_sha256":reel_workflow::digest(text.as_bytes()),"selected":false,"approved":false})).map_err(|_|error(ErrorKind::Invalid,"openai-text-result-json"))?;
        Ok(BTreeMap::from([
            (self.output.clone(), text.into_bytes()),
            ("text.json".into(), meta),
        ]))
    }
}
/// The wire schema uses the service's strict structural subset. Exact unit,
/// evidence, modality, frame and edition constraints remain native checks.
fn report_schema(copy: &str) -> Value {
    let string = json!({"type":"string"});
    let artifact = json!({"type":"object","additionalProperties":false,"required":["logical_id","sha256","bytes","media_type"],"properties":{"logical_id":string,"sha256":string,"bytes":{"type":"integer"},"media_type":string}});
    json!({"type":"object","additionalProperties":false,"required":["schema","copy_sha256","findings"],"properties":{"schema":{"type":"string","enum":["reel.workflow-review-report.v1"]},"copy_sha256":{"type":"string","enum":[copy]},"findings":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["code","unit","frames","modality","route","requested_change","evidence"],"properties":{"code":string,"unit":string,"frames":{"anyOf":[{"type":"null"},{"type":"object","additionalProperties":false,"required":["start","end"],"properties":{"start":{"type":"integer"},"end":{"type":"integer"}}}]},"modality":{"type":"string","enum":["visual","audio","audiovisual","text","layout"]},"route":{"type":"string","enum":["project","reel","tig"]},"requested_change":string,"evidence":{"type":"array","items":artifact}}}}}})
}
