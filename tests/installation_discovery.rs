use fiverr_agent_mcp::{
    bootstrap,
    core::ErrorCode,
    storage::{Root, Store},
};
use std::{path::Path, time::Duration};
#[tokio::test]
async fn native_relocated_installation_validates_marker_before_storage() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    std::fs::create_dir(base.join("bin")).unwrap();
    let binary = base.join("bin").join(if cfg!(windows) {
        "fiverr-agent-mcp.exe"
    } else {
        "fiverr-agent-mcp"
    });
    std::fs::copy(env!("CARGO_BIN_EXE_fiverr-agent-mcp"), &binary).unwrap();
    let run = || {
        let mut c = tokio::process::Command::new(&binary);
        c.arg("status")
            .current_dir(&base)
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.as_std_mut().creation_flags(0x08000000);
        }
        c
    };
    let output = tokio::time::timeout(Duration::from_secs(10), run().output())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("marker missing"));
    for marker in [
        b"invalid".as_slice(),
        br#"{"product":"wrong","layout_version":1}"#,
    ] {
        std::fs::write(base.join("fiverr.install.json"), marker).unwrap();
        let output = tokio::time::timeout(Duration::from_secs(10), run().output())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
    }
    std::fs::write(
        base.join("fiverr.install.json"),
        br#"{"product":"fiverr-agent-mcp","layout_version":1}"#,
    )
    .unwrap();
    let root = Root::explicit(&base).unwrap();
    let mut c = run();
    c.envs(bootstrap::environment(&root));
    let output = tokio::time::timeout(Duration::from_secs(10), c.output())
        .await
        .unwrap()
        .unwrap();
    assert!(output.status.success());
    assert!(
        root.path()
            .join(".state/workspace/workspace.sqlite3")
            .is_file()
    );
    let outside = base.join("not-bin");
    std::fs::create_dir(&outside).unwrap();
    let copy = outside.join(binary.file_name().unwrap());
    std::fs::copy(&binary, &copy).unwrap();
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new(copy)
            .arg("status")
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Nonstandard"));
    let store = Store::open(root).unwrap();
    assert!(store.list("local", "note", 0, 100).unwrap().is_empty());
    assert_eq!(
        Root::explicit(Path::new("relative")).unwrap_err().code,
        ErrorCode::FilePolicyViolation
    );
}
