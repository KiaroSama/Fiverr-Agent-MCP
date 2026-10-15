use crate::{
    core::{Result, invalid, storage_error},
    storage::{Root, Store},
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct State {
    pub revision: u64,
    pub mode: String,
    pub account: String,
    pub generation: u64,
    pub username: String,
    pub extension_id: Option<String>,
    pub observed_at: Option<String>,
    pub state: String,
}
// This separate store is deliberately outside workspace backup/restore and exports.
// Never derive Debug/Serialize for the containing credential tuple or return it to MCP.
fn connection(root: &Root) -> Result<Connection> {
    let path = root.safe_path(".state/browser", "sessions.sqlite3")?;
    if !path.exists() {
        match root.write_new(&path, b"") {
            Ok(()) => {}
            Err(_) if path.is_file() => {}
            Err(e) => return Err(e),
        }
    }
    let connection = Connection::open(path).map_err(|_| storage_error())?;
    connection
        .busy_timeout(Duration::from_secs(3))
        .map_err(|_| storage_error())?;
    connection.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA secure_delete=ON; PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF; CREATE TABLE IF NOT EXISTS session (id INTEGER PRIMARY KEY CHECK(id=1), metadata TEXT NOT NULL, cookie TEXT);").map_err(|_| storage_error())?;
    connection
        .prepare("SELECT id,metadata,cookie FROM session LIMIT 0")
        .map_err(|_| storage_error())?;
    Ok(connection)
}
pub(super) fn load(store: &Store) -> Result<Option<(State, Option<String>)>> {
    let connection = connection(&store.root)?;
    let row: Option<(String, Option<String>)> = connection
        .query_row("SELECT metadata,cookie FROM session WHERE id=1", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()
        .map_err(|_| storage_error())?;
    let state: Option<(State, Option<String>)> = row
        .map(|(text, cookie)| {
            Ok((
                serde_json::from_str(&text).map_err(|_| storage_error())?,
                cookie,
            ))
        })
        .transpose()?;
    Ok(state.filter(|(s, _)| s.state != "forgotten"))
}
pub(super) fn replace(store: &Store, mut state: State, cookie: Option<&str>) -> Result<()> {
    let mut connection = connection(&store.root)?;
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| storage_error())?;
    let old: Option<String> = tx
        .query_row("SELECT metadata FROM session WHERE id=1", [], |r| r.get(0))
        .optional()
        .map_err(|_| storage_error())?;
    state.revision = old
        .map(|text| serde_json::from_str::<State>(&text).map_err(|_| storage_error()))
        .transpose()?
        .map_or(Some(1), |s| s.revision.checked_add(1))
        .ok_or_else(|| invalid("Session revision exhausted; original session preserved."))?;
    tx.execute("INSERT INTO session(id,metadata,cookie) VALUES(1,?1,?2) ON CONFLICT(id) DO UPDATE SET metadata=excluded.metadata,cookie=excluded.cookie",params![serde_json::to_string(&state).map_err(|_|storage_error())?,cookie]).map_err(|_|storage_error())?;
    tx.commit().map_err(|_| storage_error())
}
pub(super) fn record(store: &Store, revision: u64, state: &str) -> Result<()> {
    let mut connection = connection(&store.root)?;
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| storage_error())?;
    let text: String = tx
        .query_row("SELECT metadata FROM session WHERE id=1", [], |r| r.get(0))
        .map_err(|_| storage_error())?;
    let mut saved: State = serde_json::from_str(&text).map_err(|_| storage_error())?;
    if saved.revision != revision {
        return Err(invalid(
            "Session changed during verification; result discarded.",
        ));
    }
    saved.revision = saved.revision.checked_add(1).ok_or_else(storage_error)?;
    saved.state = state.into();
    saved.observed_at = (state == "account_observed").then(crate::core::now);
    tx.execute(
        "UPDATE session SET metadata=?1 WHERE id=1",
        [serde_json::to_string(&saved).map_err(|_| storage_error())?],
    )
    .map_err(|_| storage_error())?;
    tx.commit().map_err(|_| storage_error())
}
pub(super) fn forget(store: &Store) -> Result<()> {
    let mut connection = connection(&store.root)?;
    connection
        .pragma_update(None, "secure_delete", true)
        .map_err(|_| storage_error())?;
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| storage_error())?;
    let text: Option<String> = tx
        .query_row("SELECT metadata FROM session WHERE id=1", [], |r| r.get(0))
        .optional()
        .map_err(|_| storage_error())?;
    if let Some(text) = text {
        let mut saved: State = serde_json::from_str(&text).map_err(|_| storage_error())?;
        saved.revision = saved.revision.checked_add(1).ok_or_else(storage_error)?;
        saved.state = "forgotten".into();
        saved.username.clear();
        saved.extension_id = None;
        saved.observed_at = None;
        tx.execute(
            "UPDATE session SET metadata=?1,cookie=NULL WHERE id=1",
            [serde_json::to_string(&saved).map_err(|_| storage_error())?],
        )
        .map_err(|_| storage_error())?;
    }
    tx.commit().map_err(|_| storage_error())
}
