//! Private local completion receipts; never stores replayable arguments or credentials.
use super::{Store, private_permissions};
use crate::core::{ErrorCode, Result, envelope, error, now, storage_error, validate_id};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};

#[cfg(test)]
#[path = "operation_tests.rs"]
mod tests;

const PREPARING: u8 = 0;
const ADMITTED: u8 = 1;
const EXPIRED: u8 = 2;
struct Admission(Arc<AtomicU8>);
impl Drop for Admission {
    fn drop(&mut self) {
        let _ = self
            .0
            .compare_exchange(PREPARING, EXPIRED, Ordering::AcqRel, Ordering::Acquire);
    }
}

fn not_started() -> crate::core::AppError {
    error(
        ErrorCode::StorageUnavailable,
        "Workspace request expired before starting; no operation executed.",
    )
}

fn journal(store: &Store) -> Result<Connection> {
    let path = store.root.safe_path(".state/audit", "operations.sqlite3")?;
    if !path.exists() {
        store.root.write_new(&path, b"")?;
        private_permissions(&path)?;
    }
    let connection = Connection::open(path).map_err(|_| storage_error())?;
    connection
        .busy_timeout(Duration::from_secs(1))
        .map_err(|_| storage_error())?;
    connection.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; CREATE TABLE IF NOT EXISTS operations(id TEXT PRIMARY KEY,account TEXT NOT NULL,tool TEXT NOT NULL,state TEXT NOT NULL,created TEXT NOT NULL,completed TEXT,result TEXT);").map_err(|_|storage_error())?;
    Ok(connection)
}
fn mutation_lock(store: &Store) -> Result<std::fs::File> {
    let path = store.root.safe_path(".state/audit", "operations.lock")?;
    if !path.exists()
        && let Err(error) = store.root.write_new(&path, b"")
    {
        if !path.is_file() {
            return Err(error);
        }
    }
    store.root.assert_owned(&path)?;
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|_| storage_error())
}
fn recover(connection: &Connection) -> Result<()> {
    connection
        .execute(
            "UPDATE operations SET state='outcome_unknown' WHERE state='executing'",
            [],
        )
        .map_err(|_| storage_error())?;
    Ok(())
}
fn begin(connection: &mut Connection, id: &str, account: &str, tool: &str) -> Result<()> {
    recover(connection)?;
    let tx = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|_| storage_error())?;
    let count: i64 = tx
        .query_row("SELECT count(*) FROM operations", [], |r| r.get(0))
        .map_err(|_| storage_error())?;
    if count >= 10000 {
        return Err(error(
            ErrorCode::StorageUnavailable,
            "Operation receipt capacity reached; preserve receipts before maintenance. No work started.",
        ));
    }
    tx.execute(
        "INSERT INTO operations(id,account,tool,state,created) VALUES(?1,?2,?3,'executing',?4)",
        params![id, account, tool, now()],
    )
    .map_err(|_| storage_error())?;
    tx.commit().map_err(|_| storage_error())
}
fn complete(connection: &Connection, id: &str, result: &Result<Value>) -> Result<()> {
    // Persist the outcome, not a second copy of private draft/action payloads.
    let (state, value) = match result {
        Ok(value) => {
            let data = &value["data"];
            let mut summary = serde_json::Map::new();
            for field in [
                "id", "kind", "version", "state", "file", "format", "bytes", "sha256", "count",
                "imported",
            ] {
                if let Some(value) = data.get(field).filter(|value| {
                    value.is_boolean()
                        || value.is_number()
                        || value.as_str().is_some_and(|text| text.len() <= 256)
                }) {
                    summary.insert(field.into(), value.clone());
                }
            }
            ("succeeded", json!({"ok": true, "summary": summary}))
        }
        Err(e) => ("failed", json!({"error": {"code": e.code.as_str()}})),
    };
    if connection.execute("UPDATE operations SET state=?2,completed=?3,result=?4 WHERE id=?1 AND state='executing'",params![id,state,now(),value.to_string()]).map_err(|_|storage_error())?!=1{return Err(storage_error())}
    Ok(())
}
impl Store {
    pub async fn dispatch(&self, name: String, args: Value) -> Result<Value> {
        if args.to_string().len() > 1024 * 1024 {
            return Err(crate::core::invalid("Tool arguments exceed 1 MiB."));
        }
        crate::server::validation::arguments(&name, &args)?;
        if crate::server::catalog::TOOLS
            .iter()
            .any(|t| t.0 == name && t.3)
        {
            return self
                .run(move |store| crate::service::dispatch(store, &name, args))
                .await;
        }
        self.dispatch_write(
            name.clone(),
            move |store| crate::service::dispatch(store, &name, args),
            Duration::from_secs(10),
        )
        .await
    }

    async fn dispatch_write(
        &self,
        name: String,
        operation: impl FnOnce(&Store) -> Result<Value> + Send + 'static,
        timeout: Duration,
    ) -> Result<Value> {
        let id = uuid::Uuid::new_v4().to_string();
        let operation_id = id.clone();
        // A deadline may expire while admission is still waiting on storage. Only
        // advertise a pending ID after the receipt exists and the effect is admitted.
        let admission = Admission(Arc::new(AtomicU8::new(PREPARING)));
        let worker_admission = admission.0.clone();
        let pending=self.worker()?.submit(move|store|{
            let lock = mutation_lock(store)?;
            fs2::FileExt::try_lock_exclusive(&lock).map_err(|_| error(ErrorCode::StorageUnavailable, "Another process owns workspace operation completion; no work started."))?;
            let account=crate::network::Settings::load(store)?.account_id;
            let mut connection=journal(store)?;
            begin(&mut connection,&operation_id,&account,&name)?;
            if worker_admission.compare_exchange(PREPARING, ADMITTED, Ordering::AcqRel, Ordering::Acquire).is_err() {
                connection.execute("DELETE FROM operations WHERE id=?1 AND state='executing'", [&operation_id]).map_err(|_| storage_error())?;
                return Err(not_started());
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| operation(store)));
            if let Ok(result) = result && complete(&connection,&operation_id,&result).is_ok() {
                return result;
            }
            Ok(envelope(json!({"operation_id":operation_id,"state":"outcome_unknown","automatic_replay":false}),json!({"source":"local_operation_receipt"})))
        })?;
        match tokio::time::timeout(timeout, pending.wait()).await {
            Ok(result) => result,
            Err(_) => {
                if admission
                    .0
                    .compare_exchange(PREPARING, EXPIRED, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    return Err(not_started());
                }
                Ok(envelope(
                    json!({"operation_id":id,"state":"pending","automatic_replay":false}),
                    json!({"source":"local_operation_receipt"}),
                ))
            }
        }
    }
    pub(crate) fn operation(&self, id: &str) -> Result<Value> {
        validate_id(id)?;
        let account = crate::network::Settings::load(self)?.account_id;
        let path = self.root.safe_path(".state/audit", "operations.sqlite3")?;
        if !path.is_file() {
            return Err(error(
                ErrorCode::ResourceNotFound,
                "Operation receipt not found in the selected account.",
            ));
        }
        let lock_path = self.root.safe_path(".state/audit", "operations.lock")?;
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(|_| storage_error())?;
        // Only the OS can distinguish a live writer from a stopped process.
        let idle = match fs2::FileExt::try_lock_exclusive(&lock) {
            Ok(()) => true,
            Err(e) if e.raw_os_error() == fs2::lock_contended_error().raw_os_error() => false,
            Err(_) => return Err(storage_error()),
        };
        let connection =
            Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(|_| storage_error())?;
        connection
            .busy_timeout(Duration::from_secs(1))
            .map_err(|_| storage_error())?;
        type ReceiptRow = (String, String, String, Option<String>, Option<String>);
        let row: Option<ReceiptRow> = connection.query_row("SELECT tool,state,created,completed,result FROM operations WHERE id=?1 AND account=?2",params![id,account],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(|_|storage_error())?;
        let (tool, state, created, completed, result) = row.ok_or_else(|| {
            error(
                ErrorCode::ResourceNotFound,
                "Operation receipt not found in the selected account.",
            )
        })?;
        let state = match state.as_str() {
            "executing" if idle => "outcome_unknown",
            "executing" | "outcome_unknown" | "succeeded" | "failed" => state.as_str(),
            _ => return Err(storage_error()),
        };
        let result = result
            .map(|text| serde_json::from_str::<Value>(&text).map_err(|_| storage_error()))
            .transpose()?;
        Ok(envelope(
            json!({"operation_id":id,"tool":tool,"state":state,"created_at":created,"completed_at":completed,"completion":result,"automatic_replay":false}),
            json!({"source":"local_operation_receipt"}),
        ))
    }
}
