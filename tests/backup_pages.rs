use fiverr_agent_mcp::{
    core::{Provenance, Record, now},
    storage::{Root, Store},
};
use serde_json::json;
#[test]
fn multipage_backup_steps_preserve_every_record_and_validated_copy() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    let mut record = Record {
        id: "seed".into(),
        kind: "snapshot".into(),
        account_id: "local".into(),
        version: 1,
        draft: true,
        payload: json!({"observation":"x".repeat(32000)}),
        provenance: Provenance {
            source: "local_draft".into(),
            observed_at: now(),
            uri: None,
            partial: false,
        },
        updated_at: now(),
    };
    store.transaction(|tx|{let mut insert=tx.prepare("INSERT INTO records(account,kind,id,version,json) VALUES('local','snapshot',?1,1,?2)").unwrap();for i in 0..40{record.id=format!("capture-{i:02}");insert.execute(rusqlite::params![record.id,serde_json::to_string(&record).unwrap()]).unwrap();}Ok(())}).unwrap();
    let backup = store.backup("pages.sqlite3").unwrap();
    assert!(std::fs::metadata(&backup).unwrap().len() > 100 * 4096);
    assert_eq!(
        store.validate_backup("pages.sqlite3").unwrap()["records"],
        40
    );
    assert_eq!(
        store
            .get("local", "snapshot", "capture-39")
            .unwrap()
            .payload["observation"]
            .as_str()
            .unwrap()
            .len(),
        32000
    );
}
