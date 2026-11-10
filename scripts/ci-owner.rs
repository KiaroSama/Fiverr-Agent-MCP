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
struct OwnedChild(std::process::Child, Option<std::process::ExitStatus>);
impl std::ops::Deref for OwnedChild {
    type Target = std::process::Child;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for OwnedChild {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl OwnedChild {
    fn terminate(&mut self) -> std::io::Result<std::process::ExitStatus> {
        if let Some(status) = self.1 {
            return Ok(status);
        }
        #[cfg(unix)]
        {
            unsafe extern "C" {
                fn kill(pid: i32, signal: i32) -> i32;
            }
            // SAFETY: the child was created in a fresh group whose id is its positive PID.
            let result = unsafe { kill(-(self.id() as i32), 9) };
            if result != 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(3) {
                    return Err(error);
                }
            }
        }
        let _ = self.0.kill();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.0.try_wait()? {
                self.1 = Some(status);
                return Ok(status);
            }
            if Instant::now() >= deadline {
                return Err(std::io::Error::other(
                    "bootstrap child reap deadline exceeded",
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Err(error) = self.terminate() {
            eprintln!("CI bootstrap cleanup failed: {error}");
        }
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
    supervise(
        OwnedChild(command.spawn()?, None),
        Limits::PRODUCTION,
        std::process::Child::try_wait,
        |err, bytes| {
            if err {
                std::io::stderr().write_all(bytes)
            } else {
                std::io::stdout().write_all(bytes)
            }
        },
    )
}

#[derive(Clone, Copy)]
struct Limits {
    wall: Duration,
    idle: Duration,
    output: usize,
}
impl Limits {
    const PRODUCTION: Self = Self {
        wall: Duration::from_secs(900),
        idle: Duration::from_secs(180),
        output: 16 * 1024 * 1024,
    };
}

fn supervise(
    mut child: OwnedChild,
    limits: Limits,
    mut poll: impl FnMut(&mut std::process::Child) -> std::io::Result<Option<std::process::ExitStatus>>,
    mut forward: impl FnMut(bool, &[u8]) -> std::io::Result<()>,
) -> Result<(), Box<dyn std::error::Error>> {
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
    let mut drain_deadline = None;
    let mut streams_closed = false;
    let mut failure = None;
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok((err, b)) => {
                total += b.len();
                progress = Instant::now();
                if total > limits.output {
                    failure = Some("output ceiling exceeded");
                    break;
                }
                if forward(err, &b).is_err() {
                    failure = Some("output forwarding failed");
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => streams_closed = true,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if status.is_none() {
            match poll(&mut child.0) {
                Ok(Some(s)) => {
                    status = Some(s);
                    // Descendants may retain pipes after the root exits. Terminate
                    // the owned group, then keep forwarding already captured bytes.
                    child.terminate()?;
                    drain_deadline = Some(Instant::now() + Duration::from_secs(5));
                }
                Ok(None) => {}
                Err(_) => {
                    failure = Some("child status query failed");
                    break;
                }
            }
        }
        if status.is_some() && streams_closed {
            break;
        }
        if drain_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            failure = Some("bootstrap capture cleanup deadline exceeded");
            break;
        }
        if start.elapsed() >= limits.wall || progress.elapsed() >= limits.idle {
            failure = Some("900s wall/180s idle deadline exceeded");
            break;
        }
    }
    let final_status = child.terminate()?;
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

#[cfg(all(test, unix))]
#[path = "../xtask/src/bootstrap_terminal_tests.rs"]
mod terminal_tests;
