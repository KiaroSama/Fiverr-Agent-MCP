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
        drop(cleanup.owner.stdout.take());
        let deadline = Instant::now() + Duration::from_secs(5);
        while !cleanup.path.is_file() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let pid: u32 = fs::read_to_string(&cleanup.path)
            .expect("fixture PID must be recorded before output")
            .parse()
            .unwrap();
        cleanup.group = Some(pid);
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
