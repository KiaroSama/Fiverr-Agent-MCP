//! Native fixtures for the actual dependency-free bootstrap supervisor.
use super::{Limits, OwnedChild, supervise};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

const FIXTURE: &str = "bootstrap_owner_tests::owner_source::terminal_tests::bootstrap_tree_fixture";
static NEXT: AtomicUsize = AtomicUsize::new(0);
// Each fixture owns two native processes; do not nest independent fixture trees.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn bootstrap_tree_fixture() {
    let Some(directory) = std::env::var_os("FIVERR_BOOTSTRAP_TREE") else {
        return;
    };
    let directory = PathBuf::from(directory);
    let mode = std::env::var("FIVERR_BOOTSTRAP_TREE_MODE").unwrap();
    if mode == "descendant" {
        fs::write(directory.join("descendant"), std::process::id().to_string()).unwrap();
        loop {
            std::thread::park_timeout(Duration::from_millis(20));
        }
    }
    fs::write(directory.join("root"), std::process::id().to_string()).unwrap();
    let mut descendant = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", FIXTURE, "--nocapture"])
        .env("FIVERR_BOOTSTRAP_TREE", &directory)
        .env("FIVERR_BOOTSTRAP_TREE_MODE", "descendant")
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !directory.join("descendant").is_file() && Instant::now() < deadline {
        std::thread::park_timeout(Duration::from_millis(10));
    }
    if !directory.join("descendant").is_file() {
        let _ = descendant.kill();
        let _ = descendant.wait();
        panic!("descendant readiness missing");
    }
    // The supervisor owns this fresh group, including the deliberately orphaned child.
    std::mem::forget(descendant);
    fs::write(directory.join("ready"), b"ready").unwrap();
    match mode.as_str() {
        "success" => std::process::exit(0),
        "failure" => std::process::exit(7),
        "output" => loop {
            if std::io::stdout().write_all(&[b'x'; 4096]).is_err() {
                std::thread::park_timeout(Duration::from_millis(10));
            }
        },
        _ => loop {
            std::thread::park_timeout(Duration::from_millis(20));
        },
    }
}

struct Tree {
    child: Option<OwnedChild>,
    directory: PathBuf,
    identities: Vec<(u32, String)>,
}
impl Tree {
    fn spawn(mode: &str) -> Self {
        use std::os::unix::process::CommandExt;
        let directory = crate::root().join(".ci-work").join(format!(
            "bootstrap-terminal-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", FIXTURE, "--nocapture"])
            .env("FIVERR_BOOTSTRAP_TREE", &directory)
            .env("FIVERR_BOOTSTRAP_TREE_MODE", mode)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .unwrap();
        let mut tree = Self {
            child: Some(OwnedChild(child, None)),
            directory,
            identities: Vec::new(),
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while !tree.directory.join("ready").is_file() && Instant::now() < deadline {
            std::thread::park_timeout(Duration::from_millis(10));
        }
        assert!(
            tree.directory.join("ready").is_file(),
            "tree readiness missing"
        );
        for name in ["root", "descendant"] {
            let pid = fs::read_to_string(tree.directory.join(name))
                .unwrap()
                .parse()
                .unwrap();
            if let Some((_, identity)) = process_identity(pid) {
                tree.identities.push((pid, identity));
            } else {
                assert!(name == "root" && matches!(mode, "success" | "failure"));
            }
        }
        assert!(
            tree.identities
                .iter()
                .any(|(pid, _)| { *pid != tree.child.as_ref().unwrap().id() })
        );
        tree
    }
    fn assert_stopped(&self) {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let live = self.identities.iter().any(|(pid, expected)| {
                process_identity(*pid).is_some_and(|(state, identity)| {
                    identity == *expected && !state.starts_with('Z')
                })
            });
            if !live {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "owned tree survived terminal path"
            );
            std::thread::park_timeout(Duration::from_millis(10));
        }
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        drop(self.child.take());
        // Preserve unresolved diagnostics on assertion/cleanup failure.
        if !std::thread::panicking()
            && self.identities.iter().all(|(pid, expected)| {
                process_identity(*pid)
                    .is_none_or(|(state, identity)| identity != *expected || state.starts_with('Z'))
            })
        {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }
}

fn process_identity(pid: u32) -> Option<(String, String)> {
    let output = Command::new("/bin/ps")
        .args([
            "-o",
            "stat=",
            "-o",
            "lstart=",
            "-o",
            "comm=",
            "-p",
            &pid.to_string(),
        ])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    let (state, identity) = text.trim().split_once(char::is_whitespace)?;
    Some((state.to_owned(), identity.trim().to_owned()))
}

#[test]
fn bootstrap_supervisor_reaps_tree_on_each_terminal_path() {
    let _serial = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    for case in [
        "success",
        "failure",
        "poll",
        "interrupted_poll",
        "output",
        "idle",
        "wall",
    ] {
        let mut tree = Tree::spawn(case);
        let limits = Limits {
            wall: Duration::from_millis(if case == "wall" { 150 } else { 5000 }),
            idle: Duration::from_millis(if case == "idle" { 150 } else { 5000 }),
            output: if case == "output" { 1024 } else { 64 * 1024 },
        };
        let mut forwarded = 0usize;
        let result = supervise(
            tree.child.take().unwrap(),
            limits,
            |child| match case {
                "poll" => Err(std::io::Error::other("injected status failure")),
                "interrupted_poll" => Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "cancelled",
                )),
                _ => Child::try_wait(child),
            },
            |_, bytes| {
                forwarded += bytes.len();
                Ok(())
            },
        );
        match case {
            "success" => assert!(result.is_ok(), "{case}: {result:?}"),
            "failure" => assert_eq!(result.unwrap_err().to_string(), "bootstrap command failed"),
            "poll" | "interrupted_poll" => {
                assert_eq!(result.unwrap_err().to_string(), "child status query failed")
            }
            "output" => assert_eq!(result.unwrap_err().to_string(), "output ceiling exceeded"),
            _ => assert_eq!(
                result.unwrap_err().to_string(),
                "900s wall/180s idle deadline exceeded"
            ),
        }
        assert!(forwarded <= limits.output);
        tree.assert_stopped();
        let root_pid = fs::read_to_string(tree.directory.join("root"))
            .unwrap()
            .parse()
            .unwrap();
        assert!(process_identity(root_pid).is_none(), "root was not reaped");
    }
}

#[test]
fn bootstrap_guard_drop_reaps_tree_before_supervision() {
    let _serial = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    let mut tree = Tree::spawn("idle");
    let child = tree.child.take().unwrap();
    let pid = child.id();
    drop(child);
    tree.assert_stopped();
    assert!(
        process_identity(pid).is_none(),
        "root must be reaped, not merely killed"
    );
}

#[test]
fn bootstrap_missing_executable_and_production_limits_are_explicit() {
    let missing = crate::root().join(".ci-work/nonexistent-bootstrap-command");
    assert!(!missing.exists());
    assert!(Command::new(missing).spawn().is_err());
    assert_eq!(Limits::PRODUCTION.wall, Duration::from_secs(900));
    assert_eq!(Limits::PRODUCTION.idle, Duration::from_secs(180));
    assert_eq!(Limits::PRODUCTION.output, 16 * 1024 * 1024);
}
