use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use git_asset::{
    github::{self, KeySnapshot, PublicKey, Trust},
    source::GitId,
};
use git_asset_core::{Digest, identity};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
};
pub(crate) fn canonical(v: &Value) -> Vec<u8> {
    identity::canonical_json(v).unwrap()
}
fn openssl() -> PathBuf {
    let name = if cfg!(windows) {
        "openssl.exe"
    } else {
        "openssl"
    };
    for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        let p = dir.join(name);
        if p.is_file() {
            return p;
        }
    }
    if cfg!(windows) {
        for var in ["ProgramFiles", "ProgramW6432"] {
            if let Some(root) = std::env::var_os(var) {
                for child in [
                    "Git/usr/bin/openssl.exe",
                    "Git/mingw64/bin/openssl.exe",
                    "OpenSSL/bin/openssl.exe",
                ] {
                    let p = PathBuf::from(&root).join(child);
                    if p.is_file() {
                        return p;
                    }
                }
            }
        }
    }
    panic!("OpenSSL is required for independent generated RSA qualification");
}
pub(crate) struct Fixture {
    pub(crate) root: tempfile::TempDir,
    pub(crate) openssl: PathBuf,
    pub(crate) trust: Trust,
    pub(crate) keys: KeySnapshot,
    pub(crate) claims: Value,
    pub(crate) commit: GitId,
}
impl Fixture {
    pub(crate) fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let openssl = openssl();
        let key = root.path().join("synthetic-private.pem");
        let status = Command::new(&openssl)
            .args([
                "genpkey",
                "-algorithm",
                "RSA",
                "-pkeyopt",
                "rsa_keygen_bits:2048",
                "-out",
            ])
            .arg(&key)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        let modulus = Command::new(&openssl)
            .args(["rsa", "-in"])
            .arg(&key)
            .args(["-noout", "-modulus"])
            .output()
            .unwrap();
        assert!(modulus.status.success());
        let modulus = String::from_utf8(modulus.stdout).unwrap();
        let hex = modulus.trim().strip_prefix("Modulus=").unwrap();
        let n = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect::<Vec<_>>();
        let trust = Trust {
            schema: "tig.github-trust.design-1".into(),
            audience: "git-asset:synthetic-storage".into(),
            subject: "repo:fixture-owner/fixture:ref:refs/heads/main".into(),
            repository: "fixture-owner/fixture".into(),
            repository_id: "1234".into(),
            owner_id: "5678".into(),
            ref_name: "refs/heads/main".into(),
            workflow_ref: "fixture-owner/fixture/.github/workflows/git-asset.yml@refs/heads/main"
                .into(),
            reusable: None,
            events: vec!["push".into()],
            max_token_seconds: 600,
            clock_skew_seconds: 5,
            selection_enrollment: Digest::blob(b"selection"),
            placement_enrollment: Digest::blob(b"placement"),
        };
        let commit = GitId::parse(&"a".repeat(40)).unwrap();
        let claims = json!({"iss":github::ISSUER,"aud":trust.audience,"sub":trust.subject,"repository":trust.repository,"repository_id":trust.repository_id,"repository_owner_id":trust.owner_id,"ref":trust.ref_name,"ref_type":"branch","sha":commit.as_str(),"event_name":"push","runner_environment":"github-hosted","workflow_ref":trust.workflow_ref,"workflow_sha":"b".repeat(40),"run_id":"9876","run_attempt":"1","jti":"synthetic-token-id","iat":1000,"nbf":1000,"exp":1300});
        let keys = KeySnapshot {
            schema: "tig.github-keys.design-1".into(),
            issuer: github::ISSUER.into(),
            valid_from: 900,
            valid_until: 1500,
            keys: vec![PublicKey {
                kid: "fixture-key".into(),
                n: URL_SAFE_NO_PAD.encode(n),
                e: "AQAB".into(),
            }],
        };
        Self {
            root,
            openssl,
            trust,
            keys,
            claims,
            commit,
        }
    }
    pub(crate) fn signed(&self, header: &Value, claims: &[u8]) -> String {
        let message = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(canonical(header)),
            URL_SAFE_NO_PAD.encode(claims)
        );
        let input = self.root.path().join("signing-input");
        fs::write(&input, &message).unwrap();
        let signature = Command::new(&self.openssl)
            .args(["dgst", "-sha256", "-sign"])
            .arg(self.root.path().join("synthetic-private.pem"))
            .arg(input)
            .output()
            .unwrap();
        assert!(signature.status.success());
        format!("{message}.{}", URL_SAFE_NO_PAD.encode(signature.stdout))
    }
    pub(crate) fn token(&self, claims: &Value) -> String {
        self.signed(
            &json!({"alg":"RS256","typ":"JWT","kid":"fixture-key"}),
            &canonical(claims),
        )
    }
    pub(crate) fn verify(&self, claims: &Value, now: u64) -> github::Result<github::Verified> {
        github::verify(
            &self.trust,
            &self.keys,
            &self.token(claims),
            &self.commit,
            now,
        )
    }
}
