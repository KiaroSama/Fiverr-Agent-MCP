use fiverr_agent_mcp::{
    network::{Mode, RouteClient, Settings},
    server::http,
    service::actions,
    storage::{Root, Store},
};
use serde_json::json;
#[tokio::test]
async fn typed_settings_token_and_capacity_rows_fail_without_reset_or_network() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    store.transaction(|tx|{tx.execute_batch("ALTER TABLE kv RENAME TO old;CREATE TABLE kv(key TEXT,value);INSERT INTO kv VALUES('settings',x'ff');").unwrap();Ok(())}).unwrap();
    assert!(Settings::load(&store).is_err());
    assert!(store.transaction(Settings::in_transaction).is_err());
    assert!(Settings::change(&store, false, |_| Ok(())).is_err());
    store
        .transaction(|tx| {
            tx.execute_batch("INSERT INTO kv VALUES('http_token',x'ff');")
                .unwrap();
            Ok(())
        })
        .unwrap();
    assert!(http::token(&store).is_err());
    store
        .transaction(|tx| {
            tx.execute_batch("DROP TABLE kv;ALTER TABLE old RENAME TO kv;")
                .unwrap();
            Ok(())
        })
        .unwrap();
    Settings::change(&store, true, |s| {
        s.mode = Mode::Direct;
        Ok(())
    })
    .unwrap();
    let route = RouteClient::new(store.clone()).unwrap();
    Settings::change(&store, true, |s| {
        s.mode = Mode::Blocked;
        Ok(())
    })
    .unwrap();
    assert!(
        route
            .get(url::Url::parse("https://www.fiverr.com/").unwrap())
            .await
            .is_err()
    );
    store
        .transaction(|tx| {
            tx.execute_batch("ALTER TABLE history RENAME TO old;")
                .unwrap();
            Ok(())
        })
        .unwrap();
    assert!(
        actions::prepare(
            &store,
            "note",
            &json!({"target_id":"a","payload":{"body":"Keep"}})
        )
        .is_err()
    );
    store
        .transaction(|tx| {
            tx.execute_batch("ALTER TABLE old RENAME TO history;")
                .unwrap();
            Ok(())
        })
        .unwrap();
    assert!(
        actions::list(&store).unwrap()["data"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
#[test]
fn exhausted_destination_generation_and_token_deletion_failure_do_not_restore() {
    let temp = tempfile::tempdir().unwrap();
    let source =
        Store::open(Root::explicit(&temp.path().canonicalize().unwrap()).unwrap()).unwrap();
    let path = source.backup("source.sqlite3").unwrap();
    let other = tempfile::tempdir().unwrap();
    let dest = Store::open(Root::explicit(&other.path().canonicalize().unwrap()).unwrap()).unwrap();
    dest.root
        .write_new(
            &dest.root.safe_path("exports", "source.sqlite3").unwrap(),
            &std::fs::read(path).unwrap(),
        )
        .unwrap();
    dest.update_value("settings", |_| {
        Ok((
            serde_json::to_value(Settings {
                route_generation: u64::MAX,
                ..Default::default()
            })
            .unwrap(),
            (),
        ))
    })
    .unwrap();
    assert!(dest.restore_backup("source.sqlite3").is_err());
    dest.update_value("settings", |_| {
        Ok((serde_json::to_value(Settings::default()).unwrap(), ()))
    })
    .unwrap();
    dest.update_value("http_token", |_| Ok((json!("fixture-token"), ())))
        .unwrap();
    dest.transaction(|tx|{tx.execute_batch("CREATE TRIGGER refuse_delete BEFORE DELETE ON kv BEGIN SELECT RAISE(ABORT,'controlled'); END;").unwrap();Ok(())}).unwrap();
    assert!(dest.restore_backup("source.sqlite3").is_err());
    assert_eq!(
        dest.value("http_token").unwrap(),
        Some(json!("fixture-token"))
    );
    assert!(dest.list("local", "note", 0, 1).unwrap().is_empty());
}
