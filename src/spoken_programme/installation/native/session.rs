//! Private native invocation controls, not a new programme/asset manifest.
//! Original production identity is independent of the playback build context.
use super::*;
use crate::scene_delivery::FileRef;
use std::path::{Path, PathBuf};

pub const SESSION_SCHEMA: &str = "reel.installation-session.v1";
pub const MAX_REQUEST: usize = 16 * 1024;
const MAX_CONTROL: usize = 16 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OriginalProgramme {
    pub declaration: FileRef,
    pub plan: FileRef,
    pub receipt: FileRef,
    pub processing: Option<FileRef>,
    pub access_manifest: FileRef,
    /// Retained original compiler/producer bytes. Never execute this file or
    /// replace its identity with the current playback executable/receipt.
    pub compiler_producer: FileRef,
    pub source_root: PathBuf,
    pub candidate_root: PathBuf,
    pub access_root: PathBuf,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaybackBuild {
    pub runtime: FileRef,
    /// Independently bound build context; not an inferred source attestation.
    pub source_commit: String,
    pub cargo_lock: FileRef,
    pub cpal_version: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionRequest {
    pub schema: String,
    pub edition_id: String,
    pub passes: u8,
    pub original: OriginalProgramme,
    pub playback: PlaybackBuild,
    pub endpoint: EndpointBinding,
}
#[derive(Serialize)]
pub struct SessionCheck {
    pub programme: Summary,
    pub original_runtime_bytes_checked: bool,
    pub original_reads_guarded: bool,
    pub playback_runtime_checked: bool,
    pub host_profile_checked: bool,
    pub endpoint_probed: bool,
}
fn absolute(path: &Path) -> bool {
    path.is_absolute()
        && path
            .to_str()
            .is_some_and(|s| !s.is_empty() && s.len() <= 1024 && !s.chars().any(char::is_control))
}
fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn reference(file: &FileRef, max: u64) -> Result<()> {
    ensure!(
        absolute(&file.path) && hex(&file.sha256, 64) && file.bytes > 0 && file.bytes <= max,
        "installation original/build reference profile"
    );
    Ok(())
}
impl SessionRequest {
    /// Shape only: no filesystem, source processing, endpoint or playback.
    pub fn parse(raw: &[u8]) -> Result<Self> {
        ensure!(
            raw.len() <= MAX_REQUEST,
            "installation request byte profile"
        );
        let value: Self = serde_json::from_slice(raw)
            .map_err(|_| anyhow::anyhow!("installation request is not strict JSON"))?;
        value.validate()?;
        Ok(value)
    }
    pub fn selection(&self) -> Selection {
        Selection {
            edition_id: self.edition_id.clone(),
            passes: self.passes,
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema == SESSION_SCHEMA
                && !self.edition_id.trim().is_empty()
                && self.edition_id.len() <= 256
                && !self.edition_id.chars().any(char::is_control)
                && (1..=3).contains(&self.passes)
                && serde_json::to_vec(self)?.len() <= MAX_REQUEST,
            "installation session selection/profile"
        );
        self.endpoint.validate()?;
        let original = &self.original;
        reference(&original.declaration, 1024 * 1024)?;
        reference(&original.plan, MAX_CONTROL as u64)?;
        reference(&original.receipt, MAX_CONTROL as u64)?;
        reference(&original.access_manifest, 1024 * 1024)?;
        reference(&original.compiler_producer, 1024 * 1024 * 1024)?;
        if let Some(policy) = &original.processing {
            reference(policy, 1024 * 1024)?;
        }
        ensure!(
            [
                &original.source_root,
                &original.candidate_root,
                &original.access_root
            ]
            .into_iter()
            .all(|p| absolute(p)),
            "installation explicit original roots"
        );
        reference(&self.playback.runtime, 1024 * 1024 * 1024)?;
        reference(&self.playback.cargo_lock, 4 * 1024 * 1024)?;
        ensure!(
            hex(&self.playback.source_commit, 40) && self.playback.cpal_version == "0.18.2",
            "installation declared build context"
        );
        Ok(())
    }
    /// Explicit original-input checking only. This does not bind/execute the
    /// declared player or query an endpoint, even when those fields are valid.
    pub fn check_originals(
        &self,
        mut control: producer::ProductionControl<impl FnMut() -> Result<()>>,
    ) -> Result<SessionCheck> {
        self.validate()?;
        ensure!(
            control.timeout <= Duration::from_secs(30),
            "installation original check deadline profile"
        );
        (control.cancellation)()?;
        #[cfg(not(windows))]
        {
            anyhow::bail!("installation protected original file reads require Windows")
        }
        #[cfg(windows)]
        {
            let deadline = Instant::now() + control.timeout;
            let mut tick = || {
                (control.cancellation)()?;
                ensure!(
                    Instant::now() < deadline,
                    "installation original check deadline elapsed"
                );
                Ok(())
            };
            let loaded = LoadedProgramme::load(&self.original, &mut tick)?;
            loaded.recheck_controls(&mut tick)?;
            let prepared = super::super::prepare(
                &self.selection(),
                loaded.inputs(),
                producer::ProductionControl {
                    timeout: deadline.saturating_duration_since(Instant::now()),
                    cancellation: &mut tick,
                },
            )?;
            tick()?;
            Ok(SessionCheck {
                programme: prepared.summary(),
                original_runtime_bytes_checked: true,
                original_reads_guarded: true,
                playback_runtime_checked: false,
                host_profile_checked: false,
                endpoint_probed: false,
            })
        }
    }
}

#[cfg(windows)]
mod files {
    use super::*;
    use crate::{
        media_runtime::BoundInput,
        spoken_programme::{self, compiler::CompilerIdentity},
    };
    use std::{collections::BTreeMap, sync::Mutex};

    struct Root {
        declared: PathBuf,
        canonical: PathBuf,
        held: Mutex<BTreeMap<PathBuf, BoundInput>>,
    }
    impl Root {
        fn new(path: &Path) -> Result<Self> {
            ensure!(
                absolute(path) && path.is_dir(),
                "installation original root unavailable"
            );
            Ok(Self {
                declared: path.into(),
                canonical: path.canonicalize()?,
                held: Mutex::new(BTreeMap::new()),
            })
        }
    }
    impl InputFiles for Root {
        fn media_program(&self, _: crate::scene_delivery::MediaProgram) -> Result<PathBuf> {
            anyhow::bail!("installation originals have no external-program capability")
        }
        fn verified_path(&self, item: &FileRef) -> Result<PathBuf> {
            ensure!(
                item.bytes <= 64 * 1024 * 1024,
                "installation original file byte budget"
            );
            ensure!(
                self.declared.canonicalize()? == self.canonical,
                "installation original namespace changed"
            );
            let path = crate::scene_delivery::checked_file(&self.canonical, item)?;
            let mut held = self
                .held
                .lock()
                .map_err(|_| anyhow::anyhow!("installation input guard poisoned"))?;
            if let Some(file) = held.get(&path) {
                ensure!(
                    file.identity().sha256 == item.sha256 && file.identity().bytes == item.bytes,
                    "installation conflicting original pin"
                );
                file.recheck()?;
                return Ok(file.path().into());
            }
            ensure!(
                held.len() < 128 && item.bytes <= 64 * 1024 * 1024,
                "installation original provider budget"
            );
            let total = held
                .values()
                .try_fold(item.bytes, |n, file| n.checked_add(file.identity().bytes))
                .ok_or_else(|| anyhow::anyhow!("installation original byte overflow"))?;
            ensure!(
                total <= 64 * 1024 * 1024,
                "installation original provider byte budget"
            );
            let guard = BoundInput::bind(&FileRef {
                path: path.clone(),
                sha256: item.sha256.clone(),
                bytes: item.bytes,
            })?;
            let actual = guard.path().to_owned();
            held.insert(path, guard);
            Ok(actual)
        }
    }
    /// Owned by the source-check thread, never the audio callback/controller.
    /// Guards remain alive through every original check and native-worker reap.
    pub(crate) struct LoadedProgramme {
        document: spoken_programme::Document,
        plan: Vec<u8>,
        receipt: Vec<u8>,
        policy: producer::ProcessingPolicy,
        access_manifest: Vec<u8>,
        compiler: CompilerIdentity,
        controls: Vec<BoundInput>,
        sources: Root,
        candidate: Root,
        access: Root,
    }
    impl LoadedProgramme {
        pub(crate) fn load(
            original: &OriginalProgramme,
            mut tick: impl FnMut() -> Result<()>,
        ) -> Result<Self> {
            tick()?;
            let mut controls = Vec::new();
            let mut read = |file: &FileRef, max| -> Result<Vec<u8>> {
                tick()?;
                reference(file, max as u64)?;
                let guard = BoundInput::bind(file)?;
                let raw = guard.read(max)?;
                controls.push(guard);
                tick()?;
                Ok(raw)
            };
            let document = spoken_programme::parse(&read(&original.declaration, 1024 * 1024)?)?;
            let plan = read(&original.plan, MAX_CONTROL)?;
            let receipt = read(&original.receipt, MAX_CONTROL)?;
            let policy = match &original.processing {
                Some(file) => producer::parse_processing(&read(file, 1024 * 1024)?)?,
                None => producer::ProcessingPolicy::identity(),
            };
            let access_manifest = read(&original.access_manifest, 1024 * 1024)?;
            reference(&original.compiler_producer, 1024 * 1024 * 1024)?;
            let compiler_guard = BoundInput::bind(&original.compiler_producer)?;
            let compiler = CompilerIdentity {
                sha256: compiler_guard.identity().sha256.clone(),
                bytes: compiler_guard.identity().bytes,
            };
            controls.push(compiler_guard);
            tick()?;
            Ok(Self {
                document,
                plan,
                receipt,
                policy,
                access_manifest,
                compiler,
                controls,
                sources: Root::new(&original.source_root)?,
                candidate: Root::new(&original.candidate_root)?,
                access: Root::new(&original.access_root)?,
            })
        }
        pub(crate) fn recheck_controls(&self, mut tick: impl FnMut() -> Result<()>) -> Result<()> {
            for file in &self.controls {
                tick()?;
                file.recheck()?;
            }
            tick()
        }
        pub(crate) fn inputs(&self) -> Inputs<'_> {
            Inputs {
                spoken: checker::CheckRequest {
                    document: &self.document,
                    plan_bytes: &self.plan,
                    receipt_bytes: &self.receipt,
                    processing: &self.policy,
                    expected_runtime: &self.compiler,
                },
                sources: &self.sources,
                candidate: &self.candidate,
                access: &self.access,
                access_manifest: &self.access_manifest,
            }
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn guarded_original_provider_has_no_ambient_program_fallback() {
            let dir = tempfile::tempdir().unwrap();
            let root = Root::new(dir.path()).unwrap();
            assert!(
                root.media_program(crate::scene_delivery::MediaProgram::Ffmpeg)
                    .is_err()
            );
            assert!(
                root.media_program(crate::scene_delivery::MediaProgram::Ffprobe)
                    .is_err()
            );
        }
    }
}
#[cfg(windows)]
pub(crate) use files::LoadedProgramme;

#[cfg(all(test, windows))]
pub(super) fn fixture_request() -> SessionRequest {
    tests::request()
}
#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn request() -> SessionRequest {
        let file = FileRef {
            path: std::env::current_exe().unwrap(),
            sha256: "a".repeat(64),
            bytes: 1,
        };
        let root = std::env::current_dir().unwrap();
        SessionRequest {
            schema: SESSION_SCHEMA.into(),
            edition_id: "es-technical".into(),
            passes: 2,
            original: OriginalProgramme {
                declaration: file.clone(),
                plan: file.clone(),
                receipt: file.clone(),
                processing: None,
                access_manifest: file.clone(),
                compiler_producer: file.clone(),
                source_root: root.clone(),
                candidate_root: root.clone(),
                access_root: root,
            },
            playback: PlaybackBuild {
                runtime: file.clone(),
                source_commit: "b".repeat(40),
                cargo_lock: file,
                cpal_version: "0.18.2".into(),
            },
            endpoint: EndpointBinding {
                profile: PROFILE.into(),
                device_id: "wasapi:{0.0.0.00000000}.{11111111-1111-1111-1111-111111111111}".into(),
                os_build: 26300,
                driver_version: "fixture".into(),
                configuration_sha256: "c".repeat(64),
            },
        }
    }
    #[test]
    fn private_session_shape_rejects_forged_or_unbounded_controls_without_execution() {
        let r = request();
        r.validate().unwrap();
        let raw = serde_json::to_vec(&r).unwrap();
        assert_eq!(
            SessionRequest::parse(&raw).unwrap().selection().edition_id,
            "es-technical"
        );
        let mut forged: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        forged["device_playback_checked"] = serde_json::json!(true);
        assert!(SessionRequest::parse(&serde_json::to_vec(&forged).unwrap()).is_err());
        assert!(SessionRequest::parse(&vec![b' '; MAX_REQUEST + 1]).is_err());
        for mutation in [
            (|x: &mut SessionRequest| x.passes = 4) as fn(&mut SessionRequest),
            |x| x.endpoint.device_id = "default".into(),
            |x| x.original.source_root = "relative".into(),
            |x| x.original.compiler_producer.sha256 = "A".repeat(64),
            |x| x.original.plan.bytes = MAX_CONTROL as u64 + 1,
            |x| x.playback.source_commit.clear(),
            |x| x.playback.cpal_version = "latest".into(),
        ] {
            let mut value = r.clone();
            mutation(&mut value);
            assert!(value.validate().is_err());
        }
    }
    #[test]
    fn cancelled_original_check_never_opens_original_controls() {
        let result = request().check_originals(producer::ProductionControl {
            timeout: Duration::from_secs(30),
            cancellation: || anyhow::bail!("fixture cancellation before filesystem access"),
        });
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("fixture cancellation")
        );
    }
    #[cfg(windows)]
    #[test]
    fn expired_original_check_never_binds_a_runtime() {
        let result = request().check_originals(producer::ProductionControl {
            timeout: Duration::ZERO,
            cancellation: || Ok(()),
        });
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("deadline elapsed")
        );
    }
    #[cfg(not(windows))]
    #[test]
    fn protected_source_check_refuses_without_native_guards() {
        let result = request().check_originals(producer::ProductionControl {
            timeout: Duration::from_secs(30),
            cancellation: || Ok(()),
        });
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("require Windows")
        );
    }
}
