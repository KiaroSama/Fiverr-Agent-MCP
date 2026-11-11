//! Explicit sandbox-only Linux qualification of real bootstrap cleanup refusal.
use super::super::OwnedChild;
use std::{
    fs,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const CASE: &str =
    "bootstrap_owner_tests::owner_source::terminal_tests::denial::sandboxed_kernel_cleanup_denial";

#[test]
fn sandboxed_kernel_cleanup_denial() {
    // Normal native suites do not claim this separately constrained OS scenario.
    if std::env::var("FIVERR_SANDBOX").as_deref() != Ok("required") {
        return;
    }
    if std::env::var("FIVERR_DENIAL_WORKER").as_deref() == Ok("1") {
        worker();
        return;
    }
    preflight();
    // Become a subreaper before the worker spawns: orphan cleanup remains owned.
    unsafe extern "C" {
        fn prctl(op: i32, ...) -> i32;
        fn kill(pid: i32, signal: i32) -> i32;
        fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
    }
    assert_eq!(unsafe { prctl(36, 1usize, 0usize, 0usize, 0usize) }, 0);
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", CASE, "--nocapture"])
        .env("FIVERR_DENIAL_WORKER", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "denial worker must report failed cleanup"
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("CI bootstrap cleanup failed:"), "{stderr}");
    let pid: i32 = fs::read_to_string("/scratch/owned.pid")
        .unwrap()
        .parse()
        .unwrap();
    assert!(pid > 1);
    assert_eq!(
        unsafe { kill(-pid, 9) },
        0,
        "unfiltered outer owner must clean exact group"
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut status = 0;
    loop {
        let result = unsafe { waitpid(pid, &mut status, 1) };
        if result == pid {
            break;
        }
        assert_eq!(result, 0, "outer owner lost adopted child");
        assert!(Instant::now() < deadline, "outer cleanup exceeded deadline");
        std::thread::park_timeout(Duration::from_millis(10));
    }
    assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
    fs::remove_file("/scratch/owned.pid").unwrap();
    println!("sandbox cleanup-denial: actual EPERM, bounded refusal, adopted child reaped");
}

fn preflight() {
    let allowed = ["HOME", "TMPDIR", "XDG_CACHE_HOME", "PATH", "FIVERR_SANDBOX"];
    assert!(std::env::vars_os().all(|(k, _)| allowed.iter().any(|a| k == *a)));
    assert_eq!(std::env::var("HOME").unwrap(), "/scratch");
    assert_eq!(std::env::var("TMPDIR").unwrap(), "/scratch");
    assert_eq!(std::env::var("XDG_CACHE_HOME").unwrap(), "/scratch");
    let status = fs::read_to_string("/proc/self/status").unwrap();
    assert!(status.lines().any(|l| l == "NoNewPrivs:\t1"));
    assert!(status.lines().any(|l| l == "CapEff:\t0000000000000000"));
    assert!(status.lines().any(|l| l == "Seccomp:\t2"));
    assert!(
        fs::read_dir("/sys/class/net")
            .unwrap()
            .all(|e| e.unwrap().file_name() == "lo")
    );
    assert!(fs::write("/outside-scratch", b"refuse").is_err());
    assert!(fs::write("/input/overwrite", b"refuse").is_err());
    assert!(fs::write("/tmp/outside-scratch", b"refuse").is_err());
    fs::write("/scratch/probe", b"owned").unwrap();
    fs::remove_file("/scratch/probe").unwrap();
    let limits = fs::read_to_string("/proc/self/limits").unwrap();
    assert!(
        limits
            .lines()
            .any(|l| l.starts_with("Max file size") && l.contains("8388608"))
    );
    assert!(
        limits
            .lines()
            .any(|l| l.starts_with("Max cpu time") && l.contains("30"))
    );
    assert_eq!(
        fs::read_to_string("/sys/fs/cgroup/memory.max")
            .unwrap()
            .trim(),
        "536870912"
    );
    assert_eq!(
        fs::read_to_string("/sys/fs/cgroup/pids.max")
            .unwrap()
            .trim(),
        "32"
    );
    let cpu = fs::read_to_string("/sys/fs/cgroup/cpu.max").unwrap();
    let values: Vec<u64> = cpu.split_whitespace().map(|v| v.parse().unwrap()).collect();
    assert_eq!(values.len(), 2);
    assert!(values[0] <= values[1]);
}

fn worker() {
    use std::os::unix::process::CommandExt;
    let child = Command::new("/bin/sleep")
        .arg("30")
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut child = OwnedChild(child, None);
    fs::write("/scratch/owned.pid", child.id().to_string()).unwrap();
    #[repr(C)]
    struct Filter {
        code: u16,
        jt: u8,
        jf: u8,
        k: u32,
    }
    #[repr(C)]
    struct Program {
        len: u16,
        filter: *const Filter,
    }
    // Linux x86_64 only. Deny the actual group-kill syscall, never a Rust mock.
    let filters = [
        Filter {
            code: 0x20,
            jt: 0,
            jf: 0,
            k: 0,
        },
        Filter {
            code: 0x15,
            jt: 0,
            jf: 1,
            k: 62,
        },
        Filter {
            code: 0x06,
            jt: 0,
            jf: 0,
            k: 0x0005_0001,
        },
        Filter {
            code: 0x06,
            jt: 0,
            jf: 0,
            k: 0x7fff_0000,
        },
    ];
    let program = Program {
        len: filters.len() as u16,
        filter: filters.as_ptr(),
    };
    unsafe extern "C" {
        fn prctl(op: i32, ...) -> i32;
    }
    assert_eq!(unsafe { prctl(22, 2usize, &program, 0usize, 0usize) }, 0);
    let started = Instant::now();
    let error = child.terminate().unwrap_err();
    assert_eq!(error.raw_os_error(), Some(1));
    assert!(started.elapsed() < Duration::from_secs(1));
    drop(child); // The real guard reports cleanup failure without blocking forever.
    std::process::exit(23);
}
