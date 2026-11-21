//! Private qualification component. No public seal/activation API.
use super::*;
#[cfg(test)]
use git_asset_core::Object;
const TABLE: &str = "CREATE TABLE git_asset_activation(id INTEGER PRIMARY KEY CHECK(id=1),body BLOB NOT NULL CHECK(length(body)<=65536),hash TEXT NOT NULL)";
pub(in crate::ledger) fn verify_seal(conn: &Connection, body: &[u8], hash: &Digest) -> Result<()> {
    let receipt = inspect(conn)?.ok_or(LedgerError::NeedsRecovery)?;
    if receipt.intent.bytes()? != body || &receipt.hash != hash {
        return Err(LedgerError::Corrupt);
    }
    Ok(())
}
pub(in crate::ledger) fn seal_fixture(ledger: &mut CapacityLedger, body: &[u8]) -> Result<()> {
    let intent: Intent = decode(body)?;
    seal(ledger, &intent, |_| Ok(())).map(|_| ())
}
type Intent = continuation::SealIntent;
impl Intent {
    fn bytes(&self) -> Result<Vec<u8>> {
        self.to_json().map_err(|_| LedgerError::Corrupt)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ledger) struct Receipt {
    intent: Intent,
    hash: Digest,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ledger) enum Stage {
    Checked,
    Written,
    Committed,
}
pub(in crate::ledger) fn definitions() -> BTreeMap<String, (String, String)> {
    let mut result = BTreeMap::new();
    result.insert(
        "git_asset_activation".into(),
        ("table".into(), TABLE.into()),
    );
    for table in ["events", "meta", "git_asset_activation"] {
        for action in ["INSERT", "UPDATE", "DELETE"] {
            let name = format!("git_asset_activation_{table}_{action}");
            let sql = format!(
                "CREATE TRIGGER {name} BEFORE {action} ON {table} BEGIN SELECT RAISE(ABORT,'git-asset:sealed-source'); END"
            );
            result.insert(name, ("trigger".into(), sql));
        }
    }
    result
}
fn inspect(conn: &Connection) -> Result<Option<Receipt>> {
    let mut stmt = conn.prepare(
        "SELECT length(name),length(type),length(sql),name,type,sql FROM sqlite_master WHERE name GLOB 'git_asset_activation*' LIMIT 11",
    )?;
    let mut rows = stmt.query([])?;
    let mut found = BTreeMap::new();
    while let Some(row) = rows.next()? {
        if number(row, 0)? > 128 || number(row, 1)? > 16 || number(row, 2)? > 4096 {
            return Err(LedgerError::Corrupt);
        }
        let name: String = row.get(3)?;
        let kind: String = row.get(4)?;
        let sql: Option<String> = row.get(5)?;
        if found
            .insert(name, (kind, sql.ok_or(LedgerError::Corrupt)?))
            .is_some()
        {
            return Err(LedgerError::Corrupt);
        }
    }
    if found.is_empty() {
        return Ok(None);
    }
    if found != definitions() {
        return Err(LedgerError::Corrupt);
    }
    drop(rows);
    drop(stmt);
    let mut stmt = conn.prepare(
        "SELECT id,length(body),length(hash),body,hash FROM git_asset_activation LIMIT 2",
    )?;
    let mut rows = stmt.query([])?;
    let row = rows.next()?.ok_or(LedgerError::Corrupt)?;
    if number(row, 0)? != 1 || number(row, 1)? > 65536 || number(row, 2)? != 64 {
        return Err(LedgerError::Corrupt);
    }
    let body: Vec<u8> = row.get(3)?;
    let intent: Intent = decode(&body)?;
    if intent.bytes()? != body {
        return Err(LedgerError::Corrupt);
    }
    let hash = Digest::parse(&row.get::<_, String>(4)?).map_err(|_| LedgerError::Corrupt)?;
    if hash != Digest::blob(&body) || rows.next()?.is_some() {
        return Err(LedgerError::Corrupt);
    }
    let (current, total) = head(conn)?;
    if current != intent.source || total > MAX_JOURNAL_BYTES {
        return Err(LedgerError::Corrupt);
    }
    if intent.predecessor.is_some() {
        continuation::tail::inspect_sealed_source(conn, &intent)?;
    }
    Ok(Some(Receipt { intent, hash }))
}
fn head(conn: &Connection) -> Result<(Checkpoint, u64)> {
    let (genesis, sequence, hash, bytes) = conn.query_row(
        "SELECT genesis,sequence,hash,bytes FROM meta WHERE id=1",
        [],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                number(r, 1)?,
                r.get::<_, String>(2)?,
                number(r, 3)?,
            ))
        },
    )?;
    Ok((
        Checkpoint {
            genesis: Digest::parse(&genesis).map_err(|_| LedgerError::Corrupt)?,
            sequence,
            hash: Digest::parse(&hash).map_err(|_| LedgerError::Corrupt)?,
        },
        bytes,
    ))
}
pub(in crate::ledger) fn seal(
    ledger: &mut CapacityLedger,
    intent: &Intent,
    mut hook: impl FnMut(Stage) -> Result<()>,
) -> Result<Receipt> {
    let body = intent.bytes()?;
    match &ledger.continued {
        Some(mode) => mode.check_seal(&ledger.root, intent)?,
        None if intent.predecessor.is_some() => return Err(LedgerError::InvalidReference),
        None => (),
    }
    ledger.sync()?;
    if let Some(mode) = &ledger.continued {
        mode.check_seal(&ledger.root, intent)?;
    }
    if ledger.checkpoint != intent.source
        || Digest::blob(&encode(&ledger.config)?) != intent.authority
    {
        return Err(LedgerError::Conflict);
    }
    let result = (|| {
        let tx = ledger
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (current, bytes) = head(&tx)?;
        if current != intent.source || bytes != ledger.bytes {
            return Err(LedgerError::Conflict);
        }
        if let Some(existing) = inspect(&tx)? {
            if existing.intent != *intent {
                return Err(LedgerError::Conflict);
            }
            tx.commit()?;
            return Ok(existing);
        }
        if let Some(mode) = &ledger.continued {
            mode.verify_unsealed_source(&tx)?;
        }
        hook(Stage::Checked)?;
        tx.execute(TABLE, [])?;
        let hash = Digest::blob(&body);
        tx.execute(
            "INSERT INTO git_asset_activation VALUES(1,?1,?2)",
            params![body, hash.as_str()],
        )?;
        for (kind, sql) in definitions().values() {
            if kind == "trigger" {
                tx.execute(sql, [])?;
            }
        }
        let receipt = inspect(&tx)?.ok_or(LedgerError::Corrupt)?;
        hook(Stage::Written)?;
        tx.commit()?;
        hook(Stage::Committed)?;
        Ok(receipt)
    })();
    // Even a lost reply must never leave this old handle admitting commands.
    if result.is_ok()
        || matches!(
            result,
            Err(LedgerError::Sql(_) | LedgerError::Injected | LedgerError::Corrupt)
        )
    {
        ledger.poisoned = true;
    }
    result
}
#[cfg(test)]
mod tests;
