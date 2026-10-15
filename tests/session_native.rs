use fiverr_agent_mcp::{
    session::{self, native},
    storage::{Root, Store},
};
use serde_json::json;
const ID: &str = "abcdefghijklmnopabcdefghijklmnop";
#[test]
fn paired_native_observation_is_account_bound_and_never_cookie_authentication() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(Root::explicit(temp.path()).unwrap()).unwrap();
    native::pair(&store, ID, "fixture_owner").unwrap();
    let input = json!({"type":"account_observation","url":"https://www.fiverr.com/users/manage_gigs","username":"fixture_owner","management":true,"logout":true});
    let bytes = serde_json::to_vec(&input).unwrap();
    let caller = format!("chrome-extension://{ID}/");
    assert!(
        native::receive(
            &store,
            "chrome-extension://pppppppppppppppppppppppppppppppp/",
            &bytes
        )
        .is_err()
    );
    assert!(
        !session::status(&store).unwrap()["account_observed"]
            .as_bool()
            .unwrap()
    );
    let accepted = native::receive(&store, &caller, &bytes).unwrap();
    assert_eq!(accepted["ok"], true);
    assert_eq!(accepted["status"]["state"], "account_observed");
    assert_eq!(accepted["status"]["live_authenticated"], false);
    assert_eq!(
        accepted["status"]["route_assurance"],
        "operator_managed_unverified"
    );
    let mut wrong = input.clone();
    wrong["username"] = json!("other");
    assert!(native::receive(&store, &caller, &serde_json::to_vec(&wrong).unwrap()).is_err());
    assert_eq!(session::status(&store).unwrap()["account_observed"], false);
    for bytes in [b"not json".to_vec(), vec![b'x'; 65537], b"{}".to_vec()] {
        assert!(native::receive(&store, &caller, &bytes).is_err());
    }
    assert!(native::pair(&store, "invalid", "fixture_owner").is_err());
    assert!(native::pair(&store, ID, "../invalid").is_err());
}
#[tokio::test(start_paused = true)]
async fn native_frame_deadlines_and_invalid_lengths_fail_closed() {
    let (_sender, mut receiver) = tokio::io::duplex(32);
    assert!(
        native::read_frame(&mut receiver)
            .await
            .unwrap_err()
            .message
            .contains("deadline")
    );
    for bytes in [
        0_u32.to_ne_bytes().to_vec(),
        [4_u32.to_ne_bytes().as_slice(), b"xy"].concat(),
        vec![0xff; 3],
    ] {
        let mut reader = bytes.as_slice();
        assert!(native::read_frame(&mut reader).await.is_err());
    }
    let (mut writer, _reader) = tokio::io::duplex(1);
    assert!(
        native::write_frame(&mut writer, &json!({"ok":true}))
            .await
            .is_err()
    );
    assert!(
        native::write_frame(&mut tokio::io::sink(), &json!("x".repeat(65537)))
            .await
            .is_err()
    );
}
#[tokio::test]
async fn native_length_framing_is_exact_bounded_and_rejects_eof() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (mut sender, mut receiver) = tokio::io::duplex(1024);
    sender.write_all(&3_u32.to_ne_bytes()).await.unwrap();
    sender.write_all(b"abc").await.unwrap();
    assert_eq!(native::read_frame(&mut receiver).await.unwrap(), b"abc");
    native::write_frame(&mut sender, &json!({"ok":false}))
        .await
        .unwrap();
    let bytes = native::read_frame(&mut receiver).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
        json!({"ok":false})
    );
    sender.write_all(&65537_u32.to_ne_bytes()).await.unwrap();
    assert!(native::read_frame(&mut receiver).await.is_err());
    sender.shutdown().await.unwrap();
    assert!(native::read_frame(&mut receiver).await.is_err());
    let mut output = [0; 1];
    assert_eq!(receiver.read(&mut output).await.unwrap(), 0);
}
