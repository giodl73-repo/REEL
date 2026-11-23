//! Fixed-origin bounded GitHub metadata. No token forwarding to issuer/JWKS.
use super::*;
use std::io::Read;
pub struct Client {
    agent: ureq::Agent,
    token: Option<String>,
    millis: u64,
    api: String,
    jwks: String,
}
impl Client {
    /// The optional access token is independently supplied by the operator. It
    /// is never the incoming OIDC bearer and is sent only to GitHub's API.
    pub fn new(access_token: Option<String>, millis: u64) -> Result<Self> {
        if millis == 0
            || millis > 30000
            || access_token.as_ref().is_some_and(|s| {
                s.is_empty() || s.len() > 8192 || !s.bytes().all(|b| (0x21..=0x7e).contains(&b))
            })
        {
            return Err(Error::Invalid);
        }
        let agent = ureq::Agent::config_builder()
            .proxy(None)
            .max_redirects(0)
            .max_idle_connections(0)
            .http_status_as_error(false)
            .max_response_header_size(16384)
            .input_buffer_size(65536)
            .output_buffer_size(65536)
            .build()
            .into();
        Ok(Self {
            agent,
            token: access_token,
            millis,
            api: "https://api.github.com".into(),
            jwks: JWKS.into(),
        })
    }
    fn get(&self, url: &str, api: bool, start: Instant) -> Result<serde_json::Value> {
        self.get_bounded(url, api, start, 65536).map(|(v, _)| v)
    }
    pub(super) fn get_bounded(
        &self,
        url: &str,
        api: bool,
        start: Instant,
        cap: usize,
    ) -> Result<(serde_json::Value, usize)> {
        if cap == 0 || cap > 16 * 1024 * 1024 {
            return Err(Error::Budget);
        }
        let remaining = Duration::from_millis(self.millis)
            .checked_sub(start.elapsed())
            .ok_or(Error::Budget)?;
        self.get_for(url, api, remaining, cap)
    }
    pub(super) fn get_for(
        &self,
        url: &str,
        api: bool,
        remaining: Duration,
        cap: usize,
    ) -> Result<(serde_json::Value, usize)> {
        if cap == 0 || cap > 16 * 1024 * 1024 || remaining.is_zero() {
            return Err(Error::Budget);
        }
        let remaining = remaining.min(Duration::from_millis(self.millis));
        let start = Instant::now();
        let mut request = self
            .agent
            .get(url)
            .header("accept", "application/json")
            .header("accept-encoding", "identity")
            .header("user-agent", "git-assethub-trigger");
        if api && let Some(token) = &self.token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        if api {
            request = request.header("x-github-api-version", "2026-03-10");
        }
        let mut response = request
            .config()
            .timeout_global(Some(remaining))
            .build()
            .call()
            .map_err(|_| Error::Unavailable)?;
        if matches!(response.status().as_u16(), 401 | 403) {
            return Err(Error::Denied);
        }
        if response.status() != 200 {
            return Err(Error::Unavailable);
        }
        if response
            .headers()
            .get("content-encoding")
            .is_some_and(|v| v.as_bytes() != b"identity")
        {
            return Err(Error::Invalid);
        }
        if response.headers().get("content-type").is_none_or(|v| {
            v.to_str().ok().is_none_or(|s| {
                !matches!(
                    s.split(';').next().unwrap_or("").trim(),
                    "application/json" | "application/jwk-set+json"
                )
            })
        }) {
            return Err(Error::Invalid);
        }
        let mut bytes = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(cap as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::Unavailable)?;
        if bytes.len() > cap || start.elapsed() >= remaining {
            return Err(Error::Budget);
        }
        Ok((json(&bytes)?, bytes.len()))
    }
    pub fn keys(&self, now: u64, valid_seconds: u64) -> Result<KeySnapshot> {
        if valid_seconds == 0 || valid_seconds > 3600 {
            return Err(Error::Budget);
        }
        let value = self.get(&self.jwks, false, Instant::now())?;
        let array = value
            .get("keys")
            .and_then(serde_json::Value::as_array)
            .ok_or(Error::Key)?;
        if array.is_empty() || array.len() > 8 {
            return Err(Error::Key);
        }
        let mut keys = Vec::new();
        for key in array {
            if key.get("kty").and_then(serde_json::Value::as_str) != Some("RSA")
                || key.get("use").is_some_and(|v| v.as_str() != Some("sig"))
                || key.get("alg").is_some_and(|v| v.as_str() != Some("RS256"))
            {
                return Err(Error::Key);
            }
            let field = |name| {
                key.get(name)
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .ok_or(Error::Key)
            };
            keys.push(PublicKey {
                kid: field("kid")?,
                n: field("n")?,
                e: field("e")?,
            });
        }
        let snapshot = KeySnapshot {
            schema: "tig.github-keys.design-1".into(),
            issuer: ISSUER.into(),
            valid_from: now,
            valid_until: now.checked_add(valid_seconds).ok_or(Error::Clock)?,
            keys,
        };
        snapshot.validate(now)?;
        Ok(snapshot)
    }
    /// Returns current GitHub head only after checking immutable repository and
    /// owner identity; this does not refresh or mutate the local Git mirror.
    pub fn current_head(&self, trust: &Trust) -> Result<GitId> {
        trust.id()?;
        let start = Instant::now();
        let repository = self.get(
            &format!("{}/repositories/{}", self.api, trust.repository_id),
            true,
            start,
        )?;
        let id = |v: &serde_json::Value| v.as_u64().map(|n| n.to_string());
        if id(&repository["id"]).as_deref() != Some(&trust.repository_id)
            || id(&repository["owner"]["id"]).as_deref() != Some(&trust.owner_id)
            || repository["full_name"].as_str() != Some(&trust.repository)
        {
            return Err(Error::Trust);
        }
        let branch = trust.ref_name.strip_prefix("refs/").ok_or(Error::Trust)?;
        let branch = branch
            .bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect::<String>();
        let head = self.get(
            &format!("{}/repos/{}/git/ref/{}", self.api, trust.repository, branch),
            true,
            start,
        )?;
        if head["ref"].as_str() != Some(&trust.ref_name)
            || head["object"]["type"].as_str() != Some("commit")
        {
            return Err(Error::Trust);
        }
        GitId::parse(head["object"]["sha"].as_str().ok_or(Error::Invalid)?)
            .map_err(|_| Error::Invalid)
    }
    pub(super) fn api(&self) -> &str {
        &self.api
    }
    #[cfg(test)]
    pub(super) fn fixture_api(base: String) -> Self {
        let mut client = Self::new(None, 1000).unwrap();
        client.api = base;
        client
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, net::TcpListener, thread};
    fn peer(responses: Vec<(u16, String)>) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut byte = [0; 1];
                while !bytes.ends_with(b"\r\n\r\n") {
                    assert!(bytes.len() < 16384);
                    stream.read_exact(&mut byte).unwrap();
                    bytes.push(byte[0]);
                }
                requests.push(String::from_utf8(bytes).unwrap());
                write!(stream,"HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nLocation: http://127.0.0.1:1/never\r\n\r\n{body}",body.len()).unwrap();
            }
            requests
        });
        (address, handle)
    }
    fn trust() -> Trust {
        Trust {
            schema: "tig.github-trust.design-1".into(),
            audience: "git-asset:fixture".into(),
            subject: "fixture".into(),
            repository: "fixture/repo".into(),
            repository_id: "42".into(),
            owner_id: "7".into(),
            ref_name: "refs/heads/main".into(),
            workflow_ref: "fixture/repo/.github/workflows/git-asset.yml@refs/heads/main".into(),
            reusable: None,
            events: vec!["push".into()],
            max_token_seconds: 600,
            clock_skew_seconds: 0,
            selection_enrollment: Digest::blob(b"selection"),
            placement_enrollment: Digest::blob(b"placement"),
        }
    }
    #[test]
    fn jwks_origin_never_receives_private_api_credentials_and_snapshot_is_bounded() {
        let body=serde_json::json!({"keys":[{"kty":"RSA","use":"sig","alg":"RS256","kid":"fixture","n":URL_SAFE_NO_PAD.encode([0x80;256]),"e":"AQAB"}]}).to_string();
        let (base, server) = peer(vec![(200, body)]);
        let mut client = Client::new(Some("private-fixture-api-token".into()), 1000).unwrap();
        client.jwks = format!("{base}/jwks");
        let keys = client.keys(1000, 300).unwrap();
        assert_eq!(keys.valid_until, 1300);
        assert_eq!(keys.keys[0].kid, "fixture");
        let requests = server.join().unwrap();
        assert!(requests[0].starts_with("GET /jwks "));
        assert!(!requests[0].to_ascii_lowercase().contains("authorization:"));
        assert!(matches!(client.keys(1000, 3601), Err(Error::Budget)));
    }
    #[test]
    fn immutable_identity_and_exact_unicode_ref_precede_current_head() {
        let mut t = trust();
        t.ref_name = "refs/heads/版本".into();
        t.workflow_ref = "fixture/repo/.github/workflows/git-asset.yml@refs/heads/版本".into();
        let sha = "a".repeat(40);
        let (base, server) = peer(vec![
            (
                200,
                serde_json::json!({"id":42,"owner":{"id":7},"full_name":"fixture/repo"})
                    .to_string(),
            ),
            (
                200,
                serde_json::json!({"ref":t.ref_name,"object":{"type":"commit","sha":sha}})
                    .to_string(),
            ),
        ]);
        let mut client = Client::new(Some("private-fixture-api-token".into()), 1000).unwrap();
        client.api = base;
        assert_eq!(client.current_head(&t).unwrap().as_str(), sha);
        let requests = server.join().unwrap();
        assert!(requests[0].starts_with("GET /repositories/42 "));
        assert!(
            requests[1].starts_with("GET /repos/fixture/repo/git/ref/heads/%E7%89%88%E6%9C%AC ")
        );
        assert!(requests.iter().all(|s| {
            s.to_ascii_lowercase()
                .contains("authorization: bearer private-fixture-api-token")
        }));
    }
    #[test]
    fn mismatched_repository_denial_redirect_and_oversize_refuse() {
        for (status, body, expected) in [
            (
                200,
                serde_json::json!({"id":99,"owner":{"id":7},"full_name":"fixture/repo"})
                    .to_string(),
                Error::Trust,
            ),
            (403, "{}".into(), Error::Denied),
            (302, "{}".into(), Error::Unavailable),
            (200, "x".repeat(65537), Error::Budget),
        ] {
            let (base, server) = peer(vec![(status, body)]);
            let mut client = Client::new(None, 1000).unwrap();
            client.api = base;
            assert_eq!(client.current_head(&trust()).unwrap_err(), expected);
            assert_eq!(server.join().unwrap().len(), 1);
        }
    }
}
