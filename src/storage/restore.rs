use super::Store;
use crate::{
    core::{
        ErrorCode, Record, Result, error, hash, invalid, storage_error, validate_id, validate_kind,
    },
    network::{Mode, Settings, profiles},
    service::actions::Action,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension, config::DbConfig, params};
use serde_json::{Value, json};

struct Snapshot {
    records: Vec<Record>,
    history: Vec<Record>,
    actions: Vec<Action>,
    settings: Settings,
    sha256: String,
}
struct Staging(std::path::PathBuf);
impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

impl Store {
    pub fn validate_backup(&self, name: &str) -> Result<Value> {
        let snapshot = snapshot(self, name)?;
        Ok(
            json!({"valid":true,"schema_version":1,"sha256":snapshot.sha256,"records":snapshot.records.len(),"history":snapshot.history.len(),"actions":snapshot.actions.len(),"browser_sessions_included":false}),
        )
    }

    pub fn restore_backup(&self, name: &str) -> Result<Value> {
        self.restore_backup_checked(name, None)
    }

    pub fn restore_backup_checked(&self, name: &str, approved_hash: Option<&str>) -> Result<Value> {
        let mut snapshot = snapshot(self, name)?;
        if approved_hash.is_some_and(|expected| expected != snapshot.sha256) {
            return Err(error(
                ErrorCode::ActionStateChanged,
                "Backup changed after review; nothing restored. Review the current file again.",
            ));
        }
        self.transaction(|tx| {
            let occupied: i64 = tx.query_row("SELECT (SELECT count(*) FROM records)+(SELECT count(*) FROM history)+(SELECT count(*) FROM actions)", [], |r| r.get(0)).map_err(|_| storage_error())?;
            if occupied != 0 {
                return Err(error(ErrorCode::ActionStateChanged, "Restore requires an empty workspace; existing records, revisions and journal entries are never replaced."));
            }
            let current = Settings::in_transaction(tx)?;
            snapshot.settings.revision = current.revision.max(snapshot.settings.revision).checked_add(1).ok_or_else(storage_error)?;
            snapshot.settings.route_generation = current.route_generation.max(snapshot.settings.route_generation).checked_add(1).ok_or_else(storage_error)?;
            snapshot.settings.mode = Mode::Blocked;
            snapshot.settings.selected_profile_id = None;
            snapshot.settings.browser_executable = None;
            snapshot.settings.http_auth_mode = "token".into();
            for record in &snapshot.records {
                tx.execute("INSERT INTO records(account,kind,id,version,json) VALUES(?1,?2,?3,?4,?5)", params![record.account_id,record.kind,record.id,i64::try_from(record.version).map_err(|_|storage_error())?,serde_json::to_string(record).map_err(|_|storage_error())?]).map_err(|_|storage_error())?;
            }
            for record in &snapshot.history {
                tx.execute("INSERT INTO history(account,kind,id,version,json) VALUES(?1,?2,?3,?4,?5)", params![record.account_id,record.kind,record.id,i64::try_from(record.version).map_err(|_|storage_error())?,serde_json::to_string(record).map_err(|_|storage_error())?]).map_err(|_|storage_error())?;
            }
            for action in &mut snapshot.actions {
                match action.state.as_str() {
                    "prepared" | "approved" => { action.state = "cancelled".into(); action.approved_at = None; }
                    "executing" => action.state = "outcome_unknown".into(),
                    _ => {}
                }
                tx.execute("INSERT INTO actions(id,state,json) VALUES(?1,?2,?3)",params![action.id,action.state,serde_json::to_string(action).map_err(|_|storage_error())?]).map_err(|_|storage_error())?;
            }
            // Only settings are imported: tokens and source installation credentials never regain authority.
            tx.execute("DELETE FROM kv WHERE key='http_token'", []).map_err(|_|storage_error())?;
            tx.execute("INSERT INTO kv(key,value) VALUES('settings',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[serde_json::to_string(&snapshot.settings).map_err(|_|storage_error())?]).map_err(|_|storage_error())?;
            Ok(json!({"restored":true,"records":snapshot.records.len(),"history":snapshot.history.len(),"actions":snapshot.actions.len(),"route":"blocked","old_approvals_invalidated":true,"http_token_restored":false,"browser_sessions_restored":false}))
        })
    }
}

fn snapshot(store: &Store, name: &str) -> Result<Snapshot> {
    let source = store.root.safe_path("exports", name)?;
    let bytes = store.root.read_bounded(&source, 64 * 1024 * 1024)?;
    if !bytes.starts_with(b"SQLite format 3\0") {
        return Err(invalid("Backup is not a SQLite database."));
    }
    // Freeze the bounded input once; a concurrent replacement cannot change rows after validation.
    let path = store.root.safe_path(
        ".cache/tmp",
        &format!("restore-{}.sqlite3", uuid::Uuid::new_v4()),
    )?;
    store.root.write_new(&path, &bytes)?;
    let staging = Staging(path);
    let connection = Connection::open_with_flags(
        &staging.0,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| invalid("Backup cannot be opened; original workspace preserved."))?;
    for (limit, maximum) in [
        (rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH, 1024 * 1024),
        (rusqlite::limits::Limit::SQLITE_LIMIT_SQL_LENGTH, 64 * 1024),
        (rusqlite::limits::Limit::SQLITE_LIMIT_COLUMN, 32),
        (rusqlite::limits::Limit::SQLITE_LIMIT_WORKER_THREADS, 0),
    ] {
        connection
            .set_limit(limit, maximum)
            .map_err(|_| storage_error())?;
    }
    connection
        .set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)
        .map_err(|_| storage_error())?;
    connection
        .set_db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false)
        .map_err(|_| storage_error())?;
    connection
        .busy_timeout(std::time::Duration::from_secs(1))
        .map_err(|_| storage_error())?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut operations = 0_u32;
    connection
        .progress_handler(
            1000,
            Some(move || {
                operations += 1;
                operations > 50_000 || std::time::Instant::now() >= deadline
            }),
        )
        .map_err(|_| storage_error())?;
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|_| invalid("Backup schema is unreadable."))?;
    if version != 1 {
        return Err(invalid(
            "Backup schema version is unsupported; preserve it and use a compatible executable.",
        ));
    }
    let mut objects = connection.prepare("SELECT type,name FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY name LIMIT 10").map_err(|_|invalid("Backup schema is invalid."))?;
    let objects = objects
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|_| storage_error())?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| storage_error())?;
    let expected =
        ["actions", "history", "kv", "records"].map(|name| ("table".to_owned(), name.to_owned()));
    if objects != expected {
        return Err(invalid(
            "Backup must contain only the supported ordinary workspace tables, without views or triggers.",
        ));
    }
    for (table, columns) in [
        ("kv", "key,value"),
        ("records", "account,kind,id,version,json"),
        ("history", "account,kind,id,version,json"),
        ("actions", "id,state,json"),
    ] {
        let table_type: String = connection
            .query_row(
                "SELECT type FROM pragma_table_list WHERE schema='main' AND name=?1",
                [table],
                |r| r.get(0),
            )
            .map_err(|_| invalid("Backup table type is invalid."))?;
        if table_type != "table" {
            return Err(invalid("Virtual/shadow backup tables are refused."));
        }
        let actual: String = connection
            .query_row(
                "SELECT group_concat(name,',') FROM pragma_table_info(?1)",
                [table],
                |r| r.get(0),
            )
            .map_err(|_| invalid("Backup table schema is invalid."))?;
        if actual != columns {
            return Err(invalid(
                "Backup table columns do not match this application.",
            ));
        }
    }
    let integrity: String = connection
        .query_row("PRAGMA integrity_check(1)", [], |r| r.get(0))
        .map_err(|_| invalid("Backup integrity check failed."))?;
    if integrity != "ok" {
        return Err(invalid(
            "Backup integrity check failed; original workspace preserved.",
        ));
    }
    let current_records = records(&connection, "records")?;
    let history = records(&connection, "history")?;
    let versions: std::collections::BTreeMap<_, _> = current_records
        .iter()
        .map(|record| {
            (
                (
                    record.account_id.as_str(),
                    record.kind.as_str(),
                    record.id.as_str(),
                ),
                record.version,
            )
        })
        .collect();
    for record in &history {
        if versions
            .get(&(
                record.account_id.as_str(),
                record.kind.as_str(),
                record.id.as_str(),
            ))
            .is_none_or(|current| record.version >= *current)
        {
            return Err(invalid(
                "Backup history must belong to a current record and precede its version.",
            ));
        }
    }
    let mut statement = connection
        .prepare("SELECT id,state,json FROM actions LIMIT 10001")
        .map_err(|_| storage_error())?;
    let rows = statement
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(|_| storage_error())?;
    let mut actions = Vec::new();
    for row in rows {
        let (id, state, text) = row.map_err(|_| storage_error())?;
        let action: Action =
            serde_json::from_str(&text).map_err(|_| invalid("Backup action is malformed."))?;
        validate_id(&action.id)?;
        validate_id(&action.account_id)?;
        validate_id(&action.target_id)?;
        validate_kind(&action.kind)?;
        if actions.len() >= 10000
            || action.id != id
            || action.state != state
            || ![
                "prepared",
                "approved",
                "executing",
                "succeeded",
                "failed",
                "outcome_unknown",
                "cancelled",
                "manual_handoff",
            ]
            .contains(&state.as_str())
            || hash(action.payload.to_string().as_bytes()) != action.payload_hash
            || !action.payload.is_object()
            || action.payload.to_string().len() > 256 * 1024
            || action.files.len() > 10
        {
            return Err(invalid(
                "Backup journal binding, state or limit is invalid.",
            ));
        }
        actions.push(action);
    }
    let settings: Option<String> = connection
        .query_row("SELECT value FROM kv WHERE key='settings'", [], |r| {
            r.get(0)
        })
        .optional()
        .map_err(|_| storage_error())?;
    let settings: Settings = settings
        .map(|text| {
            serde_json::from_str(&text).map_err(|_| invalid("Backup settings are malformed."))
        })
        .transpose()?
        .unwrap_or_default();
    validate_id(&settings.account_id)?;
    if settings.revision >= u64::MAX - 1 || settings.route_generation >= u64::MAX - 1 {
        return Err(invalid(
            "Backup settings counters are exhausted; original workspace preserved.",
        ));
    }
    settings.effective_port(None, None)?;
    if settings.profiles.len() > 100 {
        return Err(invalid("Backup proxy profile limit exceeded."));
    }
    for profile in &settings.profiles {
        profiles::normalize(&profile.url)?;
    }
    Ok(Snapshot {
        records: current_records,
        history,
        actions,
        settings,
        sha256: hash(&bytes),
    })
}

fn records(connection: &Connection, table: &str) -> Result<Vec<Record>> {
    let sql = match table {
        "records" => "SELECT account,kind,id,version,json FROM records LIMIT 10001",
        "history" => "SELECT account,kind,id,version,json FROM history LIMIT 10001",
        _ => return Err(storage_error()),
    };
    let mut statement = connection.prepare(sql).map_err(|_| storage_error())?;
    let rows = statement
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
            ))
        })
        .map_err(|_| storage_error())?;
    let mut records = Vec::new();
    for row in rows {
        let (account, kind, id, version, text) = row.map_err(|_| storage_error())?;
        let record: Record =
            serde_json::from_str(&text).map_err(|_| invalid("Backup record is malformed."))?;
        validate_id(&record.account_id)?;
        validate_id(&record.id)?;
        validate_kind(&record.kind)?;
        record.provenance.validate()?;
        if records.len() >= 10000
            || version <= 0
            || record.account_id != account
            || record.kind != kind
            || record.id != id
            || record.version != u64::try_from(version).map_err(|_| storage_error())?
            || !record.payload.is_object()
            || record.payload.to_string().len() > 256 * 1024
        {
            return Err(invalid("Backup record binding or limit is invalid."));
        }
        records.push(record);
    }
    Ok(records)
}
