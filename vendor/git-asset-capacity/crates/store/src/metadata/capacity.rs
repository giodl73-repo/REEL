//! Explicit identity-fenced capacity transitions. No node copy or deletion.
use super::*;

const MAX_PLAN: usize = 8192;
const TABLE: &str = "metadata_capacity";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Provider(super::Error),
    Stale,
    Conflict,
    /// The original operation must be inspected/retried; never replace its ID.
    Uncertain,
}
impl Error {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Stale => "stale",
            Self::Conflict => "conflict",
            Self::Uncertain => "uncertain",
            Self::Provider(super::Error::Busy) => "busy",
            Self::Provider(super::Error::Budget) => "admission_denied",
            Self::Provider(super::Error::Foreign) => "foreign",
            Self::Provider(super::Error::Corrupt) => "corrupt",
            Self::Provider(super::Error::NeedsRecovery) => "needs_recovery",
            Self::Provider(super::Error::Rollback) => "rollback",
            _ => "unavailable",
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}
impl std::error::Error for Error {}
impl From<super::Error> for Error {
    fn from(e: super::Error) -> Self {
        Self::Provider(e)
    }
}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        if let rusqlite::Error::SqliteFailure(c, _) = &e
            && matches!(
                c.code,
                rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase
            )
        {
            return Self::Provider(super::Error::Corrupt);
        }
        Self::Provider(e.into())
    }
}
impl From<std::io::Error> for Error {
    fn from(_: std::io::Error) -> Self {
        Self::Provider(super::Error::Io)
    }
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub operation: Digest,
    pub expected: Checkpoint,
    pub expected_limits: Limits,
    pub limits: Limits,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    request: Request,
    prior: Config,
    next: Config,
    id: Digest,
}
impl Plan {
    pub fn id(&self) -> &Digest {
        &self.id
    }
    pub fn operation(&self) -> &Digest {
        &self.request.operation
    }
    pub fn expected(&self) -> &Checkpoint {
        &self.request.expected
    }
    pub fn limits(&self) -> Limits {
        self.next.limits
    }
    fn build(request: Request, prior: Config) -> Result<Self> {
        request.limits.validate()?;
        prior.limits.validate()?;
        if prior.schema != "tig.metadata.design-1"
            || prior.limits != request.expected_limits
            || Digest::blob(&encode(&prior)?) != request.expected.identity
        {
            return Err(super::Error::Foreign.into());
        }
        let instance = Digest::blob(&encode(&(
            "git-asset.metadata-capacity.instance.v1",
            &request,
            &prior,
        ))?);
        let next = Config {
            schema: prior.schema.clone(),
            instance,
            limits: request.limits,
        };
        let id = Digest::blob(&encode(&(
            "git-asset.metadata-capacity.plan.v1",
            &request,
            &prior,
            &next,
        ))?);
        let plan = Self {
            request,
            prior,
            next,
            id,
        };
        if encode(&plan)?.len() > MAX_PLAN {
            return Err(super::Error::Budget.into());
        }
        Ok(plan)
    }
    fn validate(&self) -> Result<()> {
        if &Self::build(self.request.clone(), self.prior.clone())? != self {
            return Err(super::Error::Corrupt.into());
        }
        Ok(())
    }
    fn receipt(&self) -> Result<Receipt> {
        Ok(Receipt {
            plan: self.id.clone(),
            operation: self.request.operation.clone(),
            before: self.request.expected.clone(),
            after: Checkpoint {
                identity: Digest::blob(&encode(&self.next)?),
                generation: self
                    .request
                    .expected
                    .generation
                    .checked_add(1)
                    .ok_or(super::Error::Budget)?,
                nodes: self.request.expected.nodes,
                bytes: self.request.expected.bytes,
            },
            previous_limits: self.prior.limits,
            limits: self.next.limits,
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub plan: Digest,
    pub operation: Digest,
    pub before: Checkpoint,
    pub after: Checkpoint,
    pub previous_limits: Limits,
    pub limits: Limits,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preview {
    pub plan: Plan,
    pub remaining_nodes: u64,
    pub remaining_body_bytes: u64,
    pub database_bytes: u64,
    pub available_bytes: u64,
    pub reserved_capacity: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum Status {
    NotApplied,
    Pending { receipt: Receipt },
    Applied { receipt: Receipt },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    BeforeCommit,
    AfterCommit,
    BeforePublish,
    AfterPublish,
}

fn connection(root: &Path, write: bool) -> Result<Connection> {
    local::check_root_spelling(root).map_err(super::Error::from)?;
    local::directory(root).map_err(super::Error::from)?;
    local::regular(&root.join("nodes.sqlite")).map_err(super::Error::from)?;
    let flags = if write {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    let conn = Connection::open_with_flags(
        root.join("nodes.sqlite"),
        flags | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(Duration::ZERO)?;
    conn.pragma_update(None, "cache_size", -8192)?;
    if write {
        conn.pragma_update(None, "synchronous", "FULL")?;
        let mode: String = conn.pragma_query_value(None, "journal_mode", |r| r.get(0))?;
        if mode != "delete" {
            return Err(super::Error::Corrupt.into());
        }
    } else {
        conn.pragma_update(None, "query_only", true)?;
        conn.execute_batch("BEGIN")?;
    }
    Ok(conn)
}
fn config(root: &Path) -> Result<Config> {
    let c: Config = local::read_record(&root.join("metadata.json")).map_err(|e| {
        Error::Provider(match e {
            crate::Error::CorruptRecord | crate::Error::Integrity => super::Error::Corrupt,
            _ => super::Error::Io,
        })
    })?;
    c.limits.validate()?;
    if c.schema != "tig.metadata.design-1" {
        return Err(super::Error::Corrupt.into());
    }
    Ok(c)
}
fn head(conn: &Connection) -> Result<Checkpoint> {
    let (id, generation, nodes, bytes) = conn.query_row(
        "SELECT identity,generation,nodes,bytes FROM counters WHERE id=1",
        [],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                unsigned(r, 1)?,
                unsigned(r, 2)?,
                unsigned(r, 3)?,
            ))
        },
    )?;
    Ok(Checkpoint {
        identity: Digest::parse(&id).map_err(|_| super::Error::Corrupt)?,
        generation,
        nodes,
        bytes,
    })
}
fn record(conn: &Connection, column: &str, value: &Digest) -> Result<Option<Plan>> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [TABLE],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(None);
    }
    // Only internal constant column names reach this indexed, bounded lookup.
    let sql = match column {
        "operation" => {
            "SELECT length(plan),substr(plan,1,8193) FROM metadata_capacity WHERE operation=?1"
        }
        "from_identity" => {
            "SELECT length(plan),substr(plan,1,8193) FROM metadata_capacity WHERE from_identity=?1"
        }
        "to_identity" => {
            "SELECT length(plan),substr(plan,1,8193) FROM metadata_capacity WHERE to_identity=?1"
        }
        _ => return Err(super::Error::Corrupt.into()),
    };
    let row: Option<(u64, String)> = conn
        .query_row(sql, [value.as_str()], |r| Ok((unsigned(r, 0)?, r.get(1)?)))
        .optional()?;
    row.map(|(len, text)| {
        if len as usize > MAX_PLAN || text.len() > MAX_PLAN {
            return Err(super::Error::Corrupt.into());
        }
        let p: Plan = serde_json::from_slice(text.as_bytes()).map_err(|_| super::Error::Corrupt)?;
        p.validate()?;
        if encode(&p)? != text.as_bytes() {
            return Err(super::Error::Corrupt.into());
        }
        let matched = match column {
            "operation" => p.operation().clone(),
            "from_identity" => p.expected().identity.clone(),
            _ => p.receipt()?.after.identity,
        };
        if &matched != value {
            return Err(super::Error::Corrupt.into());
        }
        Ok(p)
    })
    .transpose()
}
fn state(root: &Path, conn: &Connection) -> Result<(Config, Checkpoint, bool)> {
    let h = head(conn)?;
    let c = config(root)?;
    if h.identity == Digest::blob(&encode(&c)?) {
        counters(conn, &h.identity, c.limits)?;
        return Ok((c, h, false));
    }
    let p = record(conn, "to_identity", &h.identity)?.ok_or(super::Error::Corrupt)?;
    if c != p.prior || h != p.receipt()?.after {
        return Err(super::Error::Corrupt.into());
    }
    Ok((c, h, true))
}
fn admission(root: &Path, conn: &Connection, h: &Checkpoint, limits: Limits) -> Result<(u64, u64)> {
    limits.validate()?;
    let pages: u64 = conn.pragma_query_value(None, "page_count", |r| unsigned(r, 0))?;
    let page_size: u64 = conn.pragma_query_value(None, "page_size", |r| unsigned(r, 0))?;
    if page_size != 4096 {
        return Err(super::Error::Corrupt.into());
    }
    let allocated = pages.checked_mul(page_size).ok_or(super::Error::Budget)?;
    let guard = limits
        .database_bytes
        .checked_mul(3)
        .and_then(|v| v.checked_add(limits.min_free_bytes))
        .ok_or(super::Error::Budget)?;
    let free = fs2::available_space(root)?;
    if h.nodes > limits.nodes
        || h.bytes > limits.bytes
        || allocated > limits.database_bytes
        || free < guard
    {
        return Err(super::Error::Budget.into());
    }
    Ok((allocated, free))
}
/// Read-only observation; exact expected counters/config, no body scan/reservation.
pub fn plan(root: &Path, request: Request) -> Result<Preview> {
    let conn = connection(root, false)?;
    let (c, h, pending) = state(root, &conn)?;
    if pending {
        return Err(super::Error::NeedsRecovery.into());
    }
    if h != request.expected || c.limits != request.expected_limits {
        return Err(Error::Stale);
    }
    let (database_bytes, available_bytes) = admission(root, &conn, &h, request.limits)?;
    let plan = Plan::build(request, c)?;
    if let Some(old) = record(&conn, "operation", plan.operation())?
        && old != plan
    {
        return Err(Error::Conflict);
    }
    Ok(Preview {
        remaining_nodes: plan.limits().nodes - h.nodes,
        remaining_body_bytes: plan.limits().bytes - h.bytes,
        plan,
        database_bytes,
        available_bytes,
        reserved_capacity: false,
    })
}
fn inspect(root: &Path, conn: &Connection, plan: &Plan) -> Result<Status> {
    plan.validate()?;
    let stored = record(conn, "operation", plan.operation())?;
    if stored.as_ref().is_some_and(|p| p != plan) {
        return Err(Error::Conflict);
    }
    let (_, h, pending) = state(root, conn)?;
    if stored.is_none() {
        if pending {
            return Err(super::Error::NeedsRecovery.into());
        }
        if h.identity != plan.expected().identity {
            return Err(
                if record(conn, "from_identity", &plan.expected().identity)?.is_some() {
                    Error::Stale
                } else {
                    super::Error::Foreign.into()
                },
            );
        }
        return Ok(Status::NotApplied);
    }
    let receipt = plan.receipt()?;
    if h.generation < receipt.after.generation
        || h.nodes < receipt.after.nodes
        || h.bytes < receipt.after.bytes
    {
        return Err(super::Error::Rollback.into());
    }
    if pending {
        if h != receipt.after {
            return Err(super::Error::NeedsRecovery.into());
        }
        Ok(Status::Pending { receipt })
    } else {
        Ok(Status::Applied { receipt })
    }
}
/// No recovery writes. A receipt describes the original transition, not current selection.
pub fn status(root: &Path, plan: &Plan) -> Result<Status> {
    inspect(root, &connection(root, false)?, plan)
}
/// Apply or explicitly recover the same immutable operation. Never initialize a root.
pub fn apply(root: &Path, plan: &Plan) -> Result<Receipt> {
    apply_hook(root, plan, |_| Ok(()))
}
/// Synthetic crash-boundary hooks; an error after SQL commit is uncertain.
pub fn apply_hook(
    root: &Path,
    plan: &Plan,
    mut hook: impl FnMut(Stage) -> Result<()>,
) -> Result<Receipt> {
    plan.validate()?;
    let mut conn = connection(root, true)?;
    {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        match inspect(root, &tx, plan)? {
            Status::Applied { receipt } => return Ok(receipt),
            Status::Pending { .. } => (),
            Status::NotApplied => {
                let (c, h, _) = state(root, &tx)?;
                if c != plan.prior || &h != plan.expected() {
                    return Err(Error::Stale);
                }
                admission(root, &tx, &h, plan.limits())?;
                tx.pragma_update(
                    None,
                    "max_page_count",
                    number(plan.limits().database_bytes / 4096)?,
                )?;
                let max: u64 = tx.pragma_query_value(None, "max_page_count", |r| unsigned(r, 0))?;
                if max * 4096 > plan.limits().database_bytes {
                    return Err(super::Error::Budget.into());
                }
                tx.execute_batch("CREATE TABLE IF NOT EXISTS metadata_capacity(operation TEXT PRIMARY KEY NOT NULL,from_identity TEXT UNIQUE NOT NULL,to_identity TEXT UNIQUE NOT NULL,plan TEXT NOT NULL CHECK(length(plan)<=8192));")?;
                let receipt = plan.receipt()?;
                let bytes = String::from_utf8(encode(plan)?).map_err(|_| super::Error::Corrupt)?;
                tx.execute(
                    "INSERT INTO metadata_capacity VALUES(?1,?2,?3,?4)",
                    params![
                        plan.operation().as_str(),
                        h.identity.as_str(),
                        receipt.after.identity.as_str(),
                        bytes
                    ],
                )?;
                tx.execute(
                    "UPDATE counters SET identity=?1,generation=?2 WHERE id=1",
                    params![
                        receipt.after.identity.as_str(),
                        number(receipt.after.generation)?
                    ],
                )?;
                hook(Stage::BeforeCommit)?;
                tx.commit().map_err(|_| Error::Uncertain)?;
                hook(Stage::AfterCommit).map_err(|_| Error::Uncertain)?;
            }
        }
    }
    // Reacquire the scoped writer fence before touching config: no obsolete replay
    // can publish over a later migration, even across the commit/publication gap.
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| Error::Uncertain)?;
    let result: Result<Receipt> = (|| match inspect(root, &tx, plan)? {
        Status::Applied { receipt } => Ok(receipt),
        Status::Pending { receipt } => {
            hook(Stage::BeforePublish)?;
            publish_config(root, &plan.next)?;
            hook(Stage::AfterPublish)?;
            if config(root)? != plan.next {
                return Err(super::Error::Corrupt.into());
            }
            Ok(receipt)
        }
        Status::NotApplied => Err(super::Error::Corrupt.into()),
    })();
    tx.commit().map_err(|_| Error::Uncertain)?;
    result.map_err(|_| Error::Uncertain)
}
fn publish_config(root: &Path, next: &Config) -> Result<()> {
    let mut nonce = [0; 32];
    getrandom::fill(&mut nonce).map_err(|_| super::Error::Io)?;
    let temp = root.join(format!(
        "metadata-capacity-{}.tmp",
        Digest::blob(&nonce).as_str()
    ));
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    f.write_all(&encode(next)?)?;
    f.sync_all()?;
    drop(f);
    local::regular(&root.join("metadata.json")).map_err(super::Error::from)?;
    fs::rename(temp, root.join("metadata.json"))?;
    Ok(())
}
/// Explicitly rebind old minima through bounded local lineage. Tree roots stay exact.
/// Caller must CAS/adopt the returned checkpoint and preserve the original handle.
pub fn rebind(
    root: &Path,
    historical: &Checkpoint,
    current: &Checkpoint,
    max_hops: u16,
) -> Result<Checkpoint> {
    if max_hops == 0 || max_hops > 1024 {
        return Err(super::Error::Budget.into());
    }
    let conn = connection(root, false)?;
    let (_, h, pending) = state(root, &conn)?;
    if pending {
        return Err(super::Error::NeedsRecovery.into());
    }
    if &h != current {
        return Err(Error::Stale);
    }
    let mut result = historical.clone();
    for hops in 0..=max_hops {
        if result.identity == current.identity {
            if result.generation > h.generation || result.nodes > h.nodes || result.bytes > h.bytes
            {
                return Err(super::Error::Rollback.into());
            }
            return Ok(result);
        }
        if hops == max_hops {
            return Err(super::Error::Budget.into());
        }
        let p = record(&conn, "from_identity", &result.identity)?.ok_or(super::Error::Foreign)?;
        if result.generation > p.expected().generation
            || result.nodes > p.expected().nodes
            || result.bytes > p.expected().bytes
        {
            return Err(super::Error::Rollback.into());
        }
        result.identity = p.receipt()?.after.identity;
    }
    Err(super::Error::Budget.into())
}
