//! W2 sidecar inspection through exact native record-to-annex input bindings.
//! Portable control paths are request-local names; bytes come only from declared
//! logical records and their task-bound leaves, never a matching-byte search.
use crate::{InputReader, NativeInputs, error};
use reel_workflow::{Artifact, ErrorKind, Result, film_inspect};
use std::collections::BTreeMap;

struct Reader<'a> {
    commit: &'a str,
    controls: BTreeMap<&'a str, &'a Artifact>,
    inputs: &'a mut NativeInputs,
}
impl film_inspect::Reader for Reader<'_> {
    fn read(&mut self, commit: &str, path: &str, maximum: u64) -> Result<Vec<u8>> {
        if commit != self.commit {
            return Err(error(ErrorKind::Stale, "native-film-inspection-commit"));
        }
        let artifact = self
            .controls
            .get(path)
            .ok_or_else(|| error(ErrorKind::Denied, "native-film-inspection-control"))?;
        self.inputs.read(artifact, maximum)
    }
}
pub fn inspect(
    request: &film_inspect::Request,
    inputs: &mut NativeInputs,
) -> Result<film_inspect::Sidecar> {
    film_inspect::inspect(request, &mut reader(request, inputs)?)
}

pub fn reader<'a>(
    request: &'a film_inspect::Request,
    inputs: &'a mut NativeInputs,
) -> Result<impl film_inspect::Reader + 'a> {
    if request.source_commit != inputs.snapshot.packet.source_commit
        || request.project != inputs.snapshot.packet.project
    {
        return Err(error(ErrorKind::Stale, "native-film-inspection-task"));
    }
    let mut controls = BTreeMap::new();
    for control in film_inspect::controls(request) {
        if !inputs
            .snapshot
            .packet
            .task
            .inputs
            .contains(&control.artifact)
        {
            return Err(error(
                ErrorKind::Conflict,
                "native-film-inspection-declared-record",
            ));
        }
        if controls
            .insert(control.path.as_str(), &control.artifact)
            .is_some_and(|prior| prior != &control.artifact)
        {
            return Err(error(
                ErrorKind::Conflict,
                "native-film-inspection-path-alias",
            ));
        }
    }
    Ok(Reader {
        commit: &request.source_commit,
        controls,
        inputs,
    })
}
