//! Windows ordinal environment identity; preserve UTF-16 without locale conversion.
#![allow(unsafe_code)]
use crate::Result;
use std::{
    cmp::Ordering,
    ffi::{OsStr, OsString},
    os::windows::ffi::OsStrExt,
};
use windows_sys::Win32::Globalization::CompareStringOrdinal;

type Entry = (Vec<u16>, Vec<u16>);

fn compare(left: &[u16], right: &[u16]) -> Result<Ordering> {
    // SAFETY: initialized UTF-16 buffers remain live for the explicit checked lengths.
    let value = unsafe {
        CompareStringOrdinal(
            left.as_ptr(),
            i32::try_from(left.len())?,
            right.as_ptr(),
            i32::try_from(right.len())?,
            1,
        )
    };
    match value {
        1 => Ok(Ordering::Less),
        2 => Ok(Ordering::Equal),
        3 => Ok(Ordering::Greater),
        _ => Err(std::io::Error::last_os_error().into()),
    }
}

fn key(value: &OsStr, inherited: bool) -> Result<Vec<u16>> {
    let wide: Vec<_> = value.encode_wide().collect();
    // Hidden per-drive entries are inherited only; commands cannot create arbitrary '=' keys.
    let hidden_drive = inherited
        && wide.len() == 3
        && wide[0] == 61
        && ((65..=90).contains(&wide[1]) || (97..=122).contains(&wide[1]))
        && wide[2] == 58;
    if wide.is_empty() || wide.contains(&0) || (!hidden_drive && wide.contains(&61)) {
        return Err("invalid Windows environment name".into());
    }
    i32::try_from(wide.len())?;
    Ok(wide)
}

pub(super) fn block(
    inherited: impl IntoIterator<Item = (OsString, OsString)>,
    changes: impl IntoIterator<Item = (OsString, Option<OsString>)>,
) -> Result<Vec<u16>> {
    let mut entries: Vec<Entry> = Vec::new();
    for (name, value, inherited) in inherited
        .into_iter()
        .map(|(k, v)| (k, Some(v), true))
        .chain(changes.into_iter().map(|(k, v)| (k, v, false)))
    {
        let name = key(&name, inherited)?;
        let value = value.map(|v| v.encode_wide().collect::<Vec<_>>());
        if value.as_ref().is_some_and(|value| value.contains(&0)) {
            return Err("invalid Windows environment value".into());
        }
        let mut position = entries.len();
        for (index, (old, _)) in entries.iter().enumerate() {
            match compare(old, &name)? {
                Ordering::Less => {}
                Ordering::Equal | Ordering::Greater => {
                    position = index;
                    break;
                }
            }
        }
        if position < entries.len() && compare(&entries[position].0, &name)? == Ordering::Equal {
            entries.remove(position);
        }
        if let Some(value) = value {
            entries.insert(position, (name, value));
        }
    }
    let mut result = Vec::new();
    for (name, value) in entries {
        result.extend(name);
        result.push(61);
        result.extend(value);
        result.push(0);
    }
    if result.is_empty() {
        result.push(0);
    }
    result.push(0);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordinal_identity_ordering_and_termination() {
        let initial = [
            ("Path", "old"),
            ("Å", "old"),
            ("z", "last"),
            ("=C:", "C:\\fixture"),
        ]
        .map(|(k, v)| (OsString::from(k), OsString::from(v)));
        let changes = [("PATH", Some("minimal")), ("å", None)]
            .map(|(k, v)| (OsString::from(k), v.map(OsString::from)));
        let result = block(initial, changes).unwrap();
        assert_eq!(
            result,
            "=C:=C:\\fixture\0PATH=minimal\0z=last\0\0"
                .encode_utf16()
                .collect::<Vec<_>>()
        );
        assert_eq!(block([], []).unwrap(), vec![0, 0]);
        assert_eq!(compare(&[0x00e5], &[0x00c5]).unwrap(), Ordering::Equal);
        use std::os::windows::ffi::OsStringExt;
        let lone_surrogate = OsString::from_wide(&[0xd800]);
        assert!(
            block([(OsString::from("DUMMY"), lone_surrogate)], [])
                .unwrap()
                .contains(&0xd800)
        );
    }
    #[test]
    fn invalid_environment_names_and_values_are_refused() {
        for (name, value) in [
            ("", "x"),
            ("A=B", "x"),
            ("A\0B", "x"),
            ("A", "x\0y"),
            ("=C:", "x"),
        ] {
            assert!(block([], [(OsString::from(name), Some(OsString::from(value)))]).is_err());
        }
        let result = block(
            [("Path".into(), "a".into()), ("PATH".into(), "b".into())],
            [],
        )
        .unwrap();
        assert_eq!(result, "PATH=b\0\0".encode_utf16().collect::<Vec<_>>());
    }
}
