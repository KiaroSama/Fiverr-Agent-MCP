use crate::{Result, ci::owned, root, validate};
use serde_json::Value;
use std::{env, fs};

#[path = "release_assets.rs"]
mod assets;
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
    let policy: Value =
        serde_json::from_slice(&validate::regular(&root(), "config/signing-public.json")?)?;
    let assets = assets::validate_assets(&directory, sha, &policy)?;
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
