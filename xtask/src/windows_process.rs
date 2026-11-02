//! Creation-time Windows Job ownership with allowlisted inherited pipe handles.
#![allow(unsafe_code)]
use crate::Result;
use std::{
    fs::File,
    mem::{size_of, size_of_val},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::ExitStatusExt,
    },
    process::{Command, ExitStatus},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation},
    Security::SECURITY_ATTRIBUTES,
    System::{JobObjects::*, Pipes::CreatePipe, Threading::*},
};

pub struct Job(OwnedHandle);
pub struct Child {
    process: OwnedHandle,
    pid: u32,
    pub stdout: Option<File>,
    pub stderr: Option<File>,
}
fn check(value: i32) -> Result<()> {
    if value == 0 {
        Err(std::io::Error::last_os_error().into())
    } else {
        Ok(())
    }
}
unsafe fn handle(raw: HANDLE) -> Result<OwnedHandle> {
    if raw.is_null() || raw as isize == -1 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: caller transfers a newly created, uniquely owned valid OS handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
}
fn quote(value: &std::ffi::OsStr) -> Vec<u16> {
    let mut result = vec![34];
    let mut slashes = 0;
    for c in value.encode_wide() {
        if c == 92 {
            slashes += 1;
            continue;
        }
        result.extend(std::iter::repeat_n(
            92,
            if c == 34 { slashes * 2 + 1 } else { slashes },
        ));
        result.push(c);
        slashes = 0;
    }
    result.extend(std::iter::repeat_n(92, slashes * 2));
    result.push(34);
    result
}
impl Child {
    pub fn id(&self) -> u32 {
        self.pid
    }
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        // SAFETY: owned process handle remains valid and code output is writable.
        unsafe {
            match WaitForSingleObject(self.process.as_raw_handle(), 0) {
                0 => {
                    let mut code = 0;
                    check(GetExitCodeProcess(self.process.as_raw_handle(), &mut code))?;
                    Ok(Some(ExitStatus::from_raw(code)))
                }
                258 => Ok(None),
                _ => Err(std::io::Error::last_os_error().into()),
            }
        }
    }
    pub fn wait(&mut self) -> Result<ExitStatus> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.try_wait()? {
                return Ok(status);
            }
            if Instant::now() >= deadline {
                return Err("owned process did not exit".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Job {
    pub fn spawn(command: &mut Command) -> Result<(Child, Self)> {
        // SAFETY: SDK layouts, exact sizes, live backing arrays and RAII handles cover every call.
        unsafe {
            let job = Self(handle(CreateJobObjectW(
                std::ptr::null(),
                std::ptr::null(),
            ))?);
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            check(SetInformationJobObject(
                job.0.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                &limits as *const _ as _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ))?;
            let security = SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: std::ptr::null_mut(),
                bInheritHandle: 1,
            };
            let pipe = || -> Result<(OwnedHandle, OwnedHandle)> {
                let mut read = std::ptr::null_mut();
                let mut write = std::ptr::null_mut();
                check(CreatePipe(&mut read, &mut write, &security, 0))?;
                let read = handle(read)?;
                let write = handle(write)?;
                check(SetHandleInformation(
                    read.as_raw_handle(),
                    HANDLE_FLAG_INHERIT,
                    0,
                ))?;
                Ok((read, write))
            };
            let (out_read, out_write) = pipe()?;
            let (err_read, err_write) = pipe()?;
            let (in_read, in_write) = pipe()?;
            drop(in_write);
            // Only stdin's child end inherits; parent ends and Job handle never do.
            check(SetHandleInformation(
                in_read.as_raw_handle(),
                HANDLE_FLAG_INHERIT,
                HANDLE_FLAG_INHERIT,
            ))?;
            let mut size = 0usize;
            InitializeProcThreadAttributeList(std::ptr::null_mut(), 2, 0, &mut size);
            let mut buffer = vec![0usize; size.div_ceil(size_of::<usize>())];
            let list = buffer.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
            check(InitializeProcThreadAttributeList(list, 2, 0, &mut size))?;
            let result = (|| -> Result<(Child, Self)> {
                let mut jobs = [job.0.as_raw_handle()];
                check(UpdateProcThreadAttribute(
                    list,
                    0,
                    0x2000d,
                    jobs.as_mut_ptr() as _,
                    size_of::<HANDLE>(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                ))?;
                let mut handles = [
                    in_read.as_raw_handle(),
                    out_write.as_raw_handle(),
                    err_write.as_raw_handle(),
                ];
                check(UpdateProcThreadAttribute(
                    list,
                    0,
                    0x20002,
                    handles.as_mut_ptr() as _,
                    size_of_val(&handles),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                ))?;
                let mut startup = STARTUPINFOEXW::default();
                startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
                startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
                startup.StartupInfo.hStdInput = in_read.as_raw_handle();
                startup.StartupInfo.hStdOutput = out_write.as_raw_handle();
                startup.StartupInfo.hStdError = err_write.as_raw_handle();
                startup.lpAttributeList = list;
                let mut line = quote(command.get_program());
                for arg in command.get_args() {
                    line.push(32);
                    line.extend(quote(arg));
                }
                line.push(0);
                if line.len() > 32767 || line[..line.len() - 1].contains(&0) {
                    return Err("invalid Windows command length/NUL".into());
                }
                let mut env: std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString> =
                    std::env::vars_os().collect();
                for (k, v) in command.get_envs() {
                    if let Some(v) = v {
                        env.insert(k.to_owned(), v.to_owned());
                    } else {
                        env.remove(k);
                    }
                }
                let mut environment = Vec::<u16>::new();
                for (k, v) in env {
                    environment.extend(k.encode_wide());
                    environment.push(61);
                    environment.extend(v.encode_wide());
                    environment.push(0);
                }
                environment.push(0);
                let cwd = command
                    .get_current_dir()
                    .ok_or("owned command CWD missing")?
                    .as_os_str()
                    .encode_wide()
                    .chain(Some(0))
                    .collect::<Vec<_>>();
                let mut info = PROCESS_INFORMATION::default();
                check(CreateProcessW(
                    std::ptr::null(),
                    line.as_mut_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    1,
                    CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT,
                    environment.as_ptr() as _,
                    cwd.as_ptr(),
                    &startup.StartupInfo,
                    &mut info,
                ))?;
                let process = handle(info.hProcess)?;
                CloseHandle(info.hThread);
                Ok((
                    Child {
                        process,
                        pid: info.dwProcessId,
                        stdout: Some(File::from(out_read)),
                        stderr: Some(File::from(err_read)),
                    },
                    job,
                ))
            })();
            DeleteProcThreadAttributeList(list);
            result
        }
    }
    pub fn active(&self) -> Result<u32> {
        let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        unsafe {
            check(QueryInformationJobObject(
                self.0.as_raw_handle(),
                JobObjectBasicAccountingInformation,
                &mut accounting as *mut _ as _,
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                std::ptr::null_mut(),
            ))?;
        }
        Ok(accounting.ActiveProcesses)
    }
    pub fn terminate(&self) -> Result<()> {
        unsafe {
            check(TerminateJobObject(self.0.as_raw_handle(), 1))?;
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.active()? != 0 {
            if Instant::now() >= deadline {
                return Err("owned Job did not reach zero active processes".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }
}
