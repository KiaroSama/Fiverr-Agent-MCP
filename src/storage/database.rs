use super::{Root, private_permissions};
use crate::core::{
    ErrorCode, Record, Result, error, now, storage_error, validate_id, validate_kind,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::Value;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Clone)]
pub struct Store {
    pub root: Root,
    connection: Arc<Mutex<Connection>>,
}
impl Store {
    pub fn open(root: Root) -> Result<Self> {
        let path = root.safe_path(".state/workspace", "workspace.sqlite3")?;
        if !path.exists() {
            root.write_new(&path, b"")?;
        }
        private_permissions(&path)?;
        let connection = Connection::open(&path).map_err(|_| storage_error())?;
        connection
            .busy_timeout(Duration::from_secs(3))
            .map_err(|_| storage_error())?;
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(|_| storage_error())?;
        if version > 1 {
            return Err(error(
                ErrorCode::StorageUnavailable,
                "Database is from a newer application; do not reset it. Upgrade the executable.",
            ));
        }
        connection
            .execute_batch(
                "PRAGMA foreign_keys=ON; PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;",
            )
            .map_err(|_| storage_error())?;
        if version == 0 {
            connection.execute_batch("BEGIN IMMEDIATE;
            CREATE TABLE IF NOT EXISTS kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS records (account TEXT NOT NULL,kind TEXT NOT NULL,id TEXT NOT NULL,version INTEGER NOT NULL,json TEXT NOT NULL,PRIMARY KEY(account,kind,id));
            CREATE TABLE IF NOT EXISTS history (account TEXT NOT NULL,kind TEXT NOT NULL,id TEXT NOT NULL,version INTEGER NOT NULL,json TEXT NOT NULL,PRIMARY KEY(account,kind,id,version));
            CREATE TABLE IF NOT EXISTS actions (id TEXT PRIMARY KEY,state TEXT NOT NULL,json TEXT NOT NULL);
            PRAGMA user_version=1; COMMIT;").map_err(|_|storage_error())?;
        }
        Ok(Self {
            root,
            connection: Arc::new(Mutex::new(connection)),
        })
    }
    pub fn transaction<T>(
        &self,
        operation: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<T>,
    ) -> Result<T> {
        let mut connection = self.connection.lock().map_err(|_| storage_error())?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| storage_error())?;
        let result = operation(&transaction)?;
        transaction.commit().map_err(|_| storage_error())?;
        Ok(result)
    }
    pub fn value(&self, key: &str) -> Result<Option<Value>> {
        let connection = self.connection.lock().map_err(|_| storage_error())?;
        let value: Option<String> = connection
            .query_row("SELECT value FROM kv WHERE key=?1", [key], |r| r.get(0))
            .optional()
            .map_err(|_| storage_error())?;
        value
            .map(|text| serde_json::from_str(&text).map_err(|_| storage_error()))
            .transpose()
    }
    pub fn update_value<T>(
        &self,
        key: &str,
        operation: impl FnOnce(Option<Value>) -> Result<(Value, T)>,
    ) -> Result<T> {
        self.transaction(|tx|{
            let old:Option<String>=tx.query_row("SELECT value FROM kv WHERE key=?1",[key],|r|r.get(0)).optional().map_err(|_|storage_error())?;
            let old=old.map(|t|serde_json::from_str(&t).map_err(|_|storage_error())).transpose()?;
            let (new,result)=operation(old)?;
            tx.execute("INSERT INTO kv(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,new.to_string()]).map_err(|_|storage_error())?;
            Ok(result)
        })
    }
    pub fn get(&self, account: &str, kind: &str, id: &str) -> Result<Record> {
        validate_id(account)?;
        validate_kind(kind)?;
        validate_id(id)?;
        let connection = self.connection.lock().map_err(|_| storage_error())?;
        let text: Option<String> = connection
            .query_row(
                "SELECT json FROM records WHERE account=?1 AND kind=?2 AND id=?3",
                params![account, kind, id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|_| storage_error())?;
        serde_json::from_str(&text.ok_or_else(|| {
            error(
                ErrorCode::ResourceNotFound,
                "Imported record or local draft was not found in this account.",
            )
        })?)
        .map_err(|_| storage_error())
    }
    pub fn save(&self, mut record: Record, expected: Option<u64>) -> Result<Record> {
        validate_id(&record.account_id)?;
        validate_kind(&record.kind)?;
        validate_id(&record.id)?;
        record.provenance.validate()?;
        if !record.payload.is_object() || record.payload.to_string().len() > 256 * 1024 {
            return Err(crate::core::invalid(
                "Record payload must be a bounded object.",
            ));
        }
        self.transaction(|tx|{
            let old:Option<(i64,String)>=tx.query_row("SELECT version,json FROM records WHERE account=?1 AND kind=?2 AND id=?3",params![record.account_id,record.kind,record.id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|_|storage_error())?;
            if old.as_ref().map(|v|u64::try_from(v.0).map_err(|_|storage_error())).transpose()?!=expected {return Err(error(ErrorCode::ActionStateChanged,"Record changed; reload its version before saving."));}
            if let Some((version,json))=&old {tx.execute("INSERT INTO history(account,kind,id,version,json) VALUES(?1,?2,?3,?4,?5)",params![record.account_id,record.kind,record.id,version,json]).map_err(|_|storage_error())?;}
            record.version=match old{None=>1,Some((version,_))=>u64::try_from(version.checked_add(1).ok_or_else(storage_error)?).map_err(|_|storage_error())?};record.updated_at=now();
            tx.execute("INSERT INTO records(account,kind,id,version,json) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(account,kind,id) DO UPDATE SET version=excluded.version,json=excluded.json",params![record.account_id,record.kind,record.id,i64::try_from(record.version).map_err(|_|storage_error())?,serde_json::to_string(&record).map_err(|_|storage_error())?]).map_err(|_|storage_error())?;
            Ok(record)
        })
    }
    pub fn list(
        &self,
        account: &str,
        kind: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<Record>> {
        validate_id(account)?;
        validate_kind(kind)?;
        if limit == 0 || limit > 100 || offset > 10000 {
            return Err(crate::core::invalid(
                "Collection limit must be 1-100 and offset at most 10000.",
            ));
        }
        let connection = self.connection.lock().map_err(|_| storage_error())?;
        let mut statement=connection.prepare("SELECT json FROM records WHERE account=?1 AND kind=?2 ORDER BY id LIMIT ?3 OFFSET ?4").map_err(|_|storage_error())?;
        let rows = statement
            .query_map(params![account, kind, limit as i64, offset as i64], |r| {
                r.get::<_, String>(0)
            })
            .map_err(|_| storage_error())?;
        rows.map(|r| {
            serde_json::from_str(&r.map_err(|_| storage_error())?).map_err(|_| storage_error())
        })
        .collect()
    }
    pub fn page(
        &self,
        account: &str,
        kind: &str,
        after: Option<&str>,
        limit: usize,
        drafts_only: bool,
        query: Option<&str>,
    ) -> Result<(Vec<Record>, bool)> {
        validate_id(account)?;
        validate_kind(kind)?;
        if let Some(after) = after {
            validate_id(after)?;
        }
        if limit == 0 || limit > 100 {
            return Err(crate::core::invalid("Collection limit must be 1-100."));
        }
        let connection = self.connection.lock().map_err(|_| storage_error())?;
        let mut statement=connection.prepare("SELECT json FROM records WHERE account=?1 AND kind=?2 AND id>?3 AND (?4=0 OR json_extract(json,'$.draft')=1) AND (?5='' OR instr(lower(json_extract(json,'$.payload')),lower(?5))>0) ORDER BY id LIMIT ?6").map_err(|_|storage_error())?;
        let rows = statement
            .query_map(
                params![
                    account,
                    kind,
                    after.unwrap_or(""),
                    drafts_only,
                    query.unwrap_or(""),
                    (limit + 1) as i64
                ],
                |r| r.get::<_, String>(0),
            )
            .map_err(|_| storage_error())?;
        let mut records = rows
            .map(|r| {
                serde_json::from_str(&r.map_err(|_| storage_error())?).map_err(|_| storage_error())
            })
            .collect::<Result<Vec<Record>>>()?;
        let more = records.len() > limit;
        records.truncate(limit);
        Ok((records, more))
    }
    pub fn history(&self, account: &str, kind: &str, id: &str) -> Result<Vec<Record>> {
        self.get(account, kind, id)?;
        let connection = self.connection.lock().map_err(|_| storage_error())?;
        let mut statement=connection.prepare("SELECT json FROM history WHERE account=?1 AND kind=?2 AND id=?3 ORDER BY version DESC LIMIT 100").map_err(|_|storage_error())?;
        let rows = statement
            .query_map(params![account, kind, id], |r| r.get::<_, String>(0))
            .map_err(|_| storage_error())?;
        rows.map(|r| {
            serde_json::from_str(&r.map_err(|_| storage_error())?).map_err(|_| storage_error())
        })
        .collect()
    }
    pub fn import(&self, records: &[Record]) -> Result<usize> {
        if records.len() > 1000 {
            return Err(crate::core::invalid(
                "An import contains at most 1000 records.",
            ));
        }
        for record in records {
            validate_id(&record.account_id)?;
            validate_kind(&record.kind)?;
            validate_id(&record.id)?;
            record.provenance.validate()?;
            if record.provenance.source != "user_import" || !record.payload.is_object() {
                return Err(crate::core::invalid(
                    "Imported records require user_import provenance and object payloads.",
                ));
            }
        }
        self.transaction(|tx| {
            for record in records {
                let mut record = record.clone();
                record.version = 1;
                record.draft = false;
                record.updated_at = now();
                tx.execute(
                    "INSERT INTO records(account,kind,id,version,json) VALUES(?1,?2,?3,1,?4)",
                    params![
                        record.account_id,
                        record.kind,
                        record.id,
                        serde_json::to_string(&record).map_err(|_| storage_error())?
                    ],
                )
                .map_err(|_| {
                    error(
                        ErrorCode::ActionStateChanged,
                        "Import conflicts with existing records; entire import rolled back.",
                    )
                })?;
            }
            Ok(records.len())
        })
    }
    pub fn backup(&self, name: &str) -> Result<std::path::PathBuf> {
        let path = self.root.safe_path("exports", name)?;
        self.root.write_new(&path, b"")?;
        let connection = self.connection.lock().map_err(|_| storage_error())?;
        let mut destination = Connection::open(&path).map_err(|_| storage_error())?;
        let backup = rusqlite::backup::Backup::new(&connection, &mut destination)
            .map_err(|_| storage_error())?;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if std::time::Instant::now() >= deadline {
                return Err(error(
                    ErrorCode::StorageUnavailable,
                    "Backup deadline exceeded; original workspace preserved. The incomplete destination is not a verified backup.",
                ));
            }
            match backup.step(100).map_err(|_| storage_error())? {
                rusqlite::backup::StepResult::Done => break,
                rusqlite::backup::StepResult::More => {}
                _ => {
                    return Err(error(
                        ErrorCode::StorageUnavailable,
                        "Backup source or destination is busy; no automatic retry occurred.",
                    ));
                }
            }
        }
        drop(backup);
        drop(destination);
        drop(connection);
        private_permissions(&path)?;
        // A completed copy must also fit this executable's recovery contract.
        self.validate_backup(name)?;
        Ok(path)
    }
}
