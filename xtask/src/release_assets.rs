use crate::{Result, validate};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub(super) fn validate_assets(
    directory: &Path,
    sha: &str,
    policy: &Value,
) -> Result<BTreeMap<String, PathBuf>> {
    let release: Value = serde_json::from_slice(&fs::read(
        directory.join("release-x86_64-pc-windows-msvc.json"),
    )?)?;
    if release["source_sha"] != sha
        || release["product"] != crate::PRODUCT
        || release["version"] != crate::VERSION
    {
        return Err("artifact identity mismatch".into());
    }
    let mut assets = BTreeMap::new();
    for line in fs::read_to_string(directory.join("SHA256SUMS-x86_64-pc-windows-msvc.txt"))?.lines()
    {
        let (hash, name) = line.split_once("  ").ok_or("malformed checksum line")?;
        if hash.len() != 64
            || !hash.bytes().all(|c| c.is_ascii_hexdigit())
            || Path::new(name).components().count() != 1
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        {
            return Err("unsafe checksum entry".into());
        }
        let path = directory.join(name);
        if validate::digest(&fs::read(&path)?) != hash {
            return Err("archive checksum mismatch".into());
        }
        assets.insert(name.to_owned(), path);
    }
    if assets.len() != 3 {
        return Err("incomplete signed distribution assets".into());
    }
    let receipt: Value =
        serde_json::from_slice(&fs::read(directory.join("windows-signing-receipt.json"))?)?;
    if receipt["source_sha"] != sha
        || receipt["signature_valid"] != true
        || receipt["certificate_sha256"] != policy["certificate_sha256"]
    {
        return Err("signing receipt mismatch".into());
    }
    for name in [
        "release-x86_64-pc-windows-msvc.json",
        "SHA256SUMS-x86_64-pc-windows-msvc.txt",
        "windows-signing-receipt.json",
    ] {
        assets.insert(name.into(), directory.join(name));
    }
    Ok(assets)
}

#[cfg(test)]
#[path = "release_tests.rs"]
mod tests;
