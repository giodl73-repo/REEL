//! Explicit source-head routing for a single enrolled local protection index.
pub mod accepted_head;
use super::*;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub index: Digest,
    pub source: Key,
    pub owner: Key,
    pub actor: Key,
    pub initial: Selection,
}
impl SourceJournal {
    pub fn coordinator(&self) -> Option<&Binding> {
        self.coordinator.as_ref()
    }
}
pub(crate) fn control(binding: &Binding) -> Result<Key> {
    Key::parse(&format!(
        "coordinator-{}",
        Digest::blob(&encode(&(
            "tig.source-transition-slot.design-1",
            &binding.index,
            &binding.source
        ))?)
        .as_str()
    ))
    .map_err(|_| Error::InvalidReference)
}
fn target(command: &Command) -> Result<Option<(&Key, bool)>> {
    Ok(match progress_budget::base(command)? {
        Command::Prepare { source, .. }
        | Command::Adopt { source, .. }
        | Command::AdoptGuarded { source, .. }
        | Command::Abort { source, .. }
        | Command::Withdraw { source, .. } => Some((source, true)),
        Command::Retire { source, .. }
        | Command::RetireGuarded { source, .. }
        | Command::Cancel { source, .. } => Some((source, false)),
        _ => None,
    })
}
pub(super) fn internal(j: &SourceJournal, command: &Command, privileged: bool) -> Result<bool> {
    if !privileged {
        return Ok(false);
    }
    let Some(binding) = &j.coordinator else {
        return Ok(false);
    };
    let control = control(binding)?;
    Ok(target(command)?.is_some_and(|(s, head)| (head && s == &binding.source) || s == &control))
}
pub(crate) struct Guard(std::fs::File);
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
pub(crate) fn lock(j: &SourceJournal) -> Result<Guard> {
    let root = j.coordinator_root.as_ref().ok_or(Error::Restricted)?;
    local::directory(&root.join("fences"))?;
    let path = root.join("fences").join("source-head");
    match fs::symlink_metadata(&path) {
        Ok(_) => local::regular(&path)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(Error::Io),
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    if file.metadata()?.len() != 0 {
        return Err(Error::Corrupt);
    }
    file.try_lock().map_err(|e| match e {
        std::fs::TryLockError::WouldBlock => Error::Busy,
        _ => Error::Io,
    })?;
    Ok(Guard(file))
}
pub(super) fn gate(
    j: &SourceJournal,
    command: &Command,
    privileged: bool,
) -> Result<Option<Guard>> {
    let Some(binding) = &j.coordinator else {
        return Ok(None);
    };
    let Some((source, head)) = target(command)? else {
        return Ok(None);
    };
    let is_control = source == &control(binding)?;
    if source != &binding.source && !is_control {
        return Ok(None);
    }
    if (head || is_control) && !privileged {
        return Err(Error::Restricted);
    }
    if privileged {
        Ok(None)
    } else {
        lock(j).map(Some)
    }
}
pub(super) fn pending_retirement(
    binding: Option<&Binding>,
    state: &State,
    command: &Command,
    privileged: bool,
) -> Result<()> {
    if !privileged && let Some(binding) = binding {
        let base = progress_budget::base(command)?;
        if matches!(base,Command::Retire {source,..}|Command::RetireGuarded {source,..} if source==&binding.source)
            && (state
                .sources
                .get(binding.source.as_str())
                .is_some_and(|s| s.pending.is_some())
                || state
                    .sources
                    .get(control(binding)?.as_str())
                    .is_some_and(|s| s.pending.is_some()))
        {
            return Err(Error::Pending);
        }
    }
    Ok(())
}
