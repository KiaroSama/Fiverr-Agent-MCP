use fiverr_agent_mcp::{
    core::{ErrorCode, Provenance, Record, now},
    service::actions,
    storage::{Root, Store},
};
use serde_json::json;
#[test]
fn overlimit_backup_rows_and_malformed_journal_are_rejected_before_destination_writes() {
    let temp = tempfile::tempdir().unwrap();
    let source =
        Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    let record = Record {
        id: "seed".into(),
        kind: "note".into(),
        account_id: "local".into(),
        version: 1,
        draft: true,
        payload: json!({"body":"Keep"}),
        provenance: Provenance {
            source: "local_draft".into(),
            observed_at: now(),
            uri: None,
            partial: false,
        },
        updated_at: now(),
    };
    source.save(record.clone(), None).unwrap();
    let path = source.backup("candidate.sqlite3").unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    let tx = connection.unchecked_transaction().unwrap();
    {
        let mut insert = tx
            .prepare(
                "INSERT INTO records(account,kind,id,version,json) VALUES('local','note',?1,1,?2)",
            )
            .unwrap();
        for i in 0..10000 {
            let mut row = record.clone();
            row.id = format!("r{i}");
            insert
                .execute(rusqlite::params![
                    row.id,
                    serde_json::to_string(&row).unwrap()
                ])
                .unwrap();
        }
    }
    tx.commit().unwrap();
    drop(connection);
    assert_eq!(
        source
            .validate_backup("candidate.sqlite3")
            .unwrap_err()
            .code,
        ErrorCode::InvalidInput
    );
    assert_eq!(
        source.get("local", "note", "seed").unwrap().payload["body"],
        "Keep"
    );
    let action = actions::prepare(
        &source,
        "note",
        &json!({"target_id":"seed","payload":{"body":"Keep"}}),
    )
    .unwrap();
    let path = source.backup("journal.sqlite3").unwrap();
    let original = std::fs::read(&path).unwrap();
    for text in ["{}", "not-json"] {
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute("UPDATE actions SET json=?1", [text])
            .unwrap();
        drop(connection);
        assert_eq!(
            source.validate_backup("journal.sqlite3").unwrap_err().code,
            ErrorCode::InvalidInput
        );
        std::fs::write(&path, &original).unwrap();
    }
    assert_eq!(
        actions::get(&source, action["data"]["id"].as_str().unwrap()).unwrap()["data"]["state"],
        "prepared"
    );
}
