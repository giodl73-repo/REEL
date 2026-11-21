//! Opt-in annex front door. Parsing/rendering only; no destructive passthrough.
use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};
use tig_core::LogicalPath;
use tig_git::{
    annex::{
        identity::Key,
        process::{Command, Enrollment, Remote, Runner},
        workflow,
    },
    source::{GitId, Limits, Repository, Scope},
};

fn profile(path: &str) -> io::Result<Enrollment> {
    read_profile(path)
}
fn read_profile<T: serde::de::DeserializeOwned>(path: &str) -> io::Result<T> {
    let bytes = profile_bytes(path)?;
    let value = tig_core::identity::parse_json(&bytes).map_err(|_| invalid())?;
    serde_json::from_value(value).map_err(|_| invalid())
}
fn profile_bytes(path: &str) -> io::Result<Vec<u8>> {
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    File::open(path)?.take(65_537).read_to_end(&mut bytes)?;
    if bytes.len() > 65_536 {
        return Err(invalid());
    }
    Ok(bytes)
}
fn invalid() -> io::Error {
    io::Error::other("invalid or unsupported annex request")
}
fn selection(
    enrollment: &Enrollment,
    commit: &str,
    paths: &[String],
) -> io::Result<tig_git::source::Extraction> {
    let repository = Repository::new(
        enrollment.git.clone(),
        enrollment.repository.clone(),
        Limits::default(),
    )
    .map_err(|_| invalid())?;
    let snapshot = repository
        .commit(GitId::parse(commit).map_err(|_| invalid())?)
        .map_err(|_| invalid())?;
    let scope = Scope::new(
        paths
            .iter()
            .map(|p| LogicalPath::parse(p))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| invalid())?,
    )
    .map_err(|_| invalid())?;
    repository
        .extract_annex(&snapshot, scope)
        .map_err(|_| invalid())
}
fn run(args: &[String]) -> io::Result<serde_json::Value> {
    let [action, enrollment, rest @ ..] = args else {
        return Err(invalid());
    };
    if action == "inspect-enrollment" {
        if !rest.is_empty() {
            return Err(invalid());
        }
        let bytes = profile_bytes(enrollment)?;
        let request =
            serde_json::from_value(tig_core::identity::parse_json(&bytes).map_err(|_| invalid())?)
                .map_err(|_| invalid())?;
        return serde_json::to_value(workflow::enrollment::inspect(
            &request,
            tig_core::Object::from_bytes(&bytes),
        )?)
        .map_err(|_| invalid());
    }
    if action == "read-location" {
        let [output] = rest else {
            return Err(invalid());
        };
        let bytes = profile_bytes(enrollment)?;
        let request =
            serde_json::from_value(tig_core::identity::parse_json(&bytes).map_err(|_| invalid())?)
                .map_err(|_| invalid())?;
        return workflow::reader::write_location(
            &request,
            tig_core::Object::from_bytes(&bytes),
            Path::new(output),
        );
    }
    if action == "publish-file" {
        let [output] = rest else {
            return Err(invalid());
        };
        let bytes = profile_bytes(enrollment)?;
        let value = tig_core::identity::parse_json(&bytes).map_err(|_| invalid())?;
        let publication = serde_json::from_value(value).map_err(|_| invalid())?;
        return workflow::publication::publish_file(
            &publication,
            tig_core::Object::from_bytes(&bytes),
            Path::new(output),
        );
    }
    if action == "inspect-capacity" {
        if !rest.is_empty() {
            return Err(invalid());
        }
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Profile {
            root: std::path::PathBuf,
            minimum: tig_store::source_claims::Checkpoint,
        }
        let profile: Profile = read_profile(enrollment)?;
        let mut journal =
            tig_store::source_claims::SourceJournal::open(&profile.root, &profile.minimum)
                .map_err(|_| invalid())?;
        return serde_json::to_value(journal.capacity().map_err(|_| invalid())?)
            .map_err(|_| invalid());
    }
    if action == "adopt-cache" {
        if !rest.is_empty() {
            return Err(invalid());
        }
        return serde_json::to_value(workflow::adoption::adopt(&read_profile(enrollment)?)?)
            .map_err(|_| invalid());
    }
    if action == "wake-merge" {
        if !rest.is_empty() {
            return Err(invalid());
        }
        return serde_json::to_value(workflow::merge::wake::observe(&read_profile(enrollment)?)?)
            .map_err(|_| invalid());
    }
    if action == "retire-merge-heads" {
        if !rest.is_empty() {
            return Err(invalid());
        }
        return serde_json::to_value(workflow::merge::maintenance::retire_heads(&read_profile(
            enrollment,
        )?)?)
        .map_err(|_| invalid());
    }
    if action == "init-merge" {
        if !rest.is_empty() {
            return Err(invalid());
        }
        return serde_json::to_value(workflow::merge::initialize(&read_profile(enrollment)?)?)
            .map_err(|_| invalid());
    }
    if matches!(
        action.as_str(),
        "run-merge" | "project-merge" | "prepare-merge" | "inspect-merge" | "cancel-merge"
    ) {
        if !rest.is_empty() {
            return Err(invalid());
        }
        let mode = match action.as_str() {
            "run-merge" => workflow::merge::Mode::Run,
            "project-merge" => workflow::merge::Mode::Project,
            "prepare-merge" => workflow::merge::Mode::Prepare,
            "cancel-merge" => workflow::merge::Mode::Cancel,
            _ => workflow::merge::Mode::Inspect,
        };
        return serde_json::to_value(workflow::merge::execute(&read_profile(enrollment)?, mode)?)
            .map_err(|_| invalid());
    }
    if matches!(
        action.as_str(),
        "enable-policy" | "stage-policy" | "revoke-policy"
    ) {
        if !rest.is_empty() {
            return Err(invalid());
        }
        return match action.as_str() {
            "enable-policy" => {
                serde_json::to_value(workflow::policy::enable(&read_profile(enrollment)?)?)
            }
            "stage-policy" => {
                serde_json::to_value(workflow::policy::stage(&read_profile(enrollment)?)?)
            }
            _ => serde_json::to_value(workflow::policy::revoke(&read_profile(enrollment)?)?),
        }
        .map_err(|_| invalid());
    }
    if action == "stage-retirement" {
        if !rest.is_empty() {
            return Err(invalid());
        }
        let profiles = workflow::retirement::stage(&read_profile(enrollment)?)?;
        return serde_json::to_value(profiles).map_err(|_| invalid());
    }
    if action == "init-control" {
        if !rest.is_empty() {
            return Err(invalid());
        }
        let package = workflow::control::initialize(&read_profile(enrollment)?)?;
        return serde_json::to_value(package).map_err(|_| invalid());
    }
    if action == "adopt-source" {
        if !rest.is_empty() {
            return Err(invalid());
        }
        let package = workflow::control::adopt(&read_profile(enrollment)?)?;
        return serde_json::to_value(package).map_err(|_| invalid());
    }
    if matches!(action.as_str(), "retire" | "reconcile" | "cancel") {
        if !rest.is_empty() {
            return Err(invalid());
        }
        let profile = read_profile::<workflow::retirement::Profile>(enrollment)?;
        let report = match action.as_str() {
            "retire" => workflow::retirement::apply(&profile)?,
            "reconcile" => workflow::retirement::reconcile(&profile, false)?,
            _ => workflow::retirement::reconcile(&profile, true)?,
        };
        return serde_json::to_value(report).map_err(|_| invalid());
    }
    let enrollment = profile(enrollment)?;
    let runner = Runner::new(enrollment.clone())?;
    match (action.as_str(), rest) {
        ("version", []) => {
            let output = runner.run(Command::Version)?;
            if !output.success {
                return Err(invalid());
            }
            Ok(serde_json::json!({"version":String::from_utf8_lossy(&output.stdout)}))
        }
        ("inspect", [commit, paths @ ..]) if !paths.is_empty() => {
            let extracted = selection(&enrollment, commit, paths)?;
            Ok(
                serde_json::json!({"complete":extracted.complete(), "selection":extracted.selection_digest().ok(),
                "bindings":extracted.bindings().iter().map(|b| serde_json::json!({"path":b.path.as_str(),"key":Key::from_object(&b.object).as_str(),"object":b.object})).collect::<Vec<_>>(),
                "issues":extracted.issues().iter().map(|i| serde_json::json!({"path":i.path,"kind":format!("{:?}",i.kind)})).collect::<Vec<_>>() }),
            )
        }
        ("get", [raw, source, limit]) => {
            let key = Key::parse(raw).map_err(|_| invalid())?;
            let source = Remote::parse(source)?;
            let budget: u64 = limit.parse().map_err(|_| invalid())?;
            if key.object().bytes > budget {
                return Err(invalid());
            }
            let verified = workflow::get_legacy(&runner, &key, &source, budget)?
                == workflow::TransferOutcome::VerifiedPresent;
            Ok(
                serde_json::json!({"key":key.as_str(),"outcome":if verified {"verified-present"} else {"pending"},"custody":false}),
            )
        }
        ("copy", [destination, raw, to, scratch, verify, from @ ..]) if from.len() <= 1 => {
            let destination = Runner::new(profile(destination)?)?;
            let key = Key::parse(raw).map_err(|_| invalid())?;
            let to = Remote::parse(to)?;
            let from = from.first().map(|s| Remote::parse(s)).transpose()?;
            let outcome = workflow::copy(
                &runner,
                &destination,
                &key,
                from.as_ref(),
                &to,
                scratch.parse().map_err(|_| invalid())?,
                verify.parse().map_err(|_| invalid())?,
            )?;
            Ok(
                serde_json::json!({"key":key.as_str(),"outcome":format!("{outcome:?}"),"custody":false}),
            )
        }
        ("plan", [current, old, scope]) => {
            let current = workflow::Selection::from_annex(&selection(
                &enrollment,
                current,
                std::slice::from_ref(scope),
            )?)?;
            let old = workflow::Selection::from_annex(&selection(
                &enrollment,
                old,
                std::slice::from_ref(scope),
            )?)?;
            let plan = workflow::plan(&current, &[old], vec![], &Default::default())?;
            Ok(
                serde_json::json!({"proposed":true,"coverage":"unknown","ensure":plan.ensure.iter().map(|p| serde_json::json!({"key":Key::from_object(&p.object).as_str(),"place":p.place})).collect::<Vec<_>>(),"removal_authorized":false}),
            )
        }
        // No init/drop/retire/force shortcut: these modes need their reviewed
        // enrollment and durable admitted-control implementation, not raw argv.
        _ => Err(invalid()),
    }
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() > 132 || args.iter().any(|s| s.len() > 4096) {
        eprintln!("tig-annex: request budget");
        std::process::exit(1);
    }
    match run(&args) {
        Ok(value) => println!("{value}"),
        Err(_) => {
            eprintln!("tig-annex: request refused or outcome uncertain");
            std::process::exit(1);
        }
    }
}
