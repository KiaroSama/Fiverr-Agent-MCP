//! Native equivalent of the previous protected-owner-only DACL acceptance.
#![allow(unsafe_code)]
use crate::Result;
use std::{fs, mem::size_of, os::windows::ffi::OsStrExt, path::Path};
use windows_sys::Win32::{
    Foundation::{CloseHandle, LocalFree},
    Security::{Authorization::*, *},
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

pub fn verify(root: &Path) -> Result<()> {
    // SAFETY: all returned handles/buffers remain live until after SID/ACE comparisons.
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err("current token unavailable".into());
        }
        let mut size = 0;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut size);
        let mut user = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
        let read = GetTokenInformation(token, TokenUser, user.as_mut_ptr() as _, size, &mut size);
        CloseHandle(token);
        if read == 0 {
            return Err("current owner SID unavailable".into());
        }
        let sid = (*(user.as_ptr() as *const TOKEN_USER)).User.Sid;
        for name in [
            ".state",
            ".state/workspace",
            ".state/workspace/workspace.sqlite3",
            "logs",
        ] {
            let path = root
                .join(name)
                .as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect::<Vec<_>>();
            let mut acl = std::ptr::null_mut();
            let mut descriptor = std::ptr::null_mut();
            if GetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut acl,
                std::ptr::null_mut(),
                &mut descriptor,
            ) != 0
            {
                return Err("private DACL unavailable".into());
            }
            let result = (|| -> Result<()> {
                let mut control = 0u16;
                let mut revision = 0;
                if GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) == 0
                    || control & SE_DACL_PROTECTED as u16 == 0
                    || acl.is_null()
                    || (*acl).AceCount == 0
                {
                    return Err("private DACL is not protected owner-only".into());
                }
                for index in 0..u32::from((*acl).AceCount) {
                    let mut ace = std::ptr::null_mut();
                    if GetAce(acl, index, &mut ace) == 0 {
                        return Err("private ACE unavailable".into());
                    }
                    let allowed = &*(ace as *const ACCESS_ALLOWED_ACE);
                    if allowed.Header.AceType != 0 // ACCESS_ALLOWED_ACE_TYPE
                        || EqualSid(sid, std::ptr::addr_of!(allowed.SidStart) as _) == 0
                    {
                        return Err("private DACL grants another identity".into());
                    }
                }
                Ok(())
            })();
            LocalFree(descriptor);
            result?;
        }
    }
    fs::remove_dir_all(root)?;
    Ok(())
}
