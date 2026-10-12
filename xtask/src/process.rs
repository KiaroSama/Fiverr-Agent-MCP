use crate::Result;
use std::{
    io::{Read, Write},
    path::Path,
    process::{Command, ExitStatus, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

pub struct Output {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

fn reader(
    mut stream: impl Read + Send + 'static,
    sender: mpsc::SyncSender<(bool, Vec<u8>)>,
    stderr: bool,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut bytes = [0_u8; 4096];
        while let Ok(count) = stream.read(&mut bytes) {
            if count == 0 || sender.send((stderr, bytes[..count].to_vec())).is_err() {
                break;
            }
        }
    })
}

#[cfg(unix)]
fn terminate(child: &mut std::process::Child) -> Result<()> {
    let status = Command::new("/bin/kill")
        .args(["-KILL", "--", &format!("-{}", child.id())])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    let _ = child.wait()?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let probe = Command::new("/bin/kill")
            .args(["-0", "--", &format!("-{}", child.id())])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if !probe.success() {
            return Ok(());
        }
        if !status.success() || Instant::now() >= deadline {
            return Err("owned process group still exists after termination".into());
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(windows)]
fn terminate(child: &mut std::process::Child) -> Result<()> {
    // The trusted PowerShell wrapper owns a kill-on-close non-breakaway Job.
    if child.try_wait()?.is_none() {
        child.kill()?;
    }
    Ok(())
}

struct Owner(std::process::Child);
impl std::ops::Deref for Owner {
    type Target = std::process::Child;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for Owner {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = terminate(&mut self.0);
        let _ = self.0.wait();
    }
}

pub fn run(executable: &str, args: &[String], cwd: &Path, wall: u64, idle: u64) -> Result<Output> {
    run_inner(executable, args, cwd, wall, idle, false)
}

pub fn smoke(executable: &str, args: &[String], cwd: &Path) -> Result<Output> {
    run_inner(executable, args, cwd, 15, 10, true)
}

fn run_inner(
    executable: &str,
    args: &[String],
    cwd: &Path,
    wall: u64,
    idle: u64,
    minimal: bool,
) -> Result<Output> {
    #[cfg(windows)]
    let mut command = {
        let pwsh = std::env::var_os("CI_PWSH")
            .ok_or("CI_PWSH must name the absolute PowerShell executable")?;
        let mut value = Command::new(pwsh);
        value.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-File"]);
        value.arg(crate::root().join("scripts/ci-owned.ps1"));
        value.args([
            "-Executable",
            executable,
            "-ArgumentsJson",
            &serde_json::to_string(args)?,
        ]);
        value.arg("-Directory").arg(cwd);
        value.args(["-Wall", &wall.to_string(), "-Idle", &idle.to_string()]);
        value
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut value = Command::new(executable);
        value.args(args);
        value
    };
    if minimal {
        #[cfg(windows)]
        {
            command.arg("-MinimalPath");
        }
        #[cfg(unix)]
        {
            command.env("PATH", "");
            for name in ["cargo", "rustc", "node", "python", "python3", "npm", "uv"] {
                if cwd.join(name).is_file() {
                    return Err("developer tool unexpectedly available in minimal CWD".into());
                }
            }
        }
    }
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
    let mut child = Owner(
        command
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?,
    );
    let (sender, receiver) = mpsc::sync_channel(128);
    let stdout_reader = reader(
        child.stdout.take().ok_or("stdout not captured")?,
        sender.clone(),
        false,
    );
    let stderr_reader = reader(
        child.stderr.take().ok_or("stderr not captured")?,
        sender.clone(),
        true,
    );
    drop(sender);
    let started = Instant::now();
    let mut progress = Instant::now();
    let mut output = Output {
        status: child.try_wait()?.unwrap_or_else(pending_status),
        stdout: Vec::new(),
        stderr: Vec::new(),
    };
    let mut exited = None;
    let mut streams_closed = false;
    let mut failure = None;
    loop {
        match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok((stderr, bytes)) => {
                progress = Instant::now();
                let buffer = if stderr {
                    &mut output.stderr
                } else {
                    &mut output.stdout
                };
                if buffer.len() + bytes.len() > 8 * 1024 * 1024 {
                    failure = Some("owned command output limit exceeded");
                    break;
                }
                buffer.extend_from_slice(&bytes);
                let written = if stderr {
                    std::io::stderr().write_all(&bytes)
                } else {
                    std::io::stdout().write_all(&bytes)
                };
                if written.is_err() {
                    failure = Some("owned output forwarding failed");
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => streams_closed = true,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if exited.is_none() {
            exited = child.try_wait()?;
        }
        if exited.is_some() && streams_closed {
            break;
        }
        if started.elapsed() >= Duration::from_secs(wall)
            || progress.elapsed() >= Duration::from_secs(idle)
        {
            failure = Some("owned command wall/idle deadline exceeded");
            break;
        }
    }
    // An exited root can leave descendants with inherited pipes; timeout is failure.
    let terminated = terminate(&mut child);
    let status = child.wait()?;
    let drain_deadline = Instant::now() + Duration::from_secs(5);
    while !streams_closed && Instant::now() < drain_deadline {
        match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => streams_closed = true,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
    if !streams_closed {
        return Err("owned command capture handles did not close".into());
    }
    stdout_reader.join().map_err(|_| "stdout reader panicked")?;
    stderr_reader.join().map_err(|_| "stderr reader panicked")?;
    terminated?;
    if let Some(message) = failure {
        return Err(message.into());
    }
    output.status = exited.unwrap_or(status);
    Ok(output)
}

fn pending_status() -> ExitStatus {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(0)
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::ExitStatusExt;
        ExitStatus::from_raw(0)
    }
}
