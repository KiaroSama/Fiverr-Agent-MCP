use fiverr_agent_mcp::{
    core::{Provenance, Record, now},
    storage::{Root, Store},
};
use serde_json::json;

#[test]
fn history_retains_last_hundred_without_removing_current_record() {
    let temporary = tempfile::tempdir().unwrap();
    let store =
        Store::open(Root::explicit(&temporary.path().canonicalize().unwrap()).unwrap()).unwrap();
    let mut record = Record {
        id: "a".into(),
        kind: "note".into(),
        account_id: "local".into(),
        version: 0,
        draft: true,
        payload: json!({"body":"0"}),
        provenance: Provenance {
            source: "local_draft".into(),
            observed_at: now(),
            uri: None,
            partial: false,
        },
        updated_at: now(),
    };
    let mut version = None;
    for index in 0..106 {
        record.payload = json!({"body":index.to_string()});
        let saved = store.save(record.clone(), version).unwrap();
        version = Some(saved.version);
    }
    let history = store.history("local", "note", "a").unwrap();
    assert_eq!(history.len(), 100);
    assert_eq!(history.first().unwrap().version, 105);
    assert_eq!(history.last().unwrap().version, 6);
    assert_eq!(
        store.get("local", "note", "a").unwrap().payload["body"],
        "105"
    );
    assert!(store.backup("retained.sqlite3").is_ok());
}

#[test]
fn capacity_rejects_new_record_without_removing_existing_state() {
    let temporary = tempfile::tempdir().unwrap();
    let store =
        Store::open(Root::explicit(&temporary.path().canonicalize().unwrap()).unwrap()).unwrap();
    let example = Record {
        id: "seed".into(),
        kind: "note".into(),
        account_id: "local".into(),
        version: 0,
        draft: true,
        payload: json!({"body":"Preserved"}),
        provenance: Provenance {
            source: "local_draft".into(),
            observed_at: now(),
            uri: None,
            partial: false,
        },
        updated_at: now(),
    };
    let saved = store.save(example.clone(), None).unwrap();
    store.transaction(|tx| {
        let mut statement = tx.prepare("INSERT INTO records(account,kind,id,version,json) VALUES('local','note',?1,1,?2)").unwrap();
        for index in 0..9999 {
            let mut record = saved.clone(); record.id = format!("entry-{index}");
            statement.execute(rusqlite::params![record.id,serde_json::to_string(&record).unwrap()]).unwrap();
        }
        Ok(())
    }).unwrap();
    let mut new = example;
    new.id = "over-limit".into();
    assert!(store.save(new, None).is_err());
    assert!(store.get("local", "note", "over-limit").is_err());
    assert_eq!(
        store.get("local", "note", "seed").unwrap().payload["body"],
        "Preserved"
    );
}
