//! Low-level still renderer for an explicitly admitted frozen job.
//! The caller owns containment, capacity, source fencing and result retention.
use super::*;
use crate::tig_inputs::ArchiveProfile;
use tig_core::{Object, tree::TreeLimits};
use tig_store::metadata::{Checkpoint, MetadataReader};

const SCHEMA: &str = "reel.tig-presentation-materialization-profile.experimental-1";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    source: ArchiveProfile,
    metadata_root: PathBuf,
    metadata_minimum: Checkpoint,
    request: Request,
    runtime: Bindings,
    max_primary_objects: usize,
    max_primary_bytes: u64,
}

fn output_path(path: &Path, protected: impl Iterator<Item = PathBuf>) -> Result<PathBuf> {
    use std::path::Component;
    if path.as_os_str().is_empty()
        || path.components().any(|part| match part {
            Component::Normal(_) => false,
            Component::Prefix(_) | Component::RootDir => !path.is_absolute(),
            _ => true,
        })
        || path.symlink_metadata().is_ok()
    {
        bail!("presentation output must be a new ordinary path");
    }
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let parent = absolute
        .parent()
        .context("presentation output parent missing")?;
    for ancestor in parent.ancestors() {
        let metadata = ancestor.symlink_metadata()?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            bail!("presentation output requires ordinary existing parents");
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                bail!("presentation output cannot enter reparse parents");
            }
        }
    }
    let parent = parent.canonicalize()?;
    for root in protected {
        match root.canonicalize() {
            Ok(root) if parent.starts_with(&root) => {
                bail!("presentation output cannot enter authority or object stores")
            }
            Ok(_) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(parent.join(
        absolute
            .file_name()
            .context("presentation output name missing")?,
    ))
}

/// Execute only inside an already admitted frozen job. No Git/source checkout,
/// setup, source adoption, publication, current-production or approval claim.
/// Every source and program guard remains held through render and full check.
pub fn materialize(profile: &Path, output: &Path) -> Result<Value> {
    let mut raw = Vec::new();
    fs::File::open(profile)?
        .take(128 * 1024 + 1)
        .read_to_end(&mut raw)?;
    if raw.len() > 128 * 1024 {
        bail!("presentation materialization profile exceeds budget");
    }
    let p: Profile = serde_json::from_value(identity::parse_json(&raw)?)?;
    if p.schema != SCHEMA
        || !p.metadata_root.is_absolute()
        || p.max_primary_objects == 0
        || p.max_primary_objects > 256
        || p.max_primary_bytes == 0
        || p.max_primary_bytes > 16 * 1024 * 1024 * 1024
    {
        bail!("invalid presentation materialization admission");
    }
    let output = output_path(
        output,
        std::iter::once(p.metadata_root.clone()).chain(
            p.source
                .byte_source
                .places
                .iter()
                .map(|place| place.root.join("objects")),
        ),
    )?;
    let mut reader = MetadataReader::open(&p.metadata_root, &p.metadata_minimum)?;
    let mut source = Source::from_archive(p.source, &mut reader, TreeLimits::default())?;
    drop(reader);
    let compiled = tig_presentation::compile(&source, &p.request)?;
    let primaries = compiled.primary_files().collect::<Vec<_>>();
    let logical_bytes = primaries.iter().try_fold(0u64, |total, pin| {
        total
            .checked_add(pin.bytes)
            .context("presentation primary byte overflow")
    })?;
    if primaries.len() > p.max_primary_objects || logical_bytes > p.max_primary_bytes {
        bail!("presentation primaries exceed explicit admission");
    }
    let programs = BoundPrograms::bind(&p.runtime)?;
    let inputs = compiled.prepare(&mut source)?;
    let provider = programs.with_inputs(&inputs);
    presentation_still_sequence::build_with_control_files(
        compiled.manifest(),
        &compiled,
        &provider,
        &output,
    )?;
    presentation_still_sequence::check_with_control_files(
        compiled.manifest(),
        &compiled,
        &provider,
        &output,
    )?;
    let observation = json!({
        "schema":"reel.tig-presentation-materialization.experimental-1",
        "profile":Object::from_bytes(&raw),
        "source_checkpoint":source.checkpoint(),"source":compiled.root(),
        "manifest":compiled.manifest(),"primary_objects":primaries.len(),
        "primary_logical_bytes":logical_bytes,"native_render_and_check":true,
        "production_current":false,"adopted":false,"approval":false,
        "original_producer_assertion":false,"eviction_authority":false,
        "complete_environment":false,"complete_semantic_coverage":false
    });
    fs::write(
        output.join("native-source.json"),
        identity::canonical_json(&observation)?,
    )?;
    Ok(observation)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_rejects_existing_paths_traversal_and_authority_children() {
        let temp = tempfile::tempdir().unwrap();
        let authority = temp.path().join("metadata");
        fs::create_dir(&authority).unwrap();
        assert!(output_path(&authority, std::iter::empty()).is_err());
        assert!(output_path(&temp.path().join("../escape"), std::iter::empty()).is_err());
        assert!(output_path(&authority.join("out"), std::iter::once(authority)).is_err());
        assert!(output_path(&temp.path().join("out"), std::iter::empty()).is_ok());
    }
}
