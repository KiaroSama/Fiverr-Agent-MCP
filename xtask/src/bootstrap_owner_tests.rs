#[cfg(unix)]
#[allow(dead_code, unsafe_code)]
#[path = "../../scripts/ci-owner.rs"]
mod owner_source;

#[cfg(unix)]
mod unix {
    use std::{
        fs,
        io::Write,
        path::PathBuf,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    #[test]
    fn bootstrap_output_fixture() {
        let Some(path) = std::env::var_os("FIVERR_BOOTSTRAP_FIXTURE_PID") else {
            return;
        };
        fs::write(path, std::process::id().to_string()).unwrap();
        loop {
            if std::io::stdout().write_all(&[b'x'; 4096]).is_err() {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    #[test]
    fn bootstrap_descendant_fixture() {
        let Some(path) = std::env::var_os("FIVERR_BOOTSTRAP_DESCENDANT_PID") else {
            return;
        };
        fs::write(path, std::process::id().to_string()).unwrap();
        loop {
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn bootstrap_early_root_fixture() {
        let Some(path) = std::env::var_os("FIVERR_BOOTSTRAP_DESCENDANT_PID") else {
            return;
        };
        fs::write(&path, std::process::id().to_string()).unwrap();
        let descendant_path = std::path::Path::new(&path).with_extension("descendant.pid");
        let mut descendant = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "bootstrap_owner_tests::unix::bootstrap_descendant_fixture",
                "--nocapture",
            ])
            .env("FIVERR_BOOTSTRAP_DESCENDANT_PID", &descendant_path)
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !descendant_path.is_file() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        if !descendant_path.is_file() {
            let _ = descendant.kill();
            let _ = descendant.wait();
            panic!("descendant failed to record identity");
        }
        // Deliberately orphan this fixture into the owner's fresh process group.
        // Its inherited capture pipes must be closed by owner cleanup, not here.
        std::mem::forget(descendant);
    }

    struct Cleanup {
        owner: std::process::Child,
        group: Option<u32>,
        path: PathBuf,
    }
    impl Drop for Cleanup {
        fn drop(&mut self) {
            if let Some(group) = self.group {
                let _ = Command::new("/bin/kill")
                    .args(["-KILL", "--", &format!("-{group}")])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
            let _ = self.owner.kill();
            let _ = self.owner.wait();
            let _ = fs::remove_file(&self.path);
        }
    }
    fn alive(pid: u32) -> bool {
        Command::new("/bin/kill")
            .args(["-0", "--", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    }

    #[test]
    fn bootstrap_owner_reaps_inherited_pipes_after_root_exit() {
        let path = crate::root()
            .join(".ci-work")
            .join(format!("bootstrap-early-root-{}.pid", std::process::id()));
        let descendant_path = path.with_extension("descendant.pid");
        assert!(!path.exists() && !descendant_path.exists());
        let owner = Command::new(crate::root().join(".ci-work/owner"))
            .arg(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "bootstrap_owner_tests::unix::bootstrap_early_root_fixture",
                "--nocapture",
            ])
            .env("FIVERR_BOOTSTRAP_DESCENDANT_PID", &path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut cleanup = Cleanup {
            owner,
            group: None,
            path,
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while !cleanup.path.is_file() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        cleanup.group = Some(fs::read_to_string(&cleanup.path).unwrap().parse().unwrap());
        let mut status = None;
        while Instant::now() < deadline {
            status = cleanup.owner.try_wait().unwrap();
            if status.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let recorded = fs::read_to_string(&descendant_path);
        let stopped = recorded.as_deref().is_ok_and(|pid| {
            let pid: u32 = pid.parse().unwrap();
            let output = Command::new("/bin/ps")
                .args(["-o", "stat=", "-p", &pid.to_string()])
                .stdin(Stdio::null())
                .output()
                .unwrap();
            // A killed orphan can await init's reap; a zombie cannot retain pipes.
            !alive(pid)
                || String::from_utf8(output.stdout)
                    .unwrap()
                    .trim()
                    .starts_with('Z')
        });
        if descendant_path.exists() {
            fs::remove_file(descendant_path).unwrap();
        }
        assert!(recorded.is_ok(), "descendant identity missing");
        assert!(
            status.is_some_and(|value| value.success()),
            "root exit left inherited pipes alive"
        );
        assert!(stopped, "live descendant survived root exit");
    }

    #[test]
    fn bootstrap_owner_reaps_children_after_forwarding_failure() {
        let path = crate::root()
            .join(".ci-work")
            .join(format!("bootstrap-fixture-{}.pid", std::process::id()));
        assert!(!path.exists());
        let mut owner = Command::new(crate::root().join(".ci-work/owner"));
        owner
            .arg(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "bootstrap_owner_tests::unix::bootstrap_output_fixture",
                "--nocapture",
            ])
            .env("FIVERR_BOOTSTRAP_FIXTURE_PID", &path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut cleanup = Cleanup {
            owner: owner.spawn().unwrap(),
            group: None,
            path,
        };
        // libtest writes its banner before entering the fixture; keep the sink open
        // until the recorded PID proves the owned child actually started.
        let deadline = Instant::now() + Duration::from_secs(5);
        while !cleanup.path.is_file() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let pid: u32 = fs::read_to_string(&cleanup.path)
            .expect("fixture PID must be recorded before output")
            .parse()
            .unwrap();
        cleanup.group = Some(pid);
        drop(cleanup.owner.stdout.take());
        let mut status = None;
        while Instant::now() < deadline {
            status = cleanup.owner.try_wait().unwrap();
            if status.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            status.is_some_and(|status| !status.success()),
            "closed forwarding sink must fail promptly"
        );
        while alive(pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !alive(pid),
            "bootstrap child survived its owner's forwarding error"
        );
    }
}
