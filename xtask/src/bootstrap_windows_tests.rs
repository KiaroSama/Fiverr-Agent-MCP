//! Actual bootstrap Job close semantics, using disposable native process fixtures.
#![allow(unsafe_code)]
use std::{
    fs,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    TerminateProcess, WaitForSingleObject,
};
const FIXTURE: &str = "bootstrap_windows_tests::tree_fixture";

#[test]
fn tree_fixture() {
    let Some(directory) = std::env::var_os("FIVERR_WINDOWS_TREE") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    if std::env::var_os("FIVERR_WINDOWS_DESCENDANT").is_some() {
        fs::write(directory.join("descendant"), std::process::id().to_string()).unwrap();
        fs::write(directory.join("ready"), b"ready").unwrap();
    } else {
        fs::write(directory.join("root"), std::process::id().to_string()).unwrap();
        let descendant = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", FIXTURE, "--nocapture"])
            .env("FIVERR_WINDOWS_DESCENDANT", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        // The enclosing Job owns this deliberately retained descendant.
        std::mem::forget(descendant);
    }
    loop {
        std::thread::park_timeout(Duration::from_millis(20));
    }
}

fn process_handle(pid: u32, access: u32) -> OwnedHandle {
    // SAFETY: OpenProcess returns a new uniquely owned handle, pinned before termination.
    let raw = unsafe { OpenProcess(access, 0, pid) };
    assert!(!raw.is_null());
    unsafe { OwnedHandle::from_raw_handle(raw) }
}

#[test]
fn forced_bootstrap_exit_closes_job_and_terminates_descendants() {
    let directory = crate::root()
        .join(".ci-work")
        .join(format!("windows-bootstrap-tree-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let owner = crate::root().join(".ci-work/owner.exe");
    let owner = if owner.is_file() {
        owner
    } else {
        crate::root().join(".ci-work/owner")
    };
    let mut command = Command::new(owner);
    command
        .arg(std::env::current_exe().unwrap())
        .args(["--exact", FIXTURE, "--nocapture"])
        .env("FIVERR_WINDOWS_TREE", &directory)
        .current_dir(crate::root());
    let (mut guardian, outer_job) = crate::windows_process::Job::spawn(&mut command).unwrap();
    let guardian_handle = process_handle(guardian.id(), PROCESS_TERMINATE | PROCESS_SYNCHRONIZE);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !directory.join("ready").exists() && Instant::now() < deadline {
        std::thread::park_timeout(Duration::from_millis(10));
    }
    assert!(
        directory.join("descendant").is_file(),
        "tree readiness missing"
    );
    let handles: Vec<OwnedHandle> = ["root", "descendant"]
        .iter()
        .map(|name| {
            let pid = fs::read_to_string(directory.join(name))
                .unwrap()
                .parse()
                .unwrap();
            process_handle(pid, PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION)
        })
        .collect();
    assert!(
        handles
            .iter()
            .all(|h| unsafe { WaitForSingleObject(h.as_raw_handle(), 0) } == 258)
    );
    // Kill the guardian only, not the outer Job or its child tree.
    assert_ne!(
        unsafe { TerminateProcess(guardian_handle.as_raw_handle(), 7) },
        0
    );
    assert!(!guardian.wait().unwrap().success());
    for handle in &handles {
        // A retained native handle pins identity even if Windows reuses its PID.
        assert_eq!(
            unsafe { WaitForSingleObject(handle.as_raw_handle(), 3000) },
            0,
            "live descendant survived guardian Job close"
        );
    }
    drop(handles);
    drop(guardian_handle);
    drop(guardian);
    drop(outer_job);
    fs::remove_dir_all(directory).unwrap();
}
