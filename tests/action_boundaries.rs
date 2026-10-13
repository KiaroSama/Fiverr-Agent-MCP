use fiverr_agent_mcp::{
    core::{ErrorCode, hash},
    network::Settings,
    service::{self, actions},
    storage::{Root, Store},
};
use serde_json::{Value, json};

fn workspace() -> (tempfile::TempDir, Store) {
    let temporary = tempfile::tempdir().unwrap();
    let root = Root::explicit(&temporary.path().canonicalize().unwrap()).unwrap();
    (temporary, Store::open(root).unwrap())
}
fn prepare(store: &Store) -> Value {
    actions::prepare(store, "message", &json!({"target_id":"conversation1","payload":{"recipient_id":"buyer1","body":"Owner review only"}})).unwrap()["data"].clone()
}
fn journal(store: &Store, action: &Value) {
    store
        .transaction(|tx| {
            tx.execute(
                "UPDATE actions SET state=?1,json=?2 WHERE id=?3",
                rusqlite::params![
                    action["state"].as_str().unwrap(),
                    action.to_string(),
                    action["id"].as_str().unwrap()
                ],
            )
            .unwrap();
            Ok(())
        })
        .unwrap();
}

#[test]
fn expired_or_payload_tampered_plans_cannot_be_approved_or_consumed() {
    let (_temporary, store) = workspace();
    for state in ["prepared", "approved"] {
        for corruption in ["expired", "payload"] {
            let mut action = prepare(&store);
            let id = action["id"].as_str().unwrap().to_owned();
            if state == "approved" {
                actions::approve(&store, &id, true).unwrap();
                action = actions::get(&store, &id).unwrap()["data"].clone();
            }
            let expected = if corruption == "expired" {
                action["expires_at"] = json!(0);
                ErrorCode::ConfirmationExpired
            } else {
                action["payload"]["body"] = json!("Unreviewed replacement");
                ErrorCode::ActionStateChanged
            };
            journal(&store, &action);
            assert_eq!(
                actions::approve(&store, &id, true).unwrap_err().code,
                expected
            );
            assert_eq!(actions::commit(&store, &id).unwrap_err().code, expected);
            assert_eq!(actions::get(&store, &id).unwrap()["data"], action);
        }
    }
}

#[test]
fn account_and_generation_fences_do_not_consume_original_approval() {
    let (_temporary, store) = workspace();
    let action = prepare(&store);
    let id = action["id"].as_str().unwrap();
    actions::approve(&store, id, true).unwrap();
    Settings::change(&store, false, |settings| {
        settings.account_id = "other".into();
        Ok(())
    })
    .unwrap();
    assert_eq!(
        actions::approve(&store, id, true).unwrap_err().code,
        ErrorCode::ActionStateChanged
    );
    assert_eq!(
        actions::commit(&store, id).unwrap_err().code,
        ErrorCode::ResourceNotFound
    );
    assert_eq!(
        actions::reconcile(&store, id).unwrap_err().code,
        ErrorCode::ResourceNotFound
    );
    Settings::change(&store, false, |settings| {
        settings.account_id = "local".into();
        Ok(())
    })
    .unwrap();
    assert_eq!(
        actions::get(&store, id).unwrap()["data"]["state"],
        "approved"
    );
    Settings::change(&store, true, |_| Ok(())).unwrap();
    assert_eq!(
        actions::approve(&store, id, true).unwrap_err().code,
        ErrorCode::ActionStateChanged
    );
    assert_eq!(
        actions::commit(&store, id).unwrap_err().code,
        ErrorCode::ActionStateChanged
    );
    assert_eq!(
        actions::get(&store, id).unwrap()["data"]["state"],
        "approved"
    );
}

#[test]
fn cancellation_and_completed_states_are_single_use_and_reconciliation_is_not_dispatch() {
    let (_temporary, store) = workspace();
    for state in [
        "approved",
        "cancelled",
        "manual_handoff",
        "succeeded",
        "failed",
    ] {
        let mut action = prepare(&store);
        let id = action["id"].as_str().unwrap().to_owned();
        action["state"] = json!(state);
        journal(&store, &action);
        assert_eq!(
            actions::approve(&store, &id, true).unwrap_err().code,
            ErrorCode::ActionStateChanged
        );
        assert_eq!(actions::reconcile(&store, &id).unwrap()["data"], action);
        if state == "approved" {
            assert_eq!(
                actions::cancel(&store, &id).unwrap()["data"]["state"],
                "cancelled"
            );
        } else {
            assert_eq!(
                actions::cancel(&store, &id).unwrap_err().code,
                ErrorCode::ActionStateChanged
            );
        }
        assert_eq!(
            actions::commit(&store, &id).unwrap_err().code,
            ErrorCode::ConfirmationRequired
        );
        assert_eq!(
            actions::get(&store, &id).unwrap()["data"]["state"],
            if state == "approved" {
                "cancelled"
            } else {
                state
            }
        );
    }
    for state in ["executing", "outcome_unknown"] {
        let mut action = prepare(&store);
        let id = action["id"].as_str().unwrap().to_owned();
        action["state"] = json!(state);
        journal(&store, &action);
        assert_eq!(
            actions::reconcile(&store, &id).unwrap_err().code,
            ErrorCode::ActionOutcomeUnknown
        );
        assert_eq!(actions::get(&store, &id).unwrap()["data"], action);
    }
}

#[test]
fn malformed_or_missing_journal_entries_fail_without_resetting_other_plans() {
    let (_temporary, store) = workspace();
    let healthy = prepare(&store);
    let broken = prepare(&store);
    let id = broken["id"].as_str().unwrap();
    for text in ["not-json", "{}"] {
        store
            .transaction(|tx| {
                tx.execute(
                    "UPDATE actions SET json=?1 WHERE id=?2",
                    rusqlite::params![text, id],
                )
                .unwrap();
                Ok(())
            })
            .unwrap();
        for operation in [
            actions::get,
            actions::commit,
            actions::cancel,
            actions::reconcile,
        ] {
            assert_eq!(
                operation(&store, id).unwrap_err().code,
                ErrorCode::StorageUnavailable
            );
            assert_eq!(
                operation(&store, "absent").unwrap_err().code,
                ErrorCode::ResourceNotFound
            );
            assert_eq!(
                operation(&store, "bad/id").unwrap_err().code,
                ErrorCode::InvalidInput
            );
        }
        assert_eq!(
            actions::approve(&store, id, true).unwrap_err().code,
            ErrorCode::StorageUnavailable
        );
        assert_eq!(
            actions::get(&store, healthy["id"].as_str().unwrap()).unwrap()["data"],
            healthy
        );
        if text == "not-json" {
            assert_eq!(
                actions::list(&store).unwrap_err().code,
                ErrorCode::StorageUnavailable
            );
        }
    }
    journal(&store, &broken);
    assert_eq!(
        actions::list(&store).unwrap()["data"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    store.transaction(|tx| { tx.execute_batch("CREATE TRIGGER refuse_action BEFORE UPDATE ON actions BEGIN SELECT RAISE(ABORT,'controlled'); END;").unwrap(); Ok(()) }).unwrap();
    assert_eq!(
        actions::approve(&store, id, true).unwrap_err().code,
        ErrorCode::StorageUnavailable
    );
    assert_eq!(
        actions::get(&store, id).unwrap()["data"]["state"],
        "prepared"
    );
}

#[test]
fn invalid_attachment_shapes_types_signatures_and_sizes_do_not_prepare_actions() {
    let (_temporary, store) = workspace();
    let input =
        json!({"target_id":"conversation1","payload":{"recipient_id":"buyer1","body":"Review"}});
    for (files, code) in [
        (json!(false), ErrorCode::InvalidInput),
        (json!([1]), ErrorCode::InvalidInput),
        (json!(vec!["a.txt"; 11]), ErrorCode::InvalidInput),
        (json!(["../outside.txt"]), ErrorCode::FilePolicyViolation),
        (json!(["missing.txt"]), ErrorCode::ResourceNotFound),
    ] {
        let mut bad = input.clone();
        bad["files"] = files;
        assert_eq!(
            actions::prepare(&store, "message", &bad).unwrap_err().code,
            code
        );
    }
    for (name, bytes, code) in [
        ("bad.txt", b"\xff\xfe".as_slice(), ErrorCode::InvalidInput),
        (
            "bad.pdf",
            b"plain text".as_slice(),
            ErrorCode::FilePolicyViolation,
        ),
        (
            "bad.png",
            b"plain text".as_slice(),
            ErrorCode::FilePolicyViolation,
        ),
        (
            "bad.jpg",
            b"plain text".as_slice(),
            ErrorCode::FilePolicyViolation,
        ),
        (
            "active.html",
            b"<script>".as_slice(),
            ErrorCode::FilePolicyViolation,
        ),
        (
            "archive.zip",
            b"PK".as_slice(),
            ErrorCode::FilePolicyViolation,
        ),
        (
            "no-extension",
            b"plain text".as_slice(),
            ErrorCode::FilePolicyViolation,
        ),
    ] {
        let path = store.root.safe_path(".state/workspace", name).unwrap();
        store.root.write_new(&path, bytes).unwrap();
        let mut bad = input.clone();
        bad["files"] = json!([name]);
        assert_eq!(
            actions::prepare(&store, "message", &bad).unwrap_err().code,
            code,
            "{name}"
        );
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
    let path = store
        .root
        .safe_path(".state/workspace", "oversize.txt")
        .unwrap();
    store.root.write_new(&path, b"").unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(10 * 1024 * 1024 + 1)
        .unwrap();
    let mut bad = input;
    bad["files"] = json!(["oversize.txt"]);
    assert_eq!(
        actions::prepare(&store, "message", &bad).unwrap_err().code,
        ErrorCode::FilePolicyViolation
    );
    assert!(
        actions::list(&store).unwrap()["data"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn signature_only_fixtures_bind_exact_bytes_without_claiming_real_media_validation() {
    let (_temporary, store) = workspace();
    // Partial signature-boundary evidence only: these are not usable images or PDFs.
    for (name, bytes, mime, digest) in [
        (
            "text.TXT",
            b"abc".as_slice(),
            "text/plain",
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
        ),
        ("sample.pdf", b"%PDF-".as_slice(), "application/pdf", None),
        (
            "sample.png",
            b"\x89PNG\r\n\x1a\n".as_slice(),
            "image/png",
            None,
        ),
        (
            "sample.jpeg",
            b"\xff\xd8\xff".as_slice(),
            "image/jpeg",
            None,
        ),
    ] {
        let path = store.root.safe_path(".state/workspace", name).unwrap();
        store.root.write_new(&path, bytes).unwrap();
        let prepared = actions::prepare(&store, "delivery", &json!({"target_id":"order1","payload":{"order_id":"order1","body":"Review only"},"files":[name]})).unwrap();
        let file = &prepared["data"]["files"][0];
        assert_eq!(file["path"], name);
        assert_eq!(file["bytes"], bytes.len() as u64);
        assert_eq!(file["mime"], mime);
        if let Some(digest) = digest {
            assert_eq!(file["sha256"], digest);
        }
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
}

#[test]
fn deleted_attachment_and_missing_or_negative_draft_versions_invalidate_plan() {
    let (_temporary, store) = workspace();
    let path = store
        .root
        .safe_path(".state/workspace", "delivery.txt")
        .unwrap();
    store.root.write_new(&path, b"Original").unwrap();
    service::dispatch(&store, "fiverr_record_save", json!({"kind":"delivery","id":"draft1","payload":{"order_id":"order1","body":"Owner review"}})).unwrap();
    let input = json!({"target_id":"order1","payload":{"order_id":"order1","body":"Owner review"},"files":["delivery.txt"],"record_binding":{"kind":"delivery","id":"draft1","version":1}});
    let action = actions::prepare(&store, "delivery", &input).unwrap();
    let id = action["data"]["id"].as_str().unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(
        actions::approve(&store, id, true).unwrap_err().code,
        ErrorCode::ResourceNotFound
    );
    store.root.write_new(&path, b"Original").unwrap();
    store
        .transaction(|tx| {
            tx.execute("UPDATE records SET version=-1 WHERE id='draft1'", [])
                .unwrap();
            Ok(())
        })
        .unwrap();
    assert_eq!(
        actions::approve(&store, id, true).unwrap_err().code,
        ErrorCode::StorageUnavailable
    );
    store
        .transaction(|tx| {
            tx.execute("DELETE FROM records WHERE id='draft1'", [])
                .unwrap();
            Ok(())
        })
        .unwrap();
    assert_eq!(
        actions::approve(&store, id, true).unwrap_err().code,
        ErrorCode::ActionStateChanged
    );
    assert_eq!(
        actions::get(&store, id).unwrap()["data"]["state"],
        "prepared"
    );
    for (binding, code) in [
        (
            json!({"kind":"delivery","id":"draft1"}),
            ErrorCode::InvalidInput,
        ),
        (
            json!({"kind":"delivery","id":"draft1","version":1}),
            ErrorCode::ResourceNotFound,
        ),
        (
            json!({"kind":"unknown","id":"draft1","version":1}),
            ErrorCode::InvalidInput,
        ),
    ] {
        let mut bad = input.clone();
        bad["record_binding"] = binding;
        assert_eq!(
            actions::prepare(&store, "delivery", &bad).unwrap_err().code,
            code
        );
    }
    service::dispatch(&store, "fiverr_record_save", json!({"kind":"delivery","id":"draft1","payload":{"order_id":"order1","body":"New review"}})).unwrap();
    let mut stale = input.clone();
    stale["record_binding"]["version"] = json!(2);
    assert_eq!(
        actions::prepare(&store, "delivery", &stale)
            .unwrap_err()
            .code,
        ErrorCode::ActionStateChanged
    );
    for (kind, input) in [
        ("unknown", input),
        ("message", json!({"target_id":"bad/id","payload":{}})),
        ("message", json!({"target_id":"buyer1"})),
    ] {
        assert_eq!(
            actions::prepare(&store, kind, &input).unwrap_err().code,
            ErrorCode::InvalidInput
        );
    }
}

#[test]
fn restored_executing_journal_remains_unknown_and_cannot_be_replayed() {
    let (_temporary, source) = workspace();
    let mut action = prepare(&source);
    let id = action["id"].as_str().unwrap().to_owned();
    action["state"] = json!("executing");
    journal(&source, &action);
    let backup = source.backup("journal.sqlite3").unwrap();
    let original = std::fs::read(&backup).unwrap();
    for (field, value) in [
        ("state", json!("unrecognized")),
        ("id", json!("mismatch")),
        ("payload_hash", json!("bad")),
        (
            "files",
            json!(vec![
                json!({"path":"a.txt","bytes":0,"sha256":"bad","mime":"text/plain"});
                11
            ]),
        ),
    ] {
        let mut corrupt = action.clone();
        corrupt[field] = value;
        let connection = rusqlite::Connection::open(&backup).unwrap();
        connection
            .execute("UPDATE actions SET json=?1", [corrupt.to_string()])
            .unwrap();
        drop(connection);
        assert_eq!(
            source.validate_backup("journal.sqlite3").unwrap_err().code,
            ErrorCode::InvalidInput,
            "{field}"
        );
        std::fs::write(&backup, &original).unwrap();
    }
    let (_destination, destination) = workspace();
    destination
        .root
        .write_new(
            &destination
                .root
                .safe_path("exports", "journal.sqlite3")
                .unwrap(),
            &original,
        )
        .unwrap();
    let review = destination.validate_backup("journal.sqlite3").unwrap();
    assert_eq!(review["actions"], 1);
    assert_eq!(review["records"], 0);
    destination
        .restore_backup_checked("journal.sqlite3", review["sha256"].as_str())
        .unwrap();
    assert_eq!(
        actions::get(&destination, &id).unwrap()["data"]["state"],
        "outcome_unknown"
    );
    assert_eq!(
        actions::commit(&destination, &id).unwrap_err().code,
        ErrorCode::ActionOutcomeUnknown
    );
    assert_eq!(
        actions::reconcile(&destination, &id).unwrap_err().code,
        ErrorCode::ActionOutcomeUnknown
    );
    assert_eq!(
        actions::cancel(&destination, &id).unwrap_err().code,
        ErrorCode::ActionStateChanged
    );
    assert_eq!(
        actions::get(&source, &id).unwrap()["data"]["state"],
        "executing"
    );
    assert_eq!(
        hash(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}
