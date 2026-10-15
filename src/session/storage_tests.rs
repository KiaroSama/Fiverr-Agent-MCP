use super::*;
use crate::{
    core::ErrorCode,
    network::{Mode, profiles::Profile},
};
use rusqlite::Connection;

fn setup() -> (tempfile::TempDir, Store, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(crate::storage::Root::explicit(tmp.path()).unwrap()).unwrap();
    native::pair(&store, "abcdefghijklmnopabcdefghijklmnop", "fixture_owner").unwrap();
    let path = store
        .root
        .safe_path(".state/browser", "sessions.sqlite3")
        .unwrap();
    (tmp, store, path)
}
#[test]
fn corrupt_session_rows_and_transaction_failures_preserve_original_data() {
    for operation in ["load", "replace", "record", "forget"] {
        let (_tmp, store, path) = setup();
        let connection = Connection::open(path).unwrap();
        connection
            .execute("UPDATE session SET metadata='invalid-json'", [])
            .unwrap();
        let state = initial(
            &Settings::load(&store).unwrap(),
            "extension",
            "fixture_owner",
        );
        let result = match operation {
            "load" => storage::load(&store).map(|_| ()),
            "replace" => storage::replace(&store, state, None),
            "record" => storage::record(&store, 1, "account_observed"),
            _ => storage::forget(&store),
        };
        assert!(result.is_err());
        assert_eq!(
            connection
                .query_row("SELECT metadata FROM session", [], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            "invalid-json"
        );
    }
    for operation in ["replace", "record", "forget"] {
        let (_tmp, store, path) = setup();
        let connection = Connection::open(path).unwrap();
        connection.execute_batch("CREATE TRIGGER reject_session BEFORE UPDATE ON session BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
        let (state, _) = storage::load(&store).unwrap().unwrap();
        let revision = state.revision;
        let result = match operation {
            "replace" => storage::replace(&store, state, None),
            "record" => storage::record(&store, revision, "account_observed"),
            _ => storage::forget(&store),
        };
        assert!(result.is_err());
        assert_eq!(status(&store).unwrap()["state"], "extension_paired");
    }
    let (_tmp, store, path) = setup();
    let connection = Connection::open(path).unwrap();
    let (mut state, _) = storage::load(&store).unwrap().unwrap();
    state.revision = u64::MAX;
    connection
        .execute(
            "UPDATE session SET metadata=?1",
            [serde_json::to_string(&state).unwrap()],
        )
        .unwrap();
    assert!(storage::replace(&store, state, None).is_err());
    assert!(storage::record(&store, u64::MAX, "account_observed").is_err());
    assert!(storage::forget(&store).is_err());
    assert_eq!(status(&store).unwrap()["state"], "extension_paired");
}
#[test]
fn session_missing_or_wrong_schema_and_failed_commits_never_reset_evidence() {
    for operation in ["load", "replace", "record", "forget"] {
        let (_tmp, store, path) = setup();
        let connection = Connection::open(path).unwrap();
        connection.execute_batch("DROP TABLE session; CREATE TABLE session(id INTEGER PRIMARY KEY, metadata TEXT NOT NULL);").unwrap();
        let state = initial(
            &Settings::load(&store).unwrap(),
            "extension",
            "fixture_owner",
        );
        let result = match operation {
            "load" => storage::load(&store).map(|_| ()),
            "replace" => storage::replace(&store, state, None),
            "record" => storage::record(&store, 1, "account_observed"),
            _ => storage::forget(&store),
        };
        assert!(result.is_err());
    }
    for operation in ["replace", "record", "forget"] {
        let (_tmp, store, path) = setup();
        let connection = Connection::open(path).unwrap();
        connection.execute_batch("CREATE TABLE parent(id INTEGER PRIMARY KEY); CREATE TABLE deferred(id INTEGER REFERENCES parent(id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER fail_commit AFTER UPDATE ON session BEGIN INSERT INTO deferred VALUES(1); END;").unwrap();
        let (state, _) = storage::load(&store).unwrap().unwrap();
        let revision = state.revision;
        let result = match operation {
            "replace" => storage::replace(&store, state, None),
            "record" => storage::record(&store, revision, "account_observed"),
            _ => storage::forget(&store),
        };
        assert!(result.is_err());
        assert_eq!(status(&store).unwrap()["state"], "extension_paired");
    }
    let (_tmp, store, path) = setup();
    let connection = Connection::open(path).unwrap();
    connection
        .execute("UPDATE session SET metadata=x'ffff'", [])
        .unwrap();
    assert!(storage::load(&store).is_err());
    for operation in ["replace", "record", "forget"] {
        let (_tmp, store, path) = setup();
        let connection = Connection::open(path).unwrap();
        connection
            .execute_batch(
                "DROP TABLE session; CREATE TABLE session(id INTEGER PRIMARY KEY,metadata,cookie);",
            )
            .unwrap();
        connection
            .execute("INSERT INTO session VALUES(1,x'ffff',NULL)", [])
            .unwrap();
        let state = initial(
            &Settings::load(&store).unwrap(),
            "extension",
            "fixture_owner",
        );
        let result = match operation {
            "replace" => storage::replace(&store, state, None),
            "record" => storage::record(&store, 1, "account_observed"),
            _ => storage::forget(&store),
        };
        assert!(result.is_err());
    }
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(crate::storage::Root::explicit(tmp.path()).unwrap()).unwrap();
    let path = store
        .root
        .safe_path(".state/browser", "sessions.sqlite3")
        .unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(storage::load(&store).is_err());
    assert!(storage::forget(&store).is_err());
}
#[tokio::test]
async fn session_verification_refuses_missing_wrong_mode_stale_and_dead_route() {
    let (_tmp, store, _) = setup();
    assert_eq!(
        verify(&store).await.unwrap_err().code,
        ErrorCode::AuthRequired
    );
    forget(&store).unwrap();
    assert_eq!(
        verify(&store).await.unwrap_err().code,
        ErrorCode::AuthRequired
    );
    let path = store.root.safe_path("config", "cookie.txt").unwrap();
    store
        .root
        .write_new(&path, b"session=synthetic-failure")
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    Settings::change(&store, true, |s| {
        s.mode = Mode::Proxy;
        s.selected_profile_id = Some("dead".into());
        s.profiles = vec![Profile {
            id: "dead".into(),
            label: "Fixture".into(),
            url: format!("http://{address}"),
        }];
        Ok(())
    })
    .unwrap();
    import_cookie(&store, &path, "fixture_owner").unwrap();
    assert_eq!(
        verify(&store).await.unwrap_err().code,
        ErrorCode::ProxyUnreachable
    );
    assert_eq!(status(&store).unwrap()["state"], "route_unreachable");
    let (state, _) = storage::load(&store).unwrap().unwrap();
    assert!(
        cookie::finish_verification(
            &store,
            &state,
            Err(crate::core::error(
                ErrorCode::PermissionDenied,
                "Fixture account mismatch"
            ))
        )
        .is_err()
    );
    assert_eq!(status(&store).unwrap()["state"], "account_mismatch");
    let (state, _) = storage::load(&store).unwrap().unwrap();
    assert!(
        cookie::finish_verification(
            &store,
            &state,
            Err(crate::core::error(
                ErrorCode::TlsValidationFailed,
                "Fixture TLS refusal"
            ))
        )
        .is_err()
    );
    assert_eq!(status(&store).unwrap()["state"], "verification_failed");
    let (state, _) = storage::load(&store).unwrap().unwrap();
    storage::replace(&store, state, None).unwrap();
    assert_eq!(
        verify(&store).await.unwrap_err().code,
        ErrorCode::AuthRequired
    );
    Settings::change(&store, true, |_| Ok(())).unwrap();
    assert!(verify(&store).await.is_err());
}
