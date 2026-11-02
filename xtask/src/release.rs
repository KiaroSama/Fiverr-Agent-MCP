use crate::{Result, ci::owned, root, validate};
use serde_json::Value;
use std::{collections::BTreeMap, env, fs, path::Path};
fn api(path: &str) -> Result<Value> {
    let result = owned(60, 30, "gh", &["api", path])?;
    Ok(serde_json::from_slice(&result.stdout)?)
}
pub fn verify(run: &str, sha: &str) -> Result<()> {
    crate::sha(sha)?;
    if run.is_empty() || !run.bytes().all(|v| v.is_ascii_digit()) {
        return Err("invalid CI run ID".into());
    }
    let repo = env::var("GH_REPO")?;
    if repo != "KiaroSama/Fiverr-Agent-MCP" {
        return Err("unexpected release repository".into());
    }
    let metadata = api(&format!("repos/{repo}/actions/runs/{run}"))?;
    if metadata["status"] != "completed"
        || metadata["conclusion"] != "success"
        || metadata["head_sha"] != sha
        || metadata["head_branch"] != "feature/fiverr-native"
        || metadata["path"] != ".github/workflows/ci.yml"
        || metadata["event"] != "workflow_dispatch"
    {
        return Err("unverified exact-source manual CI".into());
    }
    let branch = api(&format!("repos/{repo}/git/ref/heads/feature/fiverr-native"))?;
    if branch["object"]["sha"] != sha {
        return Err("release source is not current default".into());
    }
    let jobs = api(&format!(
        "repos/{repo}/actions/runs/{run}/jobs?per_page=100"
    ))?;
    let list = jobs["jobs"].as_array().ok_or("missing CI jobs")?;
    if jobs["total_count"].as_u64() != Some(list.len() as u64) {
        return Err("incomplete CI job inventory".into());
    }
    for name in crate::TARGETS
        .map(|t| format!("native-{t}"))
        .into_iter()
        .chain(["security / dependencies".into()])
    {
        if !list
            .iter()
            .any(|j| j["name"] == name && j["conclusion"] == "success")
        {
            return Err(format!("required successful job missing: {name}").into());
        }
    }
    let mut output = fs::OpenOptions::new()
        .append(true)
        .open(env::var_os("GITHUB_OUTPUT").ok_or("GITHUB_OUTPUT missing")?)?;
    use std::io::Write;
    writeln!(output, "sha={sha}\nrun_id={run}")?;
    Ok(())
}
pub fn publish(sha: &str) -> Result<()> {
    crate::sha(sha)?;
    let directory = root().join("verified-artifacts/fiverr-x86_64-pc-windows-msvc");
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
    let policy: Value =
        serde_json::from_slice(&validate::regular(&root(), "config/signing-public.json")?)?;
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
    let tag = format!("v{}", crate::VERSION);
    let tags = api(&format!("repos/{}/tags?per_page=100", env::var("GH_REPO")?))?;
    if tags
        .as_array()
        .ok_or("tag inventory missing")?
        .iter()
        .any(|v| v["name"] == tag)
    {
        return Err("existing release/tag will not be overwritten".into());
    }
    let paths = assets
        .values()
        .map(|v| v.to_str().ok_or("non-UTF8 asset path"))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut args = vec![
        "release",
        "create",
        &tag,
        "--target",
        sha,
        "--draft",
        "--prerelease",
        "--title",
        "Fiverr Agent MCP 0.1.0 (experimental)",
        "--notes",
        "Experimental owner-signed Windows package. Other targets are native source validation only; self-signed identity is not public trust.",
    ];
    args.extend(paths);
    owned(180, 60, "gh", &args)?;
    owned(60, 30, "gh", &["release", "edit", &tag, "--draft=false"])?;
    Ok(())
}
