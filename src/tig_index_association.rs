//! Pinned checksum assertions joined to saved target metadata. Never a provider
//! receipt, current remote observation, producer claim or custody authority.
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::Path,
};
use tig_core::{Digest, LogicalPath, Object};
use tig_store::existing::{ByteSource, Profile as ByteProfile, ReadCache};

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum IndexFormat {
    Sha256sums,
}
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum InventoryFormat {
    JsonLines,
}
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Separator {
    Slash,
    Backslash,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    schema: String,
    byte_source: ByteProfile,
    index: Object,
    inventory: Object,
    index_format: IndexFormat,
    inventory_format: InventoryFormat,
    index_prefix: String,
    target_prefix: String,
    target_separator: Separator,
    verify_payloads: bool,
    max_rows: usize,
    max_output_bytes: usize,
}
#[derive(Deserialize)]
struct Observed {
    relative_path: String,
    bytes: u64,
}

fn capture<S: ByteSource>(source: &mut S, pin: &Object, max: u64) -> Result<(Vec<u8>, S::Reader)> {
    if pin.bytes > max {
        bail!("index metadata byte budget");
    }
    let mut reader = source.open_verified(pin)?;
    let mut raw = Vec::new();
    reader.by_ref().take(pin.bytes + 1).read_to_end(&mut raw)?;
    if Object::from_bytes(&raw) != *pin {
        bail!("index metadata identity differs");
    }
    Ok((raw, reader))
}

/// Pure association over an explicitly supplied verified byte provider. Payload
/// reads are opt-in; their observations are sequential, not a global snapshot.
pub fn prepare(p: &Profile, source: &mut impl ByteSource) -> Result<Value> {
    if p.schema != "reel.tig-checksum-index-association.experimental-1"
        || !(1..=4096).contains(&p.max_rows)
        || !(1..=16 * 1024 * 1024).contains(&p.max_output_bytes)
        || p.target_prefix.len() > 2048
        || p.target_prefix.chars().any(char::is_control)
    {
        bail!("invalid index association profile");
    }
    LogicalPath::parse(&p.index_prefix)?;
    let (index, _index_guard) = capture(source, &p.index, 4 * 1024 * 1024)?;
    let (inventory, _inventory_guard) = capture(source, &p.inventory, 16 * 1024 * 1024)?;
    let mut observed = BTreeMap::new();
    match p.inventory_format {
        InventoryFormat::JsonLines => {
            for line in std::str::from_utf8(&inventory)?.lines() {
                if line.is_empty() {
                    continue;
                }
                if line.len() > 65_536 || observed.len() == 65_536 {
                    bail!("inventory row budget");
                }
                crate::approval_attestation::ensure_no_duplicate_keys(line.as_bytes())?;
                let row: Observed = serde_json::from_str(line)?;
                LogicalPath::parse(&row.relative_path)?;
                if observed.insert(row.relative_path, row.bytes).is_some() {
                    bail!("duplicate inventory path");
                }
            }
        }
    }
    let mut files = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut lengths = BTreeMap::new();
    match p.index_format {
        IndexFormat::Sha256sums => {
            for (line_index, line) in std::str::from_utf8(&index)?.lines().enumerate() {
                if line.is_empty() {
                    continue;
                }
                let bytes = line.as_bytes();
                if files.len() == p.max_rows
                    || bytes.len() < 67
                    || bytes[64] != b' '
                    || !matches!(bytes[65], b' ' | b'*')
                    || !bytes[..64]
                        .iter()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
                {
                    bail!("unsupported checksum line or row budget");
                }
                let digest = Digest::parse(&line[..64])?;
                let member = &line[66..];
                LogicalPath::parse(member)?;
                let relative = format!("{}/{member}", p.index_prefix);
                LogicalPath::parse(&relative)?;
                if !seen.insert(relative.clone()) {
                    bail!("duplicate indexed path");
                }
                let object = Object {
                    sha256: digest,
                    bytes: *observed
                        .get(&relative)
                        .context("indexed target absent from saved inventory")?,
                };
                if lengths
                    .insert(object.sha256.clone(), object.bytes)
                    .is_some_and(|old| old != object.bytes)
                {
                    bail!("one digest has conflicting declared lengths");
                }
                if p.verify_payloads {
                    // This verifies only the named object; it neither hydrates nor
                    // retains bytes, and grants no multi-object custody claim.
                    let _payload_guard = source.open_verified(&object)?;
                }
                let suffix = match p.target_separator {
                    Separator::Slash => relative.clone(),
                    Separator::Backslash => relative.replace('/', "\\"),
                };
                let target = format!("{}{suffix}", p.target_prefix);
                if target.len() > 4096 {
                    bail!("target locator budget");
                }
                files.push(
                    json!({"path":target,"relative_path":relative,"asset":object,
                "checksum_line":line_index+1,"cache_payload_verified":p.verify_payloads,
                "provider_payload_readback_verified":false}),
                );
            }
        }
    }
    if files.is_empty() {
        bail!("empty checksum index");
    }
    let report = json!({"schema":"reel.checksum-index-target-association.experimental-1",
        "index":p.index,"inventory":p.inventory,"evidence_basis":"checksum-assertion + saved-target-metadata",
        "payload_observation":"sequential exact-object verification only when explicitly enabled",
        "files":files,"cache_payloads_verified":p.verify_payloads,
        "provider_identity_verified":false,"current_remote_content_verified":false,
        "provider_payload_readback_verified":false,"producer_correspondence_verified":false,
        "creative_approval":false,"publication_proven":false,"independent_custody_verified":false,
        "authorizes_eviction":false,"cache_changed":false});
    if serde_json::to_vec(&report)?
        .len()
        .checked_add(1)
        .is_none_or(|n| n > p.max_output_bytes)
    {
        bail!("association output byte budget");
    }
    Ok(report)
}

/// Explicit new private metadata output. No directory, cache or Git initialization.
pub fn run(profile: &Path, output: &Path) -> Result<Value> {
    let mut raw = Vec::new();
    fs::File::open(profile)?
        .take(65_537)
        .read_to_end(&mut raw)?;
    if raw.len() > 65_536 {
        bail!("index profile byte budget");
    }
    crate::approval_attestation::ensure_no_duplicate_keys(&raw)?;
    let p: Profile = serde_json::from_slice(&raw)?;
    if !output.is_absolute() || output.file_name().is_none() {
        bail!("new absolute private output required");
    }
    for component in output.components() {
        match component {
            std::path::Component::Normal(name) => {
                LogicalPath::parse(name.to_str().context("non-Unicode output member")?)?;
            }
            std::path::Component::ParentDir | std::path::Component::CurDir => {
                bail!("ordinary output path required");
            }
            _ => {}
        }
    }
    if fs::symlink_metadata(output).is_ok() {
        bail!("metadata output already exists");
    }
    let parent = output.parent().context("output parent missing")?;
    for ancestor in parent.ancestors() {
        let metadata = fs::symlink_metadata(ancestor)?;
        #[cfg(windows)]
        let reparse = {
            use std::os::windows::fs::MetadataExt;
            metadata.file_attributes() & 0x400 != 0
        };
        #[cfg(not(windows))]
        let reparse = metadata.file_type().is_symlink();
        if !metadata.is_dir() || reparse {
            bail!("ordinary output ancestors required");
        }
    }
    let parent = parent.canonicalize()?;
    for place in &p.byte_source.places {
        let root = place
            .root
            .canonicalize()
            .unwrap_or_else(|_| place.root.clone());
        if parent.starts_with(root) {
            bail!("metadata output must be outside byte-provider roots");
        }
    }
    let mut source = ReadCache::new(p.byte_source.clone())?;
    let report = prepare(&p, &mut source)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    file.write_all(&serde_json::to_vec(&report)?)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(json!({"files":report["files"].as_array().unwrap().len(),
        "cache_payloads_verified":p.verify_payloads,"cache_changed":false,
        "verified_requests":source.requests(),"verified_bytes":source.verification_bytes(),
        "publication_proven":false,"current_remote_content_verified":false}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    struct Fixture {
        temp: tempfile::TempDir,
        value: Value,
        payload: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("existing");
            fs::create_dir_all(root.join("objects")).unwrap();
            let put = |raw: &[u8]| {
                let pin = Object::from_bytes(raw);
                fs::write(root.join("objects").join(pin.sha256.as_str()), raw).unwrap();
                pin
            };
            let payload = put(b"abc");
            let index = format!(
                "{}  one.txt\r\n{} *two.txt\r\n",
                payload.sha256.as_str(),
                payload.sha256.as_str()
            );
            let inventory = b"{\"relative_path\":\"CRM/one.txt\",\"bytes\":3,\"note\":\"saved\"}\n{\"relative_path\":\"CRM/two.txt\",\"bytes\":3}\n";
            let value = json!({"schema":"reel.tig-checksum-index-association.experimental-1",
                "byte_source":{"places":[{"root":root,"layout":"flat"}],
                    "required":if cfg!(windows){"prevent-mutation"}else{"cooperative-immutable"},
                    "limits":{"object_bytes":16777216,"total_bytes":67108864,"requests":16,"live_readers":3,"verify_millis":60000}},
                "index":put(index.as_bytes()),"inventory":put(inventory),
                "index_format":"sha256sums","inventory_format":"json-lines",
                "index_prefix":"CRM","target_prefix":"private-store://","target_separator":"slash",
                "verify_payloads":true,"max_rows":16,"max_output_bytes":65536});
            let payload = root.join("objects").join(payload.sha256.as_str());
            Self {
                temp,
                value,
                payload,
            }
        }
        fn replace(&mut self, name: &str, raw: &[u8]) {
            let pin = Object::from_bytes(raw);
            fs::write(
                self.payload.parent().unwrap().join(pin.sha256.as_str()),
                raw,
            )
            .unwrap();
            self.value[name] = serde_json::to_value(pin).unwrap();
        }
        fn check(&self) -> Result<Value> {
            let p: Profile = serde_json::from_value(self.value.clone())?;
            let mut source = ReadCache::new(p.byte_source.clone())?;
            prepare(&p, &mut source)
        }
        fn profile_path(&self) -> PathBuf {
            let path = self.temp.path().join("profile.json");
            fs::write(&path, serde_json::to_vec(&self.value).unwrap()).unwrap();
            path
        }
    }
    #[test]
    fn independent_targets_with_equal_bytes_preserve_assertion_provenance_without_provider_claims()
    {
        let f = Fixture::new();
        let r = f.check().unwrap();
        assert_eq!(r["files"].as_array().unwrap().len(), 2);
        assert_eq!(r["files"][0]["asset"], r["files"][1]["asset"]);
        assert_ne!(r["files"][0]["path"], r["files"][1]["path"]);
        assert_eq!(r["files"][0]["checksum_line"], 1);
        assert_eq!(r["index"], f.value["index"]);
        for key in [
            "publication_proven",
            "current_remote_content_verified",
            "producer_correspondence_verified",
            "authorizes_eviction",
            "cache_changed",
        ] {
            assert_eq!(r[key], false);
        }
        assert_eq!(r["cache_payloads_verified"], true);
    }
    #[test]
    fn metadata_only_does_not_open_or_claim_missing_payload_bytes() {
        let mut f = Fixture::new();
        fs::remove_file(&f.payload).unwrap();
        assert!(f.check().is_err());
        f.value["verify_payloads"] = json!(false);
        let p: Profile = serde_json::from_value(f.value.clone()).unwrap();
        let mut source = ReadCache::new(p.byte_source.clone()).unwrap();
        let r = prepare(&p, &mut source).unwrap();
        assert_eq!(source.requests(), 2);
        assert_eq!(r["cache_payloads_verified"], false);
        assert_eq!(r["files"][0]["cache_payload_verified"], false);
    }
    #[test]
    fn malformed_missing_duplicate_unsafe_and_conflicting_indexes_refuse() {
        for case in [
            "duplicate",
            "traversal",
            "missing",
            "escaped",
            "uppercase",
            "separator",
            "inventory-duplicate",
            "inventory-keys",
            "conflicting-length",
        ] {
            let mut f = Fixture::new();
            let digest = f.payload.file_name().unwrap().to_str().unwrap().to_owned();
            let index = match case {
                "duplicate" => Some(format!("{digest}  one.txt\n{digest}  one.txt\n")),
                "traversal" => Some(format!("{digest}  ../one.txt\n")),
                "missing" => Some(format!("{digest}  absent.txt\n")),
                "escaped" => Some(format!("\\{digest}  one.txt\n")),
                "uppercase" => Some(format!("{}  one.txt\n", digest.to_uppercase())),
                "separator" => Some(format!("{digest} one.txt\n")),
                _ => None,
            };
            if let Some(index) = index {
                f.replace("index", index.as_bytes());
            }
            match case {
                "inventory-duplicate"=>f.replace("inventory",b"{\"relative_path\":\"CRM/one.txt\",\"bytes\":3}\n{\"relative_path\":\"CRM/one.txt\",\"bytes\":3}\n"),
                "inventory-keys"=>f.replace("inventory",b"{\"relative_path\":\"CRM/one.txt\",\"bytes\":3,\"bytes\":9}\n"),
                "conflicting-length"=>{f.value["verify_payloads"]=json!(false);f.replace("inventory",b"{\"relative_path\":\"CRM/one.txt\",\"bytes\":3}\n{\"relative_path\":\"CRM/two.txt\",\"bytes\":9}\n");},
                _=>{}
            }
            assert!(f.check().is_err(), "{case}");
        }
    }
    #[test]
    fn corrupt_payload_and_read_row_output_or_metadata_budgets_refuse_without_output() {
        for case in ["corrupt", "requests", "rows", "output", "metadata"] {
            let mut f = Fixture::new();
            match case {
                "corrupt" => fs::write(&f.payload, b"bad").unwrap(),
                "requests" => f.value["byte_source"]["limits"]["requests"] = json!(2),
                "rows" => f.value["max_rows"] = json!(1),
                "output" => f.value["max_output_bytes"] = json!(1),
                "metadata" => f.value["index"]["bytes"] = json!(4194305),
                _ => unreachable!(),
            }
            let output = f.temp.path().join("refused.json");
            assert!(run(&f.profile_path(), &output).is_err(), "{case}");
            assert!(!output.exists());
        }
    }
    #[test]
    fn explicit_output_is_new_private_metadata_and_cannot_write_inside_cache() {
        let f = Fixture::new();
        let output = f.temp.path().join("association.json");
        assert_eq!(run(&f.profile_path(), &output).unwrap()["files"], 2);
        let original = fs::read(&output).unwrap();
        assert!(run(&f.profile_path(), &output).is_err());
        assert_eq!(fs::read(&output).unwrap(), original);
        let forbidden = f.payload.parent().unwrap().join("not-an-object.json");
        assert!(run(&f.profile_path(), &forbidden).is_err());
        assert!(!forbidden.exists());
    }
    #[test]
    fn output_rejects_devices_git_members_and_parent_traversal_on_every_platform() {
        let f = Fixture::new();
        fs::create_dir(f.temp.path().join(".git")).unwrap();
        for relative in [
            "NUL.json",
            "report:stream.json",
            ".git/report.json",
            "../escape.json",
        ] {
            let output = f.temp.path().join(relative);
            assert!(run(&f.profile_path(), &output).is_err(), "{relative}");
            assert!(!output.is_file());
        }
    }
}
