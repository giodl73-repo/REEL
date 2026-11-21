//! Explicit existing-root metadata staging for native paged block selection.
use super::*;
use git_asset::source::native::retirement::paged::{self as native, Candidate};
use git_asset_store::{final_retirement::paged as selection, source_claims::age};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Age {
    registry: Source,
    clock: aged::NativeClock,
    #[serde(default)]
    limits: age::Limits,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    enrollment: native::Enrollment,
    metadata: Metadata,
    namespaces: Vec<Source>,
    #[serde(default)]
    allow_stage: bool,
    #[serde(default)]
    limits: Limits,
    #[serde(default)]
    age: Option<Age>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexRequest {
    schema: String,
    #[serde(default)]
    base: Option<Object>,
    occurrences: Vec<native::Occurrence>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectRequest {
    schema: String,
    occurrences: Object,
    filter: Filter,
    #[serde(default)]
    paging: selection::Paging,
}
impl Profile {
    fn validate(&self) -> Result<()> {
        self.enrollment.id().map_err(code)?;
        self.limits.validate().map_err(code)?;
        if self.schema != "tig.native-paged-profile.design-1"
            || !self.metadata.root.is_absolute()
            || self.metadata.minimum.identity() != &self.enrollment.metadata
            || self.namespaces.is_empty()
            || self.namespaces.len() > 16
            || self.age.is_some() != self.enrollment.age.is_some()
        {
            return Err("invalid_profile");
        }
        let mut seen = BTreeSet::new();
        for n in &self.namespaces {
            if !n.root.is_absolute() || !seen.insert(n.minimum.identity()) {
                return Err("invalid_profile");
            }
        }
        if let (Some(policy), Some(a)) = (&self.enrollment.age, &self.age) {
            a.limits.validate().map_err(|_| "budget")?;
            a.clock.validate(&policy.enrollment.clock)?;
            if !a.registry.root.is_absolute()
                || a.registry.minimum.identity() != &policy.enrollment.registry
                || seen.contains(&policy.enrollment.registry)
            {
                return Err("invalid_profile");
            }
        }
        Ok(())
    }
}
pub(super) fn run(args: &[String]) -> Result<Value> {
    if args.len() != 3 {
        return Err("usage");
    }
    let p: Profile = document(Path::new(&args[1]))?;
    p.validate()?;
    if !p.allow_stage {
        return Err("staging_disabled");
    }
    // Decode operation input before opening a writer; neither operation creates roots.
    let index = if args[0] == "index-paged" {
        let r: IndexRequest = document(Path::new(&args[2]))?;
        if r.schema != "tig.native-paged-index-request.design-1" {
            return Err("invalid_request");
        }
        Some(r)
    } else {
        None
    };
    let select = if index.is_none() {
        let r: SelectRequest = document(Path::new(&args[2]))?;
        if r.schema != "tig.native-paged-select-request.design-1" {
            return Err("invalid_request");
        }
        r.paging.validate().map_err(|_| "invalid_request")?;
        Some(r)
    } else {
        None
    };
    let session = native::Session::new(p.limits).map_err(code)?;
    let mut writer = MetadataStore::open(&p.metadata.root, &p.metadata.minimum)
        .map_err(|_| "metadata_unavailable")?;
    if writer.checkpoint().identity() != &p.enrollment.metadata {
        return Err("wrong_metadata");
    }
    if let Some(r) = index {
        let root = native::occurrences(r.base.as_ref(), r.occurrences, &mut writer, &session)
            .map_err(code)?;
        let minimum = writer.finish().map_err(|_| "metadata_unavailable")?;
        drop(writer);
        let mut reader =
            MetadataReader::open(&p.metadata.root, &minimum).map_err(|_| "metadata_unavailable")?;
        let count = native::inspect_occurrences(&root, &mut reader, &session).map_err(code)?;
        return Ok(
            json!({"schema":"tig.native-paged-index-result.design-1","occurrences":root,
            "count":count,"metadata_checkpoint":minimum,"authorizes_claim_release":false,"authorizes_removal":false}),
        );
    }
    let r = select.ok_or("invalid_request")?;
    let mut namespaces = p
        .namespaces
        .iter()
        .map(|n| Namespace::open(&n.root, &n.minimum).map_err(|_| "source_unavailable"))
        .collect::<Result<Vec<_>>>()?;
    let candidate: Candidate = if let Some(a) = &p.age {
        let mut registry = Namespace::open(&a.registry.root, &a.registry.minimum)
            .map_err(|_| "age_registry_unavailable")?;
        native::select(
            &p.enrollment,
            &r.occurrences,
            &r.filter,
            r.paging,
            &mut namespaces,
            &mut writer,
            Some(&mut native::AgeContext {
                registry: &mut registry,
                minimum: &a.registry.minimum,
                clock: &a.clock,
                limits: &a.limits,
            }),
            &session,
        )
        .map_err(code)?
    } else {
        native::select(
            &p.enrollment,
            &r.occurrences,
            &r.filter,
            r.paging,
            &mut namespaces,
            &mut writer,
            None,
            &session,
        )
        .map_err(code)?
    };
    let minimum = writer.finish().map_err(|_| "metadata_unavailable")?;
    drop(writer);
    let mut reader =
        MetadataReader::open(&p.metadata.root, &minimum).map_err(|_| "metadata_unavailable")?;
    let inspected = native::validate_current(
        &p.enrollment,
        &candidate,
        &mut namespaces,
        &mut reader,
        &session,
    )
    .map_err(code)?;
    Ok(
        json!({"schema":"tig.native-paged-result.design-1","candidate":candidate,
        "preview":inspected.summary(),"metadata_checkpoint":minimum,
        "authorizes_claim_release":false,"authorizes_removal":false}),
    )
}
