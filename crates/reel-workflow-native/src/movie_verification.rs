//! Original owned scratch for read-only movie comparisons. No result cache.
use super::*;
use std::io::{Seek, SeekFrom};
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ticket {
    schema: String,
    profile: Profile,
    reservation: Reservation,
    owner: Owner,
    context: Object,
}

pub(crate) fn run<T>(
    profile: &Profile,
    source: &NativeProfile,
    admission: &admission::Profile,
    context: &impl Serialize,
    execute: impl FnOnce(&Path) -> Result<T>,
) -> Result<T> {
    check_parent(&profile.parent, source, admission)?;
    let (normalized, parent) = normalize_profile(profile)?;
    let repository = source
        .snapshot
        .repository
        .canonicalize()
        .map_err(io_error)?;
    if normalized.parent.starts_with(&repository) || repository.starts_with(&normalized.parent) {
        return Err(error(
            ErrorKind::Denied,
            "movie-verification-parent-source-overlap",
        ));
    }
    let raw = json(context)?;
    let object = Object::from_bytes(&raw);
    let plan = Plan {
        manifest: Manifest {
            schema: "reel.workflow-movie-verification-intent.v1".into(),
            operation: reel_workflow::digest(&raw),
            files: BTreeMap::from([(
                "verification.json".into(),
                Entry {
                    object: object.clone(),
                    input: None,
                },
            )]),
        },
        controls: BTreeMap::new(),
    };
    let control = parent
        .path()
        .join(format!("movie-verify-{}.json", plan.manifest.operation));
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(3).custom_flags(0x0020_0000);
    }
    let (mut file, fresh) = match options.create_new(true).open(&control) {
        Ok(file) => (file, true),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            options.create_new(false);
            (options.open(&control).map_err(io_error)?, false)
        }
        Err(e) => return Err(io_error(e)),
    };
    if !ordinary(&file.metadata().map_err(io_error)?, false) {
        return Err(error(
            ErrorKind::Denied,
            "movie-verification-controller-file",
        ));
    }
    fs2::FileExt::try_lock_exclusive(&file).map_err(|e| {
        if e.kind() == std::io::ErrorKind::WouldBlock || e.raw_os_error() == Some(33) {
            error(ErrorKind::Busy, "movie-verification-owned-session-busy")
        } else {
            io_error(e)
        }
    })?;
    let ticket: Ticket = if fresh {
        let (resolved, reservation, owner) =
            prepare_owned(profile, &admission.scope.coordination_root, &plan)?;
        let ticket = Ticket {
            schema: "reel.workflow-movie-verification-ticket.v1".into(),
            profile: resolved,
            reservation,
            owner,
            context: object.clone(),
        };
        file.write_all(&json(&ticket)?)
            .and_then(|_| file.sync_all())
            .map_err(|_| {
                error(
                    ErrorKind::NeedsRecovery,
                    "movie-verification-ticket-write-preserve",
                )
            })?;
        ticket
    } else {
        file.seek(SeekFrom::Start(0)).map_err(io_error)?;
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(reel_workflow::MAX_CONTROL_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        if bytes.len() > reel_workflow::MAX_CONTROL_BYTES {
            return Err(error(ErrorKind::Budget, "movie-verification-ticket-bytes"));
        }
        reel_workflow::decode(&bytes).map_err(|_| {
            error(
                ErrorKind::NeedsRecovery,
                "movie-verification-original-ticket-required",
            )
        })?
    };
    if ticket.schema != "reel.workflow-movie-verification-ticket.v1"
        || ticket.context != object
        || json(&ticket.profile)? != json(&normalized)?
    {
        return Err(error(
            ErrorKind::Conflict,
            "movie-verification-original-owned-context",
        ));
    }
    let view = open_owner(&ticket.profile, &ticket.owner, &plan)?;
    let config = configuration(&ticket.profile, &admission.scope.coordination_root)?;
    let _reservation = capacity::resume_preserved_on(
        &config,
        &ticket.profile.parent,
        &ticket.reservation.id,
        &ticket.reservation.volume,
        ticket.profile.maximum_bytes,
    )
    .map_err(scoped_error)?;
    let context_path = view.root.path().join("verification.json");
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&context_path)
    {
        Ok(mut data) => data
            .write_all(&raw)
            .and_then(|_| data.sync_all())
            .map_err(scoped_error)?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            if read_held(&context_path, reel_workflow::MAX_CONTROL_BYTES as u64)?.0 != raw {
                return Err(error(
                    ErrorKind::Integrity,
                    "movie-verification-context-bytes",
                ));
            }
        }
        Err(e) => return Err(io_error(e)),
    }
    let check = view.root.path().join("check");
    match fs::create_dir(&check) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(io_error(e)),
    }
    for entry in fs::read_dir(view.root.path()).map_err(io_error)? {
        let name = entry.map_err(io_error)?.file_name();
        if !["intent.json", "verification.json", "check"]
            .iter()
            .any(|allowed| name == *allowed)
        {
            return Err(error(
                ErrorKind::NeedsRecovery,
                "movie-verification-foreign-owned-entry",
            ));
        }
    }
    let _check = open_workspace_directory(&check).map_err(scoped_error)?;
    execute(_check.path())
}
