use fiverr_agent_mcp::{
    cli::console::{Console, Style, Terminal},
    storage::Root,
};
use std::{process::Stdio, time::Duration};
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn preview_redirected_is_plain_and_unknown_args_fail_without_menu_wait() {
    let temp = tempfile::tempdir().unwrap();
    let root = Root::explicit(&temp.path().canonicalize().unwrap()).unwrap();
    for (args, success) in [
        (&["preview"][..], true),
        (&["preview", "unexpected"][..], false),
    ] {
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_fiverr-agent-mcp"));
        command
            .arg("--root")
            .arg(root.path())
            .args(args)
            .envs(fiverr_agent_mcp::bootstrap::environment(&root))
            .stdin(Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let output = tokio::time::timeout(Duration::from_secs(10), command.output())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(output.status.success(), success);
        assert!(!output.stdout.contains(&27));
        if success {
            assert!(
                String::from_utf8(output.stdout)
                    .unwrap()
                    .contains("Fiverr Main menu:")
            );
        }
    }
}
#[test]
fn terminal_redirected_eof_and_hidden_input_are_bounded() {
    if std::env::var_os("FIVERR_CONSOLE_FIXTURE").is_some() {
        let mut terminal = Terminal;
        for style in [
            Style::Plain,
            Style::Accent,
            Style::Success,
            Style::Warning,
            Style::Error,
            Style::Muted,
        ] {
            terminal.write("fixture\n", style).unwrap();
        }
        match std::env::var("FIVERR_CONSOLE_FIXTURE").unwrap().as_str() {
            "secret" => assert!(terminal.read(true).is_err()),
            "long" => assert!(terminal.read(false).is_err()),
            "eof" => assert!(terminal.read(false).unwrap().is_none()),
            _ => assert_eq!(terminal.read(false).unwrap(), Some("Owner input".into())),
        }
    }
}
#[tokio::test]
async fn native_terminal_reader_handles_real_redirected_input() {
    for (case, input) in [
        ("eof", String::new()),
        ("line", "Owner input\n".into()),
        ("secret", String::new()),
        ("long", format!("{}\n", "a".repeat(4097))),
    ] {
        let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "terminal_redirected_eof_and_hidden_input_are_bounded",
                "--nocapture",
            ])
            .env("FIVERR_CONSOLE_FIXTURE", case)
            .env("NO_COLOR", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let mut child = command.spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(input.as_bytes()).await.unwrap();
        drop(stdin);
        let output = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output())
            .await
            .unwrap()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!output.stdout.contains(&27));
    }
}
