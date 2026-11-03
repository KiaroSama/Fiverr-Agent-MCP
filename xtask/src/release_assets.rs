use crate::{Result, archives, validate};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const TARGET: &str = "x86_64-pc-windows-msvc";

fn input(directory: &Path, name: &str, limit: u64) -> Result<Vec<u8>> {
    validate::path(name)?;
    for path in [directory.to_path_buf(), directory.join(name)] {
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err("linked release input refused".into());
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err("reparse release input refused".into());
            }
        }
    }
    let path = directory.join(name);
    if !path.is_file() || !path.canonicalize()?.starts_with(directory.canonicalize()?) {
        return Err("release input outside directory or not a file".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err("release input exceeds limit".into());
    }
    Ok(bytes)
}

pub(super) fn validate_assets(
    directory: &Path,
    sha: &str,
    policy: &Value,
) -> Result<BTreeMap<String, PathBuf>> {
    let head = crate::ci::owned(15, 10, "git", &["rev-parse", "HEAD"])?;
    if std::str::from_utf8(&head.stdout)?.trim() != sha {
        return Err("publication checkout differs from verified source".into());
    }
    validate_assets_against(
        directory,
        sha,
        policy,
        &crate::package::source(&crate::root())?,
    )
}

fn validate_assets_against(
    directory: &Path,
    sha: &str,
    policy: &Value,
    expected_source: &archives::Files,
) -> Result<BTreeMap<String, PathBuf>> {
    crate::sha(sha)?;
    let release: Value = serde_json::from_slice(&input(
        directory,
        &format!("release-{TARGET}.json"),
        1024 * 1024,
    )?)?;
    if release["source_sha"] != sha
        || release["product"] != crate::PRODUCT
        || release["version"] != crate::VERSION
        || release["target"] != TARGET
    {
        return Err("artifact identity mismatch".into());
    }
    let expected = [
        format!("{}-{}-{TARGET}.zip", crate::PRODUCT, crate::VERSION),
        format!("{}-{}-source.tar.gz", crate::PRODUCT, crate::VERSION),
        format!("fiverr-gig-optimizer-{}-plugin.zip", crate::VERSION),
    ];
    let sums = input(directory, &format!("SHA256SUMS-{TARGET}.txt"), 4096)?;
    let mut assets = BTreeMap::new();
    for line in std::str::from_utf8(&sums)?.lines() {
        let (digest, name) = line.split_once("  ").ok_or("malformed checksum line")?;
        if !expected.iter().any(|expected| expected == name)
            || assets.contains_key(name)
            || digest.len() != 64
            || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("unexpected or duplicate checksum entry".into());
        }
        if validate::digest(&input(directory, name, 128 * 1024 * 1024)?) != digest {
            return Err("archive checksum mismatch".into());
        }
        assets.insert(name.to_owned(), directory.join(name));
    }
    if assets.len() != expected.len() {
        return Err("incomplete signed distribution assets".into());
    }
    let native = archives::read(&assets[&expected[0]])?;
    let embedded: Value = serde_json::from_slice(
        native
            .get("release.json")
            .ok_or("embedded release metadata missing")?,
    )?;
    if embedded != release {
        return Err("external and embedded release metadata disagree".into());
    }
    let entries = embedded["files"]
        .as_object()
        .ok_or("release member manifest missing")?;
    if entries.len() + 1 != native.len() {
        return Err("native archive member inventory mismatch".into());
    }
    for (name, digest) in entries {
        validate::path(name)?;
        let bytes = native.get(name).ok_or("declared archive member missing")?;
        if digest.as_str() != Some(validate::digest(bytes).as_str()) {
            return Err("archive member digest mismatch".into());
        }
    }
    let executable = native
        .get("bin/fiverr-agent-mcp.exe")
        .ok_or("packaged executable missing")?;
    let receipt: Value = serde_json::from_slice(&input(
        directory,
        "windows-signing-receipt.json",
        64 * 1024,
    )?)?;
    let pin = policy["certificate_sha256"]
        .as_str()
        .filter(|pin| !pin.is_empty())
        .ok_or("signing policy pin missing")?;
    if receipt["source_sha"] != sha
        || receipt["signature_valid"] != true
        || receipt["certificate_sha256"] != pin
        || receipt["file_sha256"].as_str() != Some(validate::digest(executable).as_str())
    {
        return Err("signing receipt mismatch".into());
    }
    let source = archives::read(&assets[&expected[1]])?;
    let plugin = archives::read(&assets[&expected[2]])?;
    for (name, bytes) in plugin {
        if source.get(&name) != Some(&bytes) || native.get(&name) != Some(&bytes) {
            return Err("plugin/source/native common payload mismatch".into());
        }
    }
    for name in [
        format!("release-{TARGET}.json"),
        format!("SHA256SUMS-{TARGET}.txt"),
        "windows-signing-receipt.json".into(),
    ] {
        assets.insert(name.clone(), directory.join(name));
    }
    Ok(assets)
}

#[cfg(test)]
#[path = "release_tests.rs"]
mod tests;
