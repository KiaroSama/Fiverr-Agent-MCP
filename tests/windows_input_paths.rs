#![cfg(windows)]
use fiverr_agent_mcp::{
    cli::{
        self, Command, Proxy,
        console::{Console, Style},
        menu,
    },
    core::{ErrorCode, Result},
    network::Settings,
    storage::{Root, Store},
};
use std::{collections::VecDeque, path::PathBuf};

#[test]
fn normal_and_verbatim_owned_paths_work_without_accepting_escape_or_traversal() {
    let temp = tempfile::Builder::new()
        .prefix("Windows input space-")
        .tempdir()
        .unwrap();
    let root = Root::explicit(temp.path()).unwrap();
    let normal = dunce::simplified(root.path());
    assert_ne!(
        normal,
        root.path(),
        "Regression must use ordinary drive input against a verbatim root"
    );
    let input = normal.join("Proxy list.txt");
    root.write_new(&input, b"http://127.0.0.1:1595:::Fixture\n")
        .unwrap();
    assert_eq!(
        root.read_bounded(&input, 100).unwrap(),
        b"http://127.0.0.1:1595:::Fixture\n"
    );
    assert!(
        root.assert_owned(&root.path().join("Proxy list.txt"))
            .is_ok()
    );
    let canonical = root.path().join("exports/canonical.txt");
    root.write_new(&canonical, b"canonical").unwrap();
    assert_eq!(root.read_bounded(&canonical, 100).unwrap(), b"canonical");
    let outside = tempfile::tempdir().unwrap();
    assert_eq!(
        root.assert_owned(&outside.path().join("outside.txt"))
            .unwrap_err()
            .code,
        ErrorCode::FilePolicyViolation
    );
    for suffix in [r"\exports\..\Proxy list.txt", r"\exports\.\new.txt"] {
        let path = PathBuf::from(format!("{}{suffix}", normal.display()));
        assert_eq!(
            root.assert_owned(&path).unwrap_err().code,
            ErrorCode::FilePolicyViolation
        );
    }
}
struct Input {
    lines: VecDeque<String>,
    output: String,
}
impl Console for Input {
    fn write(&mut self, text: &str, _: Style) -> Result<()> {
        self.output.push_str(text);
        Ok(())
    }
    fn read(&mut self, _: bool) -> Result<Option<String>> {
        Ok(self.lines.pop_front())
    }
}
#[tokio::test]
async fn quoted_normal_absolute_proxy_path_imports_from_menu_and_cli() {
    let temp = tempfile::Builder::new()
        .prefix("Menu Windows space-")
        .tempdir()
        .unwrap();
    let root = Root::explicit(temp.path()).unwrap();
    let store = Store::open(root.clone()).unwrap();
    let file = dunce::simplified(root.path()).join("Proxy list.txt");
    root.write_new(&file, b"http://127.0.0.1:1595:::Fixture\n")
        .unwrap();
    let mut screen = Input {
        lines: ["2".into(), format!("\"{}\"", file.display()), "quit".into()].into(),
        output: String::new(),
    };
    menu::run(&root, &store, &mut screen).await.unwrap();
    assert!(screen.output.contains("Imported: 1"), "{}", screen.output);
    assert!(!screen.output.contains("outside the installation"));
    assert_eq!(Settings::load(&store).unwrap().profiles.len(), 1);
    let result = cli::execute(
        root,
        store,
        Command::Proxy {
            command: Proxy::Import { file },
        },
    )
    .await
    .unwrap();
    assert_eq!(result["imported"], 0);
    assert_eq!(result["skipped"], 1);
}

#[tokio::test]
async fn actual_windows_entrypoint_accepts_normal_absolute_import_and_refuses_outside() {
    let temp = tempfile::Builder::new()
        .prefix("Native Windows space-")
        .tempdir()
        .unwrap();
    let root = Root::explicit(temp.path()).unwrap();
    let file = dunce::simplified(root.path()).join("Proxy list.txt");
    root.write_new(&file, b"http://127.0.0.1:1595:::Native fixture\n")
        .unwrap();
    let outside = tempfile::tempdir().unwrap();
    let outside_file = outside.path().join("outside.txt");
    std::fs::write(&outside_file, b"http://127.0.0.1:1596\n").unwrap();
    for (input, success) in [(&file, true), (&outside_file, false)] {
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_fiverr-agent-mcp"));
        command
            .arg("--root")
            .arg(dunce::simplified(root.path()))
            .args(["proxy", "import"])
            .arg(input)
            .envs(fiverr_agent_mcp::bootstrap::environment(&root))
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true);
        command.creation_flags(0x08000000);
        let output = tokio::time::timeout(std::time::Duration::from_secs(10), command.output())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            output.status.success(),
            success,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        if success {
            let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value["imported"], 1);
        } else {
            assert!(String::from_utf8_lossy(&output.stderr).contains("FILE_POLICY_VIOLATION"));
        }
    }
    let store = Store::open(root).unwrap();
    assert_eq!(Settings::load(&store).unwrap().profiles.len(), 1);
    assert_eq!(
        std::fs::read(outside_file).unwrap(),
        b"http://127.0.0.1:1596\n"
    );
}
