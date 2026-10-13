use fiverr_agent_mcp::{
    core::{ErrorCode, Provenance, Record, error},
    storage::{Root, Store, logging::Logger},
};
use serde_json::{Value, json};
use std::{fs, fs::OpenOptions};

fn workspace() -> (tempfile::TempDir, Store) {
    let temporary = tempfile::tempdir().unwrap();
    let root = Root::explicit(&temporary.path().canonicalize().unwrap()).unwrap();
    (temporary, Store::open(root).unwrap())
}
fn record(id: &str) -> Record {
    Record {
        id: id.into(),
        kind: "note".into(),
        account_id: "local".into(),
        version: 0,
        draft: true,
        payload: json!({"body":"Preserved"}),
        provenance: Provenance {
            source: "local_draft".into(),
            observed_at: "2026-01-01T00:00:00Z".into(),
            uri: None,
            partial: false,
        },
        updated_at: "2026-01-01T00:00:00Z".into(),
    }
}
fn sql(store: &Store, statement: &str) {
    store
        .transaction(|tx| {
            tx.execute_batch(statement).unwrap();
            Ok(())
        })
        .unwrap();
}

#[test]
fn root_reads_are_bounded_and_new_writes_never_touch_outside_or_replace_files() {
    let (_temporary, store) = workspace();
    let root = &store.root;
    let path = root.safe_path("exports", "nested/فارسی.txt").unwrap();
    root.write_new(&path, "فارسی".as_bytes()).unwrap();
    assert_eq!(root.read_bounded(&path, 10).unwrap(), "فارسی".as_bytes());
    assert_eq!(
        root.read_bounded(&path, 9).unwrap_err().code,
        ErrorCode::FilePolicyViolation
    );
    assert_eq!(
        root.read_bounded(&root.path().join("exports/missing"), 1)
            .unwrap_err()
            .code,
        ErrorCode::ResourceNotFound
    );
    assert_eq!(
        root.read_bounded(&root.path().join("exports"), 1)
            .unwrap_err()
            .code,
        ErrorCode::FilePolicyViolation
    );
    assert_eq!(
        root.safe_path("unapproved", "a").unwrap_err().code,
        ErrorCode::FilePolicyViolation
    );
    for name in [
        "a//b",
        "a/./b",
        "a/",
        "",
        "LPT9.txt",
        "COM0",
        "trailing.",
        "trailing ",
        "a?b",
        "a\u{7}b",
    ] {
        assert_eq!(
            root.safe_path("exports", name).unwrap_err().code,
            ErrorCode::FilePolicyViolation,
            "{name:?}"
        );
    }
    assert_eq!(
        root.write_new(&path, b"replace").unwrap_err().code,
        ErrorCode::StorageUnavailable
    );
    assert_eq!(fs::read(&path).unwrap(), "فارسی".as_bytes());
    let outside = root
        .path()
        .parent()
        .unwrap()
        .join("never-created-by-storage-boundaries.txt");
    assert_eq!(
        root.assert_owned(&outside).unwrap_err().code,
        ErrorCode::FilePolicyViolation
    );
    assert_eq!(
        root.write_new(&outside, b"refused").unwrap_err().code,
        ErrorCode::FilePolicyViolation
    );
    assert_eq!(
        root.read_bounded(&outside, 100).unwrap_err().code,
        ErrorCode::FilePolicyViolation
    );
    let blocker = root.safe_path("exports", "blocker").unwrap();
    root.write_new(&blocker, b"file").unwrap();
    assert_eq!(
        root.write_new(&blocker.join("child"), b"refused")
            .unwrap_err()
            .code,
        ErrorCode::StorageUnavailable
    );
    assert_eq!(fs::read(blocker).unwrap(), b"file");
    assert_eq!(
        Root::explicit(std::path::Path::new("relative"))
            .unwrap_err()
            .code,
        ErrorCode::FilePolicyViolation
    );
    assert_eq!(
        Root::explicit(&root.path().join("absent-root"))
            .unwrap_err()
            .code,
        ErrorCode::StorageUnavailable
    );
    assert_eq!(
        Root::discover(Some(root.path())).unwrap().path(),
        root.path()
    );
}

#[cfg(unix)]
#[test]
fn root_rejects_internal_and_dangling_links_before_reads_or_writes() {
    let (_temporary, store) = workspace();
    let original = store.root.safe_path("exports", "original").unwrap();
    store.root.write_new(&original, b"Keep").unwrap();
    for (name, destination) in [
        ("internal", original.clone()),
        ("dangling", store.root.path().join("absent")),
    ] {
        let link = store.root.path().join("exports").join(name);
        std::os::unix::fs::symlink(destination, &link).unwrap();
        assert_eq!(
            store.root.assert_owned(&link).unwrap_err().code,
            ErrorCode::FilePolicyViolation
        );
        assert_eq!(
            store.root.read_bounded(&link, 10).unwrap_err().code,
            ErrorCode::FilePolicyViolation
        );
        assert_eq!(
            store.root.write_new(&link, b"No").unwrap_err().code,
            ErrorCode::FilePolicyViolation
        );
    }
    assert_eq!(fs::read(original).unwrap(), b"Keep");
    let link = store.root.path().join("linked-root");
    std::os::unix::fs::symlink(store.root.path(), &link).unwrap();
    assert_eq!(
        Root::explicit(&link).unwrap_err().code,
        ErrorCode::FilePolicyViolation
    );
}

#[test]
fn opening_newer_corrupt_or_invalid_schema_preserves_database_bytes() {
    for corruption in ["newer", "corrupt", "schema"] {
        let temporary = tempfile::tempdir().unwrap();
        let root = Root::explicit(&temporary.path().canonicalize().unwrap()).unwrap();
        let path = root
            .safe_path(".state/workspace", "workspace.sqlite3")
            .unwrap();
        root.write_new(&path, b"").unwrap();
        if corruption == "corrupt" {
            fs::write(&path, b"not a sqlite database").unwrap();
        } else {
            let connection = rusqlite::Connection::open(&path).unwrap();
            connection
                .execute_batch(if corruption == "newer" {
                    "PRAGMA user_version=2; CREATE TABLE preserved(value);"
                } else {
                    "CREATE TABLE preserved(value); CREATE INDEX records ON preserved(value);"
                })
                .unwrap();
        }
        let before = fs::read(&path).unwrap();
        let code = match Store::open(root) {
            Ok(_) => panic!("accepted {corruption}"),
            Err(e) => e.code,
        };
        assert_eq!(code, ErrorCode::StorageUnavailable, "{corruption}");
        assert_eq!(fs::read(path).unwrap(), before, "{corruption}");
    }
}

#[test]
fn failed_transactions_and_value_callbacks_rollback_without_poisoning_store() {
    let (_temporary, store) = workspace();
    store
        .update_value("counter", |_| Ok((json!(7), ())))
        .unwrap();
    let failure: fiverr_agent_mcp::core::Result<()> = store.transaction(|tx| {
        tx.execute("UPDATE kv SET value='8' WHERE key='counter'", [])
            .unwrap();
        Err(error(ErrorCode::InvalidInput, "Controlled rollback"))
    });
    assert_eq!(failure.unwrap_err().code, ErrorCode::InvalidInput);
    assert_eq!(store.value("counter").unwrap(), Some(json!(7)));
    let failure: fiverr_agent_mcp::core::Result<()> = store.update_value("counter", |_| {
        Err(error(ErrorCode::PermissionDenied, "Refused update"))
    });
    assert_eq!(failure.unwrap_err().code, ErrorCode::PermissionDenied);
    assert_eq!(store.value("counter").unwrap(), Some(json!(7)));
    // Deferred FK failure exercises commit failure, not merely a statement error.
    sql(
        &store,
        "CREATE TABLE parent(id INTEGER PRIMARY KEY); CREATE TABLE child(id INTEGER REFERENCES parent(id) DEFERRABLE INITIALLY DEFERRED);",
    );
    let failure = store.transaction(|tx| {
        tx.execute("INSERT INTO child VALUES(1)", []).unwrap();
        tx.execute("UPDATE kv SET value='9' WHERE key='counter'", [])
            .unwrap();
        Ok(())
    });
    assert_eq!(failure.unwrap_err().code, ErrorCode::StorageUnavailable);
    assert_eq!(store.value("counter").unwrap(), Some(json!(7)));
    sql(
        &store,
        "UPDATE kv SET value='invalid-json' WHERE key='counter';",
    );
    assert_eq!(
        store.value("counter").unwrap_err().code,
        ErrorCode::StorageUnavailable
    );
    assert_eq!(
        store
            .update_value("counter", |_| Ok((json!(10), ())))
            .unwrap_err()
            .code,
        ErrorCode::StorageUnavailable
    );
    sql(
        &store,
        "UPDATE kv SET value='7' WHERE key='counter'; CREATE TRIGGER deny_kv BEFORE UPDATE ON kv BEGIN SELECT RAISE(ABORT,'controlled'); END;",
    );
    assert_eq!(
        store
            .update_value("counter", |_| Ok((json!(10), ())))
            .unwrap_err()
            .code,
        ErrorCode::StorageUnavailable
    );
    sql(&store, "DROP TRIGGER deny_kv;");
    assert_eq!(store.value("counter").unwrap(), Some(json!(7)));
}

#[test]
fn store_lists_and_pages_enforce_limits_and_preserve_account_kind_order() {
    let (_temporary, store) = workspace();
    for id in ["c", "a", "b"] {
        store.save(record(id), None).unwrap();
    }
    let mut imported = record("import");
    imported.provenance.source = "user_import".into();
    store.import(&[imported]).unwrap();
    let mut foreign = record("foreign");
    foreign.account_id = "other".into();
    store.save(foreign, None).unwrap();
    let ids = |rows: Vec<Record>| rows.into_iter().map(|r| r.id).collect::<Vec<_>>();
    assert_eq!(ids(store.list("local", "note", 1, 2).unwrap()), ["b", "c"]);
    assert!(store.list("local", "note", 10000, 100).unwrap().is_empty());
    let (first, more) = store.page("local", "note", None, 2, true, None).unwrap();
    assert_eq!(ids(first), ["a", "b"]);
    assert!(more);
    let (last, more) = store
        .page("local", "note", Some("b"), 2, true, Some("PRESERVED"))
        .unwrap();
    assert_eq!(ids(last), ["c"]);
    assert!(!more);
    assert!(
        store
            .page("local", "note", None, 100, false, Some("absent"))
            .unwrap()
            .0
            .is_empty()
    );
    for (offset, limit) in [(0, 0), (0, 101), (10001, 1)] {
        assert_eq!(
            store.list("local", "note", offset, limit).unwrap_err().code,
            ErrorCode::InvalidInput
        );
    }
    for limit in [0, 101] {
        assert_eq!(
            store
                .page("local", "note", None, limit, false, None)
                .unwrap_err()
                .code,
            ErrorCode::InvalidInput
        );
    }
    assert_eq!(
        store
            .page("local", "note", Some("bad/id"), 1, false, None)
            .unwrap_err()
            .code,
        ErrorCode::InvalidInput
    );
    assert_eq!(
        store.get("local", "note", "missing").unwrap_err().code,
        ErrorCode::ResourceNotFound
    );
    assert_eq!(
        store.history("local", "note", "missing").unwrap_err().code,
        ErrorCode::ResourceNotFound
    );
}

#[test]
fn corrupt_rows_fail_read_list_page_and_history_instead_of_returning_partial_data() {
    let (_temporary, store) = workspace();
    let saved = store.save(record("a"), None).unwrap();
    store.save(saved.clone(), Some(1)).unwrap();
    let original = store.get("local", "note", "a").unwrap();
    for text in ["not-json", "{}", "1"] {
        store
            .transaction(|tx| {
                tx.execute("UPDATE records SET json=?1", [text]).unwrap();
                Ok(())
            })
            .unwrap();
        assert_eq!(
            store.get("local", "note", "a").unwrap_err().code,
            ErrorCode::StorageUnavailable
        );
        assert_eq!(
            store.list("local", "note", 0, 100).unwrap_err().code,
            ErrorCode::StorageUnavailable
        );
        assert_eq!(
            store
                .page("local", "note", None, 100, false, None)
                .unwrap_err()
                .code,
            ErrorCode::StorageUnavailable
        );
    }
    store
        .transaction(|tx| {
            tx.execute(
                "UPDATE records SET json=?1",
                [serde_json::to_string(&original).unwrap()],
            )
            .unwrap();
            Ok(())
        })
        .unwrap();
    sql(&store, "UPDATE history SET json='{}';");
    assert_eq!(
        store.history("local", "note", "a").unwrap_err().code,
        ErrorCode::StorageUnavailable
    );
    sql(&store, "ALTER TABLE records RENAME TO unavailable_records;");
    assert_eq!(
        store.get("local", "note", "a").unwrap_err().code,
        ErrorCode::StorageUnavailable
    );
    assert_eq!(
        store.list("local", "note", 0, 1).unwrap_err().code,
        ErrorCode::StorageUnavailable
    );
    assert_eq!(
        store
            .page("local", "note", None, 1, false, None)
            .unwrap_err()
            .code,
        ErrorCode::StorageUnavailable
    );
    sql(&store, "ALTER TABLE unavailable_records RENAME TO records;");
    assert_eq!(
        store.get("local", "note", "a").unwrap().payload,
        json!({"body":"Preserved"})
    );
}

#[test]
fn save_rejects_invalid_inputs_and_failed_history_or_record_writes_atomically() {
    let (_temporary, store) = workspace();
    let original = store.save(record("a"), None).unwrap();
    for (field, value) in [
        ("id", json!("bad/id")),
        ("account_id", json!("bad/id")),
        ("kind", json!("unknown")),
        ("payload", json!([])),
        ("payload", json!({"body":"x".repeat(256*1024)})),
        (
            "provenance",
            json!({"source":"invented","observed_at":"2026-01-01T00:00:00Z"}),
        ),
    ] {
        let mut bad = serde_json::to_value(&original).unwrap();
        bad[field] = value;
        assert_eq!(
            store
                .save(serde_json::from_value(bad).unwrap(), Some(1))
                .unwrap_err()
                .code,
            ErrorCode::InvalidInput,
            "{field}"
        );
    }
    for version in [-1, i64::MAX] {
        store
            .transaction(|tx| {
                tx.execute("UPDATE records SET version=?1", [version])
                    .unwrap();
                Ok(())
            })
            .unwrap();
        let expected = if version < 0 {
            Some(1)
        } else {
            Some(i64::MAX as u64)
        };
        assert_eq!(
            store.save(original.clone(), expected).unwrap_err().code,
            ErrorCode::StorageUnavailable
        );
        assert!(store.history("local", "note", "a").unwrap().is_empty());
    }
    sql(
        &store,
        "UPDATE records SET version=1; CREATE TRIGGER deny_history BEFORE INSERT ON history BEGIN SELECT RAISE(ABORT,'controlled'); END;",
    );
    assert_eq!(
        store.save(original.clone(), Some(1)).unwrap_err().code,
        ErrorCode::StorageUnavailable
    );
    sql(
        &store,
        "DROP TRIGGER deny_history; CREATE TRIGGER deny_record BEFORE UPDATE ON records BEGIN SELECT RAISE(ABORT,'controlled'); END;",
    );
    assert_eq!(
        store.save(original.clone(), Some(1)).unwrap_err().code,
        ErrorCode::StorageUnavailable
    );
    assert!(store.history("local", "note", "a").unwrap().is_empty());
    assert_eq!(store.get("local", "note", "a").unwrap().version, 1);
    assert_eq!(
        store.get("local", "note", "a").unwrap().payload,
        original.payload
    );
}

#[test]
fn direct_import_prevalidates_each_record_before_any_write() {
    let (_temporary, store) = workspace();
    let mut imported = record("import");
    imported.provenance.source = "user_import".into();
    for (field, value) in [
        ("id", json!("bad/id")),
        ("account_id", json!("bad/id")),
        ("kind", json!("unknown")),
        ("payload", json!([])),
        (
            "provenance",
            json!({"source":"local_draft","observed_at":"2026-01-01T00:00:00Z"}),
        ),
        (
            "provenance",
            json!({"source":"user_import","observed_at":"invalid"}),
        ),
    ] {
        let mut bad = serde_json::to_value(&imported).unwrap();
        bad[field] = value;
        assert_eq!(
            store
                .import(&[imported.clone(), serde_json::from_value(bad).unwrap()])
                .unwrap_err()
                .code,
            ErrorCode::InvalidInput,
            "{field}"
        );
        assert!(store.list("local", "note", 0, 100).unwrap().is_empty());
    }
    assert_eq!(
        store.import(&vec![imported; 1001]).unwrap_err().code,
        ErrorCode::InvalidInput
    );
    assert_eq!(store.import(&[]).unwrap(), 0);
}

#[test]
fn oversized_direct_import_is_rejected_before_inserting_recoverability_breaking_rows() {
    let (_temporary, store) = workspace();
    let mut first = record("first");
    first.provenance.source = "user_import".into();
    let mut oversized = first.clone();
    oversized.id = "oversized".into();
    oversized.payload = json!({"body":"x".repeat(256 * 1024)});
    assert_eq!(
        store.import(&[first, oversized]).unwrap_err().code,
        ErrorCode::InvalidInput
    );
    assert!(store.list("local", "note", 0, 100).unwrap().is_empty());
}

#[test]
fn backup_schema_and_row_corruption_are_rejected_without_partial_restore() {
    let (_temporary, store) = workspace();
    store.save(record("a"), None).unwrap();
    let path = store.backup("review.sqlite3").unwrap();
    let original = fs::read(&path).unwrap();
    let (_destination, destination) = workspace();
    let candidate = destination
        .root
        .safe_path("exports", "candidate.sqlite3")
        .unwrap();
    destination.root.write_new(&candidate, &original).unwrap();
    for corruption in [
        "PRAGMA user_version=0;",
        "CREATE TABLE unexpected(value);",
        "CREATE TRIGGER unexpected AFTER INSERT ON records BEGIN SELECT 1; END;",
        "ALTER TABLE kv ADD COLUMN extra TEXT;",
        "UPDATE records SET json='{}';",
        "UPDATE records SET account='other';",
        "UPDATE records SET version=0;",
        "UPDATE records SET json=json_set(json,'$.version',2);",
        "UPDATE records SET json=json_set(json,'$.payload',1);",
        "UPDATE records SET json=json_set(json,'$.provenance.source','unknown');",
        "INSERT INTO history SELECT * FROM records; UPDATE history SET id='orphan',json=json_set(json,'$.id','orphan');",
        "INSERT INTO kv VALUES('settings','{}');",
    ] {
        fs::write(&candidate, &original).unwrap();
        let connection = rusqlite::Connection::open(&candidate).unwrap();
        connection.execute_batch(corruption).unwrap();
        drop(connection);
        assert_eq!(
            destination
                .restore_backup("candidate.sqlite3")
                .unwrap_err()
                .code,
            ErrorCode::InvalidInput,
            "{corruption}"
        );
        assert!(
            destination
                .list("local", "note", 0, 100)
                .unwrap()
                .is_empty()
        );
        assert!(destination.value("settings").unwrap().is_none());
        assert_eq!(
            fs::read_dir(destination.root.path().join(".cache/tmp"))
                .unwrap()
                .count(),
            0
        );
    }
    for corruption in ["account", "port", "profiles", "proxy", "generation"] {
        let mut settings = fiverr_agent_mcp::network::Settings::default();
        match corruption {
            "account" => settings.account_id = "bad/id".into(),
            "port" => settings.http_port = 0,
            "generation" => settings.route_generation = u64::MAX,
            "profiles" => {
                settings.profiles = vec![
                    fiverr_agent_mcp::network::profiles::Profile {
                        id: "p".into(),
                        label: "Fixture".into(),
                        url: "socks5h://127.0.0.1:1595".into()
                    };
                    101
                ]
            }
            _ => {
                settings.profiles = vec![fiverr_agent_mcp::network::profiles::Profile {
                    id: "p".into(),
                    label: "Fixture".into(),
                    url: "file:///invalid".into(),
                }]
            }
        }
        fs::write(&candidate, &original).unwrap();
        let connection = rusqlite::Connection::open(&candidate).unwrap();
        connection
            .execute(
                "INSERT INTO kv VALUES('settings',?1)",
                [serde_json::to_string(&settings).unwrap()],
            )
            .unwrap();
        drop(connection);
        assert_eq!(
            destination
                .restore_backup("candidate.sqlite3")
                .unwrap_err()
                .code,
            ErrorCode::InvalidInput,
            "{corruption}"
        );
        assert!(
            destination
                .list("local", "note", 0, 100)
                .unwrap()
                .is_empty()
        );
        assert!(destination.value("settings").unwrap().is_none());
    }
    fs::write(&candidate, &original).unwrap();
    destination
        .update_value("settings", |_| {
            Ok((
                json!(fiverr_agent_mcp::network::Settings {
                    revision: u64::MAX,
                    ..Default::default()
                }),
                (),
            ))
        })
        .unwrap();
    assert_eq!(
        destination
            .restore_backup("candidate.sqlite3")
            .unwrap_err()
            .code,
        ErrorCode::StorageUnavailable
    );
    assert!(
        destination
            .list("local", "note", 0, 100)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        destination.value("settings").unwrap().unwrap()["revision"],
        u64::MAX
    );
    assert_eq!(
        store.get("local", "note", "a").unwrap().payload["body"],
        "Preserved"
    );
}

#[test]
fn logger_writes_distinct_structured_utf8_files_with_actual_redacted_context() {
    let (_temporary, store) = workspace();
    let first = Logger::open(&store.root).unwrap();
    let second = Logger::open(&store.root).unwrap();
    first.event("WARNING", "storage", "Controlled failure", json!({"count":7,"nested":[{"session":"synthetic-session","safe":"فارسی"}],"address":"https://example.test/","line":"two\nlines","short":"a\u{7}b","long":"x".repeat(257)}));
    second.event("unknown", "storage", "Exit", json!({"exit_code":1}));
    drop(first);
    drop(second);
    let mut entries = fs::read_dir(store.root.path().join("logs"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(entries.len(), 2);
    let mut records = Vec::new();
    for entry in entries {
        let name = entry.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("fiverr-agent-mcp_"));
        assert!(name.contains("_UTC_"));
        assert!(name.ends_with(".log"));
        let text = fs::read_to_string(entry).unwrap();
        assert_eq!(text.lines().count(), 1);
        assert!(!text.contains("synthetic-session"));
        let value: Value = serde_json::from_str(text.trim_end()).unwrap();
        let timestamp = value["timestamp"].as_str().unwrap();
        chrono::DateTime::parse_from_rfc3339(timestamp).unwrap();
        assert!(timestamp.ends_with('Z'));
        records.push(value);
    }
    let warning = records.iter().find(|r| r["level"] == "WARNING").unwrap();
    assert_eq!(warning["category"], "storage");
    assert_eq!(warning["message"], "Controlled failure");
    assert_eq!(
        warning["context"],
        json!({"count":7,"nested":[{"session":"[REDACTED]","safe":"فارسی"}],"address":"[REDACTED]","line":"[REDACTED]","short":"ab","long":"[REDACTED]"})
    );
    assert_eq!(
        records.iter().find(|r| r["message"] == "Exit").unwrap()["level"],
        "ERROR"
    );
}

#[test]
fn logger_retention_preserves_locked_runs_and_unrelated_files() {
    let (_temporary, store) = workspace();
    let logs = store.root.path().join("logs");
    // Direct fixture writes avoid 31 redundant Windows ACL subprocess pairs.
    for index in 0..31 {
        fs::write(
            logs.join(format!("fiverr-agent-mcp_2000_{index:02}.log")),
            b"historical",
        )
        .unwrap();
    }
    let locked_path = logs.join("fiverr-agent-mcp_2000_00.log");
    let locked = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&locked_path)
        .unwrap();
    fs2::FileExt::try_lock_exclusive(&locked).unwrap();
    fs::write(logs.join("owner-notes.log"), b"Keep owner file").unwrap();
    let logger = Logger::open(&store.root).unwrap();
    assert!(locked_path.is_file());
    assert!(!logs.join("fiverr-agent-mcp_2000_01.log").exists());
    assert_eq!(
        fs::read(logs.join("owner-notes.log")).unwrap(),
        b"Keep owner file"
    );
    drop(logger);
    drop(locked);
    let logger = Logger::open(&store.root).unwrap();
    assert!(!locked_path.exists());
    drop(logger);
}
