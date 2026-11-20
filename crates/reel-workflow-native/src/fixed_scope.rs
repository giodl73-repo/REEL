//! Bounded workflow-profile planning over qualified exact-commit Tig metadata.
//! No payload read, admission, protection, profile installation or selection.
use super::*;
use std::time::{Duration, Instant};

impl SnapshotProfile {
    /// Narrow a new consumer's fixed record/leaf declaration to named existing
    /// logical inputs. Only explicit `.tig` file scopes are supported. Definition
    /// metadata remains declared; callers must separately bind the new upstream
    /// context and pin the resulting declaration before full native admission.
    pub fn fixed_subset(&self, required: &BTreeSet<String>) -> Result<Self> {
        if required.len() > 128 || self.records.len() > 256 || self.definitions.len() > 256 {
            return Err(error(ErrorKind::Budget, "fixed-scope-count"));
        }
        if required
            .iter()
            .any(|id| !self.bindings.contains_key(id) || !self.input_streams.contains_key(id))
        {
            return Err(error(ErrorKind::Missing, "fixed-scope-input"));
        }
        let mut result = self.clone();
        result.records.clear();
        result.bindings.retain(|id, _| required.contains(id));
        result.input_streams.retain(|id, _| required.contains(id));
        if required.is_empty() {
            result.definitions.clear();
            return Ok(result);
        }
        if self.records.is_empty() || self.records.iter().any(|path| !path.ends_with(".tig")) {
            return Err(error(
                ErrorKind::Unsupported,
                "fixed-scope-explicit-record-files-required",
            ));
        }
        let _git = hold_tool(&self.git)?;
        let repository = Repository::new(
            self.git.path.clone(),
            self.repository.clone(),
            source::Limits::default(),
        )
        .map_err(git_error)?;
        let snapshot = repository
            .commit(GitId::parse(&self.commit).map_err(git_error)?)
            .map_err(git_error)?;
        let definitions = scope(self.definitions.clone())?;
        let start = Instant::now();
        let mut total = 0usize;
        let mut found = BTreeSet::new();
        for path in &self.records {
            if start.elapsed() > Duration::from_secs(30) {
                return Err(error(ErrorKind::Budget, "fixed-scope-metadata-time"));
            }
            let assets = repository
                .native(
                    &snapshot,
                    scope(vec![path.clone()])?,
                    definitions.clone(),
                    native::Limits {
                        documents: 512,
                        bytes: 16 * 1024 * 1024,
                        millis: 30_000,
                    },
                )
                .map_err(|e| in_phase(git_error(e), "fixed-scope"))?;
            total = total
                .checked_add(assets.metadata_bytes())
                .filter(|n| *n <= 16 * 1024 * 1024)
                .ok_or_else(|| error(ErrorKind::Budget, "fixed-scope-metadata-bytes"))?;
            if assets.len() != 1 {
                return Err(error(
                    ErrorKind::Conflict,
                    "fixed-scope-single-record-required",
                ));
            }
            let mut selected = false;
            for id in required {
                if assets.resolve_id(id).is_ok() {
                    if !found.insert(id.clone()) {
                        return Err(error(ErrorKind::Conflict, "fixed-scope-record-alias"));
                    }
                    selected = true;
                }
            }
            if selected {
                result.records.push(path.clone());
            }
        }
        if start.elapsed() > Duration::from_secs(30) {
            return Err(error(ErrorKind::Budget, "fixed-scope-metadata-time"));
        }
        if &found != required {
            return Err(error(ErrorKind::Missing, "fixed-scope-record"));
        }
        result.declaration_bytes()?;
        Ok(result)
    }
}
