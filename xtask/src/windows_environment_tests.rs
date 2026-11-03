use std::{
    io::Read,
    process::Command,
    time::{Duration, Instant},
};

#[test]
fn environment_child_fixture() {
    let Ok(role) = std::env::var("FIVERR_ENVIRONMENT_FIXTURE") else {
        return;
    };
    if role == "parent" {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "windows_environment_tests::environment_child_fixture",
                "--nocapture",
            ])
            .current_dir(crate::root())
            .env("FIVERR_ENVIRONMENT_FIXTURE", "leaf")
            .env_remove("FIVERR_MIXEDCASEDUMMY");
        let (mut child, job) = crate::windows_process::Job::spawn(&mut command).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            if Instant::now() >= deadline {
                job.terminate().unwrap();
                panic!("owned dummy environment deadline");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut text = String::new();
        child
            .stdout
            .take()
            .unwrap()
            .take(4096)
            .read_to_string(&mut text)
            .unwrap();
        job.terminate().unwrap();
        assert!(
            text.contains("dummy_present=false"),
            "mixed-case dummy removal did not reach child"
        );
    } else {
        // Only presence is returned; no ambient environment value is disclosed.
        println!(
            "system_root_present={}",
            std::env::var_os("SystemRoot").is_some()
        );
        println!(
            "dummy_present={}",
            std::env::var_os("FIVERR_MixedCaseDummy").is_some()
        );
    }
}

#[test]
fn windows_child_environment_honors_case_insensitive_removal() {
    assert!(
        std::env::var_os("SystemRoot").is_some(),
        "native Windows prerequisite missing"
    );
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "windows_environment_tests::environment_child_fixture",
            "--nocapture",
        ])
        .current_dir(crate::root())
        .env("FIVERR_ENVIRONMENT_FIXTURE", "1")
        .env_remove("SYSTEMROOT");
    let (mut child, job) = crate::windows_process::Job::spawn(&mut command).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            job.terminate().unwrap();
            panic!("owned environment fixture deadline exceeded");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut text = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .take(4096)
        .read_to_string(&mut text)
        .unwrap();
    job.terminate().unwrap();
    assert!(status.success());
    assert!(
        text.contains("system_root_present=false"),
        "mixed-case removal did not reach actual child"
    );
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "windows_environment_tests::environment_child_fixture",
            "--nocapture",
        ])
        .current_dir(crate::root())
        .env("FIVERR_ENVIRONMENT_FIXTURE", "parent")
        .env("FIVERR_MixedCaseDummy", "dummy");
    let (mut child, job) = crate::windows_process::Job::spawn(&mut command).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            job.terminate().unwrap();
            assert!(status.success(), "dummy parent contract failed");
            break;
        }
        if Instant::now() >= deadline {
            job.terminate().unwrap();
            panic!("dummy parent exceeded deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
