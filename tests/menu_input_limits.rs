use fiverr_agent_mcp::{
    cli::{
        console::{Console, Terminal},
        menu,
    },
    network::Settings,
    storage::{Root, Store},
};
use std::{process::Stdio, time::Duration};
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn terminal_framing_fixture() {
    let Ok(case) = std::env::var("FIVERR_FRAMING_FIXTURE") else {
        return;
    };
    let mut terminal = Terminal;
    match case.as_str() {
        "fatal" => {
            assert!(terminal.read(false).is_err());
            assert!(
                terminal.read(false).is_err(),
                "rejected input must not replay its suffix"
            );
        }
        "boundary" => {
            assert_eq!(terminal.read(false).unwrap().unwrap().len(), 4096);
            assert!(terminal.read(false).unwrap().is_none());
        }
        "crlf" => {
            assert_eq!(terminal.read(false).unwrap().as_deref(), Some("owner"));
            assert_eq!(terminal.read(false).unwrap().as_deref(), Some("0"));
            assert!(terminal.read(false).unwrap().is_none());
        }
        "menu" => {
            let path = std::env::var_os("FIVERR_FRAMING_ROOT").unwrap();
            let root = Root::explicit(std::path::Path::new(&path)).unwrap();
            let store = Store::open(root.clone()).unwrap();
            let before = serde_json::to_value(Settings::load(&store).unwrap()).unwrap();
            assert!(
                menu::run(&root, &store, &mut terminal).await.is_err(),
                "fatal form framing must abort the menu"
            );
            assert_eq!(
                serde_json::to_value(Settings::load(&store).unwrap()).unwrap(),
                before
            );
        }
        _ => panic!("Unknown owned fixture case"),
    }
}

async fn reader(case: &str, input: &[u8], root: Option<&Root>) -> std::process::Output {
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "terminal_framing_fixture", "--nocapture"])
        .env("FIVERR_FRAMING_FIXTURE", case)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(root) = root {
        command.env("FIVERR_FRAMING_ROOT", root.path());
    }
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    tokio::time::timeout(Duration::from_secs(10), async move {
        stdin.write_all(input).await.unwrap();
        drop(stdin);
        child.wait_with_output().await.unwrap()
    })
    .await
    .expect("Owned framing fixture exceeded its deadline")
}

#[tokio::test]
async fn oversized_form_input_never_replays_tail_as_selection() {
    for input in [
        format!("{}4\n0\n", "x".repeat(4097)).into_bytes(),
        vec![b'x'; 4097],
        format!("{}é4\n0\n", "x".repeat(4096)).into_bytes(),
    ] {
        let output = reader("fatal", &input, None).await;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[tokio::test]
async fn terminal_input_preserves_exact_boundary_crlf_and_eof() {
    for (case, input) in [
        ("boundary", vec![b'x'; 4096]),
        ("crlf", b"owner\r\n0\r\n".to_vec()),
    ] {
        let output = reader(case, &input, None).await;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[tokio::test]
async fn oversized_actual_menu_form_exits_without_using_tail() {
    let temp = tempfile::tempdir().unwrap();
    let root = Root::explicit(&temp.path().canonicalize().unwrap()).unwrap();
    let input = format!("11\n{}4\n0\n", "x".repeat(4097));
    // Exercise menu::run with real Terminal reads, not terminal()'s redirected-stdin gate.
    let output = reader("menu", input.as_bytes(), Some(&root)).await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(text.matches("Fiverr Main menu:").count(), 1);
}
