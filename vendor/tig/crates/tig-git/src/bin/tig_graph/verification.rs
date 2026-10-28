//! Point-in-time observations only; guards are released before report publication.
use super::*;
use tig_store::existing::{self, ByteSource, Protection, ReadCache};

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Selection {
    Primary,
    PrimaryAndControls,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ByteProfile {
    schema: String,
    graph_profile: Object,
    selection: Selection,
    byte_source: existing::Profile,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Outcome {
    Verified,
    Missing,
    Integrity,
    Denied,
    Busy,
    Budget,
    Unsupported,
    Io,
    Invalid,
}
impl From<existing::Error> for Outcome {
    fn from(error: existing::Error) -> Self {
        match error {
            existing::Error::Invalid => Self::Invalid,
            existing::Error::Budget => Self::Budget,
            existing::Error::Missing => Self::Missing,
            existing::Error::Integrity => Self::Integrity,
            existing::Error::Denied => Self::Denied,
            existing::Error::Busy => Self::Busy,
            existing::Error::Io => Self::Io,
            existing::Error::Unsupported => Self::Unsupported,
        }
    }
}
#[derive(Serialize)]
struct Observation {
    object: Object,
    outcome: Outcome,
    source_index: Option<usize>,
    protection: Option<Protection>,
}
#[derive(Serialize)]
struct Report<'a> {
    schema: &'static str,
    graph_profile: &'a Object,
    verification_profile: &'a Object,
    graph: View<'a>,
    selection: Selection,
    observations: Vec<Observation>,
    verified_objects: usize,
    verified_bytes: u64,
    selected_objects_verified: bool,
    complete_declared_payload_verification: bool,
    all_observations_same_instant: bool,
    protection_retained: bool,
    complete_claim_coverage: bool,
    independent_custody_verified: bool,
    authorizes_eviction: bool,
}
pub(super) fn run(graph_input: &Path, byte_input: &Path, output: &Path) -> Result<bool> {
    check_output(output)?;
    let mut raw = Vec::new();
    File::open(byte_input)?.take(65_537).read_to_end(&mut raw)?;
    let profile: ByteProfile = serde_json::from_value(parse_json(&raw)?)?;
    let resolved = resolve(graph_input)?;
    if profile.schema != "tig.graph-byte-verification-profile.experimental-1"
        || profile.graph_profile != resolved.input
    {
        return Err("verification profile does not admit this graph".into());
    }
    let required = resolved
        .requirements
        .as_ref()
        .ok_or("complete Production requirements required")?;
    let mut objects = required.primary().clone();
    if profile.selection == Selection::PrimaryAndControls {
        objects.extend(required.controls().iter().cloned());
    }
    if objects.is_empty() {
        return Err("nonempty byte selection required".into());
    }
    let limits = profile.byte_source.limits;
    let bytes = objects
        .iter()
        .try_fold(0u64, |total, object| total.checked_add(object.bytes))
        .ok_or("byte sum overflow")?;
    if objects.len() > limits.requests as usize
        || bytes > limits.total_bytes
        || objects
            .iter()
            .any(|object| object.bytes > limits.object_bytes)
    {
        return Err("whole verification plan exceeds provider allowance".into());
    }
    let input = Object::from_bytes(&raw);
    let mut report = Report {
        schema: "tig.graph-byte-verification.experimental-1",
        graph_profile: &resolved.input,
        verification_profile: &input,
        graph: View::of(&resolved),
        selection: profile.selection,
        // Reserve the longest outcome/protection and largest provider index before any payload IO.
        observations: objects
            .iter()
            .map(|object| Observation {
                object: object.clone(),
                outcome: Outcome::Unsupported,
                source_index: Some(15),
                protection: Some(Protection::CooperativeFileLock),
            })
            .collect(),
        verified_objects: objects.len(),
        verified_bytes: bytes,
        selected_objects_verified: false,
        complete_declared_payload_verification: false,
        all_observations_same_instant: false,
        protection_retained: false,
        complete_claim_coverage: false,
        independent_custody_verified: false,
        authorizes_eviction: false,
    };
    let mut bound = Bounded {
        bytes: Vec::new(),
        limit: resolved.report_limit,
    };
    serde_json::to_writer(&mut bound, &report)?;
    drop(bound);
    let mut cache = ReadCache::new(profile.byte_source)?;
    report.verified_objects = 0;
    report.verified_bytes = 0;
    report.observations.clear();
    for object in objects {
        let (outcome, source_index, protection) = match cache.open_verified(&object) {
            Ok(held) => {
                report.verified_objects += 1;
                report.verified_bytes += object.bytes;
                (
                    Outcome::Verified,
                    Some(held.place_index()),
                    Some(held.protection()),
                )
            }
            Err(error) => (Outcome::from(error), None, None),
        };
        report.observations.push(Observation {
            object,
            outcome,
            source_index,
            protection,
        });
    }
    if cache.active_readers() != 0 {
        return Err("read protection not released".into());
    }
    let complete = report.verified_objects == report.observations.len();
    report.selected_objects_verified = complete;
    report.complete_declared_payload_verification =
        complete && profile.selection == Selection::PrimaryAndControls;
    publish(&report, output, resolved.report_limit)?;
    println!(
        "{}",
        json!({"schema":"tig.graph-byte-verification-summary.experimental-1",
        "objects":report.observations.len(),"verified_objects":report.verified_objects,
        "verified_bytes":report.verified_bytes,"selected_objects_verified":complete,
        "complete_declared_payload_verification":report.complete_declared_payload_verification,
        "protection_retained":false,"independent_custody_verified":false,"authorizes_eviction":false})
    );
    Ok(complete)
}
