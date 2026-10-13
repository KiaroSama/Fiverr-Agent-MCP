use crate::core::{ErrorCode, Result, error, storage_error};
use rusqlite::Transaction;

pub(super) const ROW_LIMIT: i64 = 10_000;
const JSON_LIMIT: i64 = 48 * 1024 * 1024;

pub(super) fn validate(tx: &Transaction<'_>) -> Result<()> {
    let (records, history, actions, bytes): (i64,i64,i64,i64) = tx.query_row(
        "SELECT (SELECT count(*) FROM records),(SELECT count(*) FROM history),(SELECT count(*) FROM actions),coalesce((SELECT sum(length(CAST(json AS BLOB))) FROM records),0)+coalesce((SELECT sum(length(CAST(json AS BLOB))) FROM history),0)+coalesce((SELECT sum(length(CAST(json AS BLOB))) FROM actions),0)",
        [], |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
    ).map_err(|_|storage_error())?;
    if records > ROW_LIMIT || history > ROW_LIMIT || actions > ROW_LIMIT || bytes > JSON_LIMIT {
        return Err(error(
            ErrorCode::StorageUnavailable,
            "Workspace capacity reached (10,000 rows per table or 48 MiB of record/journal JSON). New write rolled back; existing records and action journal were not deleted. Export or back up before managing retained data.",
        ));
    }
    Ok(())
}

pub(super) fn retain_history(
    tx: &Transaction<'_>,
    account: &str,
    kind: &str,
    id: &str,
) -> Result<()> {
    tx.execute("DELETE FROM history WHERE account=?1 AND kind=?2 AND id=?3 AND version NOT IN (SELECT version FROM history WHERE account=?1 AND kind=?2 AND id=?3 ORDER BY version DESC LIMIT 100)",rusqlite::params![account,kind,id]).map_err(|_|storage_error())?;
    Ok(())
}
