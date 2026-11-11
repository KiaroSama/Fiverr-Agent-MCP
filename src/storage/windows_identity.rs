//! Current process identity for the existing bounded Windows ACL adapter.
#![allow(unsafe_code)]
use crate::core::{Result, storage_error};
use std::{
    mem::size_of,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
};
use windows_sys::Win32::{
    Foundation::{ERROR_INSUFFICIENT_BUFFER, GetLastError, LocalFree},
    Security::{
        Authorization::ConvertSidToStringSidW, GetTokenInformation, TOKEN_QUERY, TOKEN_USER,
        TokenUser,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

pub(super) fn current_sid() -> Result<String> {
    // SAFETY: token ownership is transferred to OwnedHandle. Pointer-aligned token
    // storage remains alive through SID conversion; both API calls must succeed.
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(storage_error());
        }
        let token = OwnedHandle::from_raw_handle(token);
        let mut length = 0;
        let first = GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            std::ptr::null_mut(),
            0,
            &mut length,
        );
        if first != 0
            || GetLastError() != ERROR_INSUFFICIENT_BUFFER
            || !(size_of::<TOKEN_USER>()..=65536).contains(&(length as usize))
        {
            return Err(storage_error());
        }
        let capacity = length;
        let mut buffer = vec![0usize; (capacity as usize).div_ceil(size_of::<usize>())];
        if GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            capacity,
            &mut length,
        ) == 0
            || length > capacity
            || (length as usize) < size_of::<TOKEN_USER>()
        {
            return Err(storage_error());
        }
        let sid = (*(buffer.as_ptr().cast::<TOKEN_USER>())).User.Sid;
        let mut text = std::ptr::null_mut();
        if ConvertSidToStringSidW(sid, &mut text) == 0 || text.is_null() {
            return Err(storage_error());
        }
        // Win32 returns a valid terminated allocation. Copy it before LocalFree;
        // retain a defensive bound above the maximum standard SID string length.
        let result = (|| {
            let mut count = 0;
            while count < 184 && *text.add(count) != 0 {
                count += 1;
            }
            if count == 184 {
                return Err(storage_error());
            }
            let value = String::from_utf16(std::slice::from_raw_parts(text, count))
                .map_err(|_| storage_error())?;
            if !value.starts_with("S-1-")
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b == b'S' || b == b'-')
            {
                return Err(storage_error());
            }
            Ok(value)
        })();
        LocalFree(text.cast());
        result
    }
}
