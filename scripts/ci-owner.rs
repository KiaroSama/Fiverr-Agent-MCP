//! Dependency-free owner for the first Cargo build, before xtask exists.
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
#[cfg(windows)]
mod job {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateJobObjectW(a: *const c_void, b: *const u16) -> *mut c_void;
        fn SetInformationJobObject(h: *mut c_void, c: i32, v: *const c_void, n: u32) -> i32;
        fn AssignProcessToJobObject(h: *mut c_void, p: *mut c_void) -> i32;
        fn GetCurrentProcess() -> *mut c_void;
    }
    pub fn enter() -> Result<(), Box<dyn std::error::Error>> {
        // The guardian itself enters the kill-on-close Job before spawning anything.
        // Abrupt guardian termination closes its sole handle and kills all descendants.
        unsafe {
            let h = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if h.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            let mut limits = [0u64; 18];
            limits[2] = 0x2000; // JOBOBJECT_EXTENDED_LIMIT_INFORMATION.BasicLimitInformation.LimitFlags
            if SetInformationJobObject(h, 9, limits.as_ptr() as _, 144) == 0
                || AssignProcessToJobObject(h, GetCurrentProcess()) == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
            // Intentionally retained until process exit; closing it kills this guardian too.
        }
        Ok(())
    }
}
fn main() {
    if let Err(e) = run() {
        eprintln!("CI bootstrap owner: {e}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(windows)]
    job::enter()?;
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let executable = args.first().ok_or("missing owned command")?;
    let mut command = Command::new(executable);
    command
        .args(&args[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn()?;
    let (tx, rx) = mpsc::sync_channel(128);
    fn read(
        mut pipe: impl Read + Send + 'static,
        tx: mpsc::SyncSender<(bool, Vec<u8>)>,
        err: bool,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let mut b = [0u8; 4096];
            while let Ok(n) = pipe.read(&mut b) {
                if n == 0 || tx.send((err, b[..n].to_vec())).is_err() {
                    break;
                }
            }
        })
    }
    let out = read(
        child.stdout.take().ok_or("stdout missing")?,
        tx.clone(),
        false,
    );
    let err = read(
        child.stderr.take().ok_or("stderr missing")?,
        tx.clone(),
        true,
    );
    drop(tx);
    let start = Instant::now();
    let mut progress = start;
    let mut total = 0usize;
    let mut status = None;
    let mut failure = None;
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok((err, b)) => {
                total += b.len();
                progress = Instant::now();
                if total > 16 * 1024 * 1024 {
                    failure = Some("output ceiling exceeded");
                    break;
                }
                if err {
                    std::io::stderr().write_all(&b)?;
                } else {
                    std::io::stdout().write_all(&b)?;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if let Some(s) = child.try_wait()? {
                    status = Some(s);
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if start.elapsed() >= Duration::from_secs(900)
            || progress.elapsed() >= Duration::from_secs(180)
        {
            failure = Some("900s wall/180s idle deadline exceeded");
            break;
        }
    }
    #[cfg(unix)]
    {
        let _ = Command::new("/bin/kill")
            .args(["-KILL", "--", &format!("-{}", child.id())])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    if status.is_none() {
        let _ = child.kill();
    }
    let final_status = child.wait()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while !out.is_finished() || !err.is_finished() {
        let _ = rx.recv_timeout(Duration::from_millis(20));
        if Instant::now() >= deadline {
            return Err("bootstrap capture cleanup deadline exceeded".into());
        }
    }
    out.join().map_err(|_| "stdout reader panicked")?;
    err.join().map_err(|_| "stderr reader panicked")?;
    if let Some(e) = failure {
        return Err(e.into());
    }
    if !status.unwrap_or(final_status).success() {
        return Err("bootstrap command failed".into());
    }
    Ok(())
}
