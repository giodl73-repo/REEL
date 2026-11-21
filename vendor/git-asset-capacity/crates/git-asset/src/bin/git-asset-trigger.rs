//! Explicit foreground loopback receiver; never an automatic service installer.
#[path = "lifecycle/profiles.rs"]
mod profiles;
use git_asset::{
    github::{
        self,
        http::Client,
        intake,
        placement::{Budget, Execution, Permissions},
    },
    lifecycle::Enrollment,
};
use git_asset_core::{Digest, identity};
use git_asset_store::{
    existing::{Profile as SourceProfile, ReadCache},
    ledger::CapacityLedger,
    local::LocalStore,
    metadata::{self, MetadataStore},
    placement::{Cursor, Local},
    source_claims::admission::Namespace,
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpListener},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    trust: github::Trust,
    enrollment: Enrollment,
    namespace: profiles::NamespaceRoot,
    metadata_root: PathBuf,
    metadata_minimum: metadata::Checkpoint,
    placement_profile: PathBuf,
    api_token_file: Option<PathBuf>,
    api_token_env: Option<String>,
    listen: SocketAddr,
    budget: Budget,
    max_connections: u32,
    #[serde(default)]
    allow_receive: bool,
    #[serde(default)]
    allow_selection: bool,
}
fn read(path: &Path, cap: u64) -> github::Result<Vec<u8>> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(github::Error::Invalid);
    }
    let metadata = fs::symlink_metadata(path).map_err(|_| github::Error::Unavailable)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > cap {
        return Err(github::Error::Invalid);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(github::Error::Invalid);
        }
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| github::Error::Unavailable)?
        .take(cap + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| github::Error::Unavailable)?;
    if bytes.len() as u64 > cap {
        return Err(github::Error::Budget);
    }
    Ok(bytes)
}
fn parse<T: DeserializeOwned>(bytes: &[u8]) -> github::Result<T> {
    serde_json::from_value(identity::parse_json(bytes).map_err(|_| github::Error::Invalid)?)
        .map_err(|_| github::Error::Invalid)
}
fn now() -> github::Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|_| github::Error::Clock)
}
fn dispatch(
    profile: &Profile,
    client: &Client,
    proof: github::Verified,
    cursor: Option<Cursor>,
) -> github::Result<Value> {
    if !profile.allow_selection {
        return Err(github::Error::Disabled);
    }
    let storage: profiles::Storage = parse(&read(&profile.placement_profile, 65536)?)?;
    if storage.schema != "tig.placement-profile.design-1"
        || storage.providers.len() != storage.config.providers.len()
        || storage.namespaces.len() != storage.config.namespaces.len()
    {
        return Err(github::Error::Trust);
    }
    proof.admit(
        now()?,
        &profile.enrollment.id().map_err(|_| github::Error::Trust)?,
        &storage.config.id().map_err(|_| github::Error::Trust)?,
    )?;
    let seed_identity = Digest::blob(
        &identity::canonical_json(&storage.byte_source).map_err(|_| github::Error::Invalid)?,
    );
    if seed_identity != storage.config.seed {
        return Err(github::Error::Trust);
    }
    let seed_profile: SourceProfile =
        serde_json::from_value(storage.byte_source).map_err(|_| github::Error::Invalid)?;
    let mut seed = ReadCache::new(seed_profile).map_err(|_| github::Error::Unavailable)?;
    // All machine roots are opened only inside authenticated dispatch.
    let mut namespaces = storage
        .namespaces
        .iter()
        .map(|n| Namespace::open(&n.root, &n.minimum).map_err(|_| github::Error::Unavailable))
        .collect::<github::Result<Vec<_>>>()?;
    let source_namespace = Namespace::open(&profile.namespace.root, &profile.namespace.minimum)
        .map_err(|_| github::Error::Unavailable)?;
    if source_namespace.identity() != &profile.enrollment.namespace
        || !namespaces
            .iter()
            .any(|n| n.identity() == source_namespace.identity())
    {
        return Err(github::Error::Trust);
    }
    let source_index = namespaces
        .iter()
        .position(|n| n.identity() == source_namespace.identity())
        .ok_or(github::Error::Trust)?;
    if fs::canonicalize(&profile.namespace.root).map_err(|_| github::Error::Unavailable)?
        != fs::canonicalize(&storage.namespaces[source_index].root)
            .map_err(|_| github::Error::Unavailable)?
    {
        return Err(github::Error::Trust);
    }
    namespaces[source_index] = source_namespace;
    let mut metadata = MetadataStore::open(&profile.metadata_root, &profile.metadata_minimum)
        .map_err(|_| github::Error::Unavailable)?;
    let stores = storage
        .providers
        .iter()
        .map(|p| LocalStore::open(&p.root).map_err(|_| github::Error::Unavailable))
        .collect::<github::Result<Vec<_>>>()?;
    let mut ledgers = storage
        .providers
        .iter()
        .map(|p| {
            CapacityLedger::open(&p.ledger_root, &p.minimum).map_err(|_| github::Error::Unavailable)
        })
        .collect::<github::Result<Vec<_>>>()?;
    let mut locals = stores
        .iter()
        .zip(ledgers.iter_mut())
        .zip(storage.config.providers.iter())
        .map(|((store, ledger), p)| Local {
            store,
            ledger,
            job: &p.job,
        })
        .collect::<Vec<_>>();
    let report = Execution {
        trust: &profile.trust,
        request: &proof,
        client,
        enrollment: &profile.enrollment,
        config: &storage.config,
        namespaces: &mut namespaces,
        metadata: &mut metadata,
        places: &mut locals,
        seed: &mut seed,
        seed_identity: &seed_identity,
        permissions: Permissions {
            selection: profile.allow_selection,
            copy: storage.allow_copy,
            eviction: storage.allow_eviction,
            reacquisition: storage.allow_reacquisition,
            cancellation: storage.allow_cancellation,
        },
        budget: &profile.budget,
    }
    .run(cursor.as_ref(), now)?;
    serde_json::to_value(report).map_err(|_| github::Error::Invalid)
}
fn output(value: &Value) -> github::Result<()> {
    let bytes = identity::canonical_json(value).map_err(|_| github::Error::Invalid)?;
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&bytes)
        .map_err(|_| github::Error::Unavailable)?;
    stdout
        .write_all(b"\n")
        .map_err(|_| github::Error::Unavailable)?;
    stdout.flush().map_err(|_| github::Error::Unavailable)
}
fn run() -> github::Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() == 1 && args[0] == "describe" {
        return output(
            &json!({"schema":"tig.github-receiver-interface.design-1","operations":["describe","serve PROFILE"],"profile_schema":"tig.github-receiver.design-1","path":"/v1/reconcile","method":"POST","request_schema":"tig.github-request.design-1","loopback_only":true,"default_receive":false,"default_selection":false,"installs_service":false,"executes_repository_code":false,"request_authority":"signed_oidc_plus_private_enrollment","convergence":"complete_same_basis_pass"}),
        );
    }
    if args.len() != 2 || args[0] != "serve" {
        return Err(github::Error::Invalid);
    }
    let profile: Profile = parse(&read(Path::new(&args[1]), 65536)?)?;
    if profile.schema != "tig.github-receiver.design-1"
        || !profile.listen.ip().is_loopback()
        || profile.max_connections == 0
        || profile.max_connections > 4096
    {
        return Err(github::Error::Invalid);
    }
    if !profile.allow_receive {
        return Err(github::Error::Disabled);
    }
    profile.budget.validate()?;
    profile.trust.id()?;
    if profile.enrollment.id().map_err(|_| github::Error::Trust)?
        != profile.trust.selection_enrollment
        || profile.enrollment.ref_name != profile.trust.ref_name
    {
        return Err(github::Error::Trust);
    }
    if profile.api_token_file.is_some() && profile.api_token_env.is_some() {
        return Err(github::Error::Invalid);
    }
    let mut token = profile
        .api_token_file
        .as_ref()
        .map(|p| {
            read(p, 8192).and_then(|b| {
                String::from_utf8(b)
                    .map(|s| s.trim_end_matches(['\r', '\n']).to_owned())
                    .map_err(|_| github::Error::Invalid)
            })
        })
        .transpose()?;
    if let Some(name) = &profile.api_token_env {
        if name.is_empty()
            || name.len() > 128
            || !name
                .bytes()
                .all(|c| c.is_ascii_uppercase() || c == b'_' || c.is_ascii_digit())
            || name.starts_with(|c: char| c.is_ascii_digit())
        {
            return Err(github::Error::Invalid);
        }
        token = Some(std::env::var(name).map_err(|_| github::Error::Unavailable)?);
    }
    let client = Client::new(token, 30000)?;
    let mut keys = client.keys(now()?, 300)?;
    let listener = TcpListener::bind(profile.listen).map_err(|_| github::Error::Unavailable)?;
    output(
        &json!({"schema":"tig.github-listener.design-1","listening":listener.local_addr().map_err(|_|github::Error::Unavailable)?.to_string()}),
    )?;
    for _ in 0..profile.max_connections {
        let (stream, _) = listener.accept().map_err(|_| github::Error::Unavailable)?;
        if keys.validate(now()?).is_err() {
            keys = client.keys(now()?, 300)?;
        }
        // One foreground request per receiver; other enrolled executors retain
        // independent scoped admission. No global provider/cache mutex exists.
        let _ = intake::handle(stream, &profile.trust, &keys, now, |proof, cursor| {
            dispatch(&profile, &client, proof, cursor)
        });
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        let _ = output(
            &json!({"schema":"tig.github-receiver-response.design-1","ok":false,"code":error}),
        );
        std::process::exit(1);
    }
}
