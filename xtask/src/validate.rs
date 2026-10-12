use crate::{PRODUCT, Result, VERSION};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path},
};

pub fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn path(value: &str) -> Result<()> {
    if value.is_empty()
        || value.contains(['\\', ':', '\0'])
        || value.starts_with('/')
        || value.ends_with('/')
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("invalid archive relative path".into());
    }
    if Path::new(value)
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("archive path traversal refused".into());
    }
    let forbidden = [
        ".ai",
        ".specify",
        "specs",
        ".git",
        ".agents",
        ".claude",
        ".codex",
        ".kiro",
        ".cursor",
        ".cline",
        ".state",
        ".cache",
        "logs",
        "target",
        "node_modules",
        "plans",
        "graphify-out",
        ".codebase-memory",
        ".ignoreme",
    ];
    if value.split('/').any(|part| {
        forbidden.contains(&part)
            || part == "secrets.md"
            || part == "explain-AI.md"
            || part == "CLAUDE.md"
            || part == "AGENTS.md"
            || part.starts_with(".env")
    }) {
        return Err("protected content excluded from distribution".into());
    }
    Ok(())
}

pub fn regular(root: &Path, relative: &str) -> Result<Vec<u8>> {
    path(relative)?;
    let canonical = root.canonicalize()?;
    let mut current = root.to_path_buf();
    for segment in relative.split('/') {
        current.push(segment);
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.file_type().is_symlink() {
            return Err("linked distribution input refused".into());
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err("reparse distribution input refused".into());
            }
        }
    }
    if !current.canonicalize()?.starts_with(canonical) || !current.is_file() {
        return Err("distribution input outside root or not a file".into());
    }
    let metadata = current.metadata()?;
    if metadata.len() > 128 * 1024 * 1024 {
        return Err("distribution input too large".into());
    }
    let bytes = fs::read(current)?;
    if ![".exe", ".png", ".jpg", ".ico", ".woff2"]
        .iter()
        .any(|extension| relative.ends_with(extension))
        && !relative.starts_with("bin/")
    {
        let text = std::str::from_utf8(&bytes)?;
        for line in text.lines() {
            if [
                "-----BEGIN PRIVATE KEY-----",
                "-----BEGIN RSA PRIVATE KEY-----",
            ]
            .iter()
            .any(|marker| line.starts_with(marker))
            {
                return Err("private key distribution content refused".into());
            }
            for prefix in ["ghp_", "github_pat_", "sk_live_"] {
                if line.split(prefix).skip(1).any(|suffix| {
                    suffix
                        .bytes()
                        .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                        .count()
                        >= 20
                }) {
                    return Err("credential-shaped distribution content refused".into());
                }
            }
        }
    }
    Ok(bytes)
}

fn json(root: &Path, relative: &str) -> Result<Value> {
    Ok(serde_json::from_slice(&regular(root, relative)?)?)
}

pub fn manifests(root: &Path) -> Result<()> {
    let marker = json(root, "fiverr.install.json")?;
    if marker["product"] != PRODUCT || marker["version"] != VERSION || marker["layout_version"] != 1
    {
        return Err("installation marker contract mismatch".into());
    }
    for name in [
        "plugin.json",
        ".claude-plugin/plugin.json",
        ".claude-plugin/marketplace.json",
        ".codex-plugin/plugin.json",
        ".cursor-plugin/plugin.json",
    ] {
        let value = json(root, name)?;
        if !value.is_object() {
            return Err(format!("manifest must be an object: {name}").into());
        }
        if name.ends_with("marketplace.json") {
            let plugins = value["plugins"]
                .as_array()
                .ok_or("marketplace plugin list missing")?;
            if plugins.len() != 1
                || plugins[0]["name"] != PRODUCT
                || plugins[0]["source"] != "./"
                || plugins[0]["version"] != VERSION
            {
                return Err("marketplace source/version mismatch".into());
            }
        } else if value["name"] != PRODUCT || value["version"] != VERSION {
            return Err(format!("plugin identity mismatch: {name}").into());
        }
        if value.get("hooks").is_some() || value.get("mcpServers").is_some() {
            return Err("instruction plugin must not auto-start runtime/hooks".into());
        }
    }
    for directory in ["config", "examples/clients"] {
        let entries = fs::read_dir(root.join(directory))?;
        let mut count = 0;
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".json") && (directory != "config" || name.ends_with(".example.json"))
            {
                json(root, &format!("{directory}/{name}"))?;
                count += 1;
            }
        }
        if count == 0 {
            return Err(format!("missing public JSON templates: {directory}").into());
        }
    }
    for name in [
        "LICENSE",
        "THIRD_PARTY_NOTICES.md",
        "skills/fiverr-gig-optimizer/SKILL.md",
    ] {
        if regular(root, name)?.is_empty() {
            return Err(format!("empty required payload: {name}").into());
        }
    }
    println!("Validated installation and client/plugin JSON manifests");
    Ok(())
}

pub fn capabilities(root: &Path) -> Result<()> {
    let value = json(root, "capabilities.json")?;
    if value["schema"] != 1 && value["schema_version"] != 1 {
        return Err("capability schema must be 1".into());
    }
    let workflows = value["workflows"]
        .as_array()
        .ok_or("missing workflow inventory")?;
    if workflows.is_empty() {
        return Err("empty workflow inventory".into());
    }
    let source = String::from_utf8(regular(root, "src/server/catalog.rs")?)?;
    let actual_tools: BTreeSet<_> = source
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let remainder = line
                .strip_prefix("(\"fiverr_")
                .or_else(|| line.strip_prefix("\"fiverr_"))?;
            let end = remainder.find('"')?;
            Some(format!("fiverr_{}", &remainder[..end]))
        })
        .collect();
    if actual_tools.is_empty() {
        return Err("implemented tool inventory missing".into());
    }
    let mut names = BTreeSet::new();
    let mut requirements = BTreeSet::new();
    for workflow in workflows {
        let name = workflow
            .get("id")
            .or_else(|| workflow.get("name"))
            .and_then(Value::as_str)
            .ok_or("workflow needs id or name")?;
        if !names.insert(name) {
            return Err("duplicate workflow identity".into());
        }
        for dimension in [
            "requirement_id",
            "implementation",
            "availability",
            "verification",
            "required_approval",
            "reason",
        ] {
            if !workflow
                .get(dimension)
                .is_some_and(|value| value.as_str().is_some_and(|text| !text.is_empty()))
            {
                return Err(format!("workflow {name} lacks {dimension}").into());
            }
        }
        if !requirements.insert(
            workflow["requirement_id"]
                .as_str()
                .ok_or("requirement identity")?,
        ) {
            return Err("duplicate requirement identity".into());
        }
        let tool = workflow["tool"]
            .as_str()
            .ok_or("workflow tool must be a string")?;
        if !actual_tools.contains(tool) {
            return Err(format!("workflow {name} maps to undiscovered tool {tool}").into());
        }
        let tests = workflow["tests"]
            .as_array()
            .ok_or("workflow tests must be an array")?;
        if workflow["implementation"] == "implemented" && tests.is_empty() {
            return Err(format!("implemented workflow {name} has no test mapping").into());
        }
        for test in tests {
            regular(root, test.as_str().ok_or("test mapping must be a string")?)?;
        }
    }
    println!("Validated {} workflow capability entries", workflows.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_trust_boundary() {
        for rejected in [
            "../secrets.md",
            "/etc/passwd",
            "a/../b",
            "a\\b",
            "a:b",
            ".ai/memory.md",
            "config/.env",
            ".state/session.json",
            "secrets.md",
        ] {
            assert!(path(rejected).is_err(), "accepted {rejected}");
        }
        for accepted in [
            ".claude-plugin/plugin.json",
            "LICENSE",
            "bin/fiverr-agent-mcp.exe",
            "skills/fiverr-gig-optimizer/SKILL.md",
        ] {
            path(accepted).unwrap();
        }
        assert_eq!(
            digest(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
