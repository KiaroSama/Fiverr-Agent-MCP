use fiverr_agent_mcp::storage::{Root, logging::Logger};
use serde_json::json;
use std::{
    io::Read,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn logger_cap_child_fixture() {
    let Some(path) = std::env::var_os("FIVERR_LOG_CAP_FIXTURE") else {
        return;
    };
    let root = Root::explicit(std::path::Path::new(&path)).unwrap();
    let logger = Logger::open(&root).unwrap();
    let log = std::fs::read_dir(root.path().join("logs"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::OpenOptions::new()
        .write(true)
        .open(log)
        .unwrap()
        .set_len(8 * 1024 * 1024)
        .unwrap();
    for _ in 0..3 {
        logger.event(
            "ERROR",
            "fixture",
            "Cap",
            json!({"token":"dummy-do-not-print"}),
        );
    }
}

#[test]
fn logger_degradation_is_visible_once_without_secret_output() {
    let temp = tempfile::tempdir().unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap());
    child
        .args(["--exact", "logger_cap_child_fixture", "--nocapture"])
        .env(
            "FIVERR_LOG_CAP_FIXTURE",
            temp.path().canonicalize().unwrap(),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        child.creation_flags(0x08000000);
    }
    let mut child = child.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("logger fixture deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .take(4096)
        .read_to_string(&mut stderr)
        .unwrap();
    assert!(status.success());
    assert_eq!(
        stderr.matches("File logging degraded").count(),
        1,
        "fixed warning must occur exactly once"
    );
    assert!(!stderr.contains("dummy-do-not-print"));
}
