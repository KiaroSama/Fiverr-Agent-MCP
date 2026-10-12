use crate::{
    PRODUCT, Result, VERSION,
    archives::{self, Files},
    process, validate,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

const REQUIRED: [&str; 12] = [
    "fiverr.install.json",
    "run.ps1",
    "run.sh",
    "plugin.json",
    ".claude-plugin/plugin.json",
    ".claude-plugin/marketplace.json",
    ".codex-plugin/plugin.json",
    ".cursor-plugin/plugin.json",
    "LICENSE",
    "THIRD_PARTY_NOTICES.md",
    "capabilities.json",
    "README.md",
];

fn public_payload(root: &Path) -> Result<Files> {
    let mut files = Files::new();
    for name in REQUIRED {
        files.insert(name.to_owned(), validate::regular(root, name)?);
    }
    for directory in [
        "docs",
        "third_party",
        "skills/fiverr-gig-optimizer",
        "examples/clients",
    ] {
        tree(root, directory, &mut files)?;
    }
    for entry in fs::read_dir(root.join("config"))? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if name.ends_with(".example.json") {
            let path = format!("config/{name}");
            files.insert(path.clone(), validate::regular(root, &path)?);
        }
    }
    for name in [
        "skills.lock.json",
        "CONTRIBUTING.md",
        "SECURITY.md",
        "CHANGELOG.md",
    ] {
        files.insert(name.into(), validate::regular(root, name)?);
    }
    Ok(files)
}

fn tree(root: &Path, directory: &str, files: &mut Files) -> Result<()> {
    validate::path(directory)?;
    let metadata = fs::symlink_metadata(root.join(directory))?;
    if metadata.file_type().is_symlink() {
        return Err("linked payload tree refused".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("reparse payload tree refused".into());
        }
    }
    for entry in fs::read_dir(root.join(directory))? {
        let entry = entry?;
        let path = format!(
            "{directory}/{}",
            entry.file_name().to_str().ok_or("non-UTF8 payload name")?
        );
        if entry.file_type()?.is_dir() {
            tree(root, &path, files)?;
        } else {
            files.insert(path.clone(), validate::regular(root, &path)?);
        }
    }
    Ok(())
}

fn licenses(root: &Path, target: &str) -> Result<Files> {
    let args = [
        "metadata",
        "--format-version",
        "1",
        "--locked",
        "--offline",
        "--filter-platform",
        target,
    ]
    .map(str::to_owned);
    let output = process::run("cargo", &args, root, 120, 60)?;
    if !output.status.success() {
        return Err("locked dependency metadata failed".into());
    }
    let metadata: Value = serde_json::from_slice(&output.stdout)?;
    let packages = metadata["packages"]
        .as_array()
        .ok_or("metadata packages missing")?;
    let nodes = metadata["resolve"]["nodes"]
        .as_array()
        .ok_or("metadata resolve missing")?;
    let application = packages
        .iter()
        .find(|package| package["name"] == PRODUCT)
        .ok_or("application metadata missing")?;
    let mut pending = vec![
        application["id"]
            .as_str()
            .ok_or("package id missing")?
            .to_owned(),
    ];
    let mut selected = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !selected.insert(id.clone()) {
            continue;
        }
        let node = nodes
            .iter()
            .find(|node| node["id"] == id)
            .ok_or("dependency node missing")?;
        for dep in node["deps"].as_array().ok_or("dependency edges missing")? {
            if dep["dep_kinds"]
                .as_array()
                .ok_or("dependency kinds missing")?
                .iter()
                .any(|kind| kind["kind"] != "dev")
            {
                pending.push(
                    dep["pkg"]
                        .as_str()
                        .ok_or("dependency id missing")?
                        .to_owned(),
                );
            }
        }
    }
    let mut files = Files::new();
    let mut inventory = Vec::new();
    for package in packages.iter().filter(|package| {
        package["id"]
            .as_str()
            .is_some_and(|id| selected.contains(id))
    }) {
        let name = package["name"].as_str().ok_or("dependency name missing")?;
        if name == PRODUCT {
            continue;
        }
        let version = package["version"]
            .as_str()
            .ok_or("dependency version missing")?;
        let manifest = Path::new(
            package["manifest_path"]
                .as_str()
                .ok_or("dependency manifest missing")?,
        );
        let directory = manifest.parent().ok_or("dependency parent missing")?;
        let mut found = 0;
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let basename = entry.file_name().to_string_lossy().into_owned();
            let upper = basename.to_ascii_uppercase();
            if entry.file_type()?.is_file()
                && ["LICENSE", "LICENCE", "COPYING", "NOTICE"]
                    .iter()
                    .any(|prefix| upper.starts_with(prefix))
            {
                if entry.metadata()?.len() > 1024 * 1024 {
                    return Err("dependency license exceeds limit".into());
                }
                let bytes = fs::read(entry.path())?;
                std::str::from_utf8(&bytes)?;
                let path = format!("licenses/dependencies/{name}-{version}/{basename}");
                validate::path(&path)?;
                files.insert(path, bytes);
                found += 1;
            }
        }
        if let Some(relative) = package["license_file"].as_str() {
            let path = directory.join(relative);
            if !path.canonicalize()?.starts_with(directory.canonicalize()?)
                || fs::symlink_metadata(&path)?.file_type().is_symlink()
            {
                return Err("dependency license path escaped".into());
            }
            let metadata = fs::metadata(&path)?;
            if !metadata.is_file() || metadata.len() > 1024 * 1024 {
                return Err("declared dependency license must be a bounded regular file".into());
            }
            let bytes = fs::read(path)?;
            std::str::from_utf8(&bytes)?;
            files.insert(
                format!("licenses/dependencies/{name}-{version}/DECLARED-LICENSE"),
                bytes,
            );
            found += 1;
        }
        if found == 0 {
            return Err(format!("no distributable license text for {name}-{version}").into());
        }
        inventory.push(json!({"name":name,"version":version,"license":package["license"]}));
    }
    files.insert(
        "licenses/dependencies.json".into(),
        serde_json::to_vec_pretty(&inventory)?,
    );
    Ok(files)
}

pub fn build(root: &Path, binary: &str, out: &str, target: &str, sha: &str) -> Result<()> {
    validate::manifests(root)?;
    validate::capabilities(root)?;
    let out = PathBuf::from(out);
    if !out.is_absolute()
        || out
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err("package output must be an absolute normal workspace path".into());
    }
    let mut ancestor = out.as_path();
    while !ancestor.exists() {
        ancestor = ancestor.parent().ok_or("output parent missing")?;
    }
    if !ancestor.canonicalize()?.starts_with(root.canonicalize()?) {
        return Err("package output outside workspace".into());
    }
    fs::create_dir_all(&out)?;
    if !out.canonicalize()?.starts_with(root.canonicalize()?) {
        return Err("package output must be workspace-contained".into());
    }
    let binary = PathBuf::from(binary).canonicalize()?;
    if !binary.starts_with(root.canonicalize()?) || !binary.is_file() {
        return Err("binary must be workspace-contained".into());
    }
    let mut files = public_payload(root)?;
    files.extend(licenses(root, target)?);
    let binary_name = if target.contains("windows") {
        "bin/fiverr-agent-mcp.exe"
    } else {
        "bin/fiverr-agent-mcp"
    };
    files.insert(binary_name.into(), fs::read(binary)?);
    let entries: BTreeMap<_, _> = files
        .iter()
        .map(|(path, bytes)| (path.clone(), validate::digest(bytes)))
        .collect();
    files.insert("release.json".into(), serde_json::to_vec_pretty(&json!({"product":PRODUCT,"version":VERSION,"source_sha":sha,"target":target,"files":entries,"built_at_utc":chrono::Utc::now().to_rfc3339()}))?);
    let extension = if target.contains("windows") {
        "zip"
    } else {
        "tar.gz"
    };
    let archive = out.join(format!("{PRODUCT}-{VERSION}-{target}.{extension}"));
    archives::write(&archive, &files)?;
    verify(
        root,
        archive.to_str().ok_or("non-UTF8 archive path")?,
        target,
        sha,
    )?;
    let plugin: Files = files
        .iter()
        .filter(|(name, _)| {
            name.starts_with("skills/")
                || name.ends_with("plugin.json")
                || name.ends_with("marketplace.json")
                || name.starts_with("third_party/")
                || name.starts_with("docs/")
                || [
                    "LICENSE",
                    "THIRD_PARTY_NOTICES.md",
                    "README.md",
                    "skills.lock.json",
                ]
                .contains(&name.as_str())
        })
        .map(|(name, bytes)| (name.clone(), bytes.clone()))
        .collect();
    let plugin_archive = out.join(format!("fiverr-gig-optimizer-{VERSION}-plugin.zip"));
    archives::write(&plugin_archive, &plugin)?;
    if archives::read(&plugin_archive)? != plugin {
        return Err("plugin archive roundtrip mismatch".into());
    }
    let source_archive = out.join(format!("{PRODUCT}-{VERSION}-source.tar.gz"));
    let source_files = source(root)?;
    archives::write(&source_archive, &source_files)?;
    if archives::read(&source_archive)? != source_files {
        return Err("source archive roundtrip mismatch".into());
    }
    let mut sums = String::new();
    for archive in [&archive, &plugin_archive, &source_archive] {
        sums.push_str(&format!(
            "{}  {}\n",
            validate::digest(&fs::read(archive)?),
            archive
                .file_name()
                .ok_or("archive basename")?
                .to_str()
                .ok_or("archive name")?
        ));
    }
    fs::write(
        out.join(format!("SHA256SUMS-{target}.txt")),
        sums.as_bytes(),
    )?;
    fs::write(
        out.join(format!("release-{target}.json")),
        files
            .get("release.json")
            .ok_or("release metadata missing")?,
    )?;
    println!("Packaged verified native, source and plugin archives for {target}");
    Ok(())
}

fn source(root: &Path) -> Result<Files> {
    let output = process::run("git", &["ls-files".into(), "-z".into()], root, 15, 10)?;
    if !output.status.success() {
        return Err("source index inventory failed".into());
    }
    let mut files = Files::new();
    for name in std::str::from_utf8(&output.stdout)?
        .split('\0')
        .filter(|name| !name.is_empty())
    {
        if [
            "src/", "tests/", "xtask/", "scripts/", ".github/", ".config/",
        ]
        .iter()
        .any(|prefix| name.starts_with(prefix))
            || [
                "Cargo.toml",
                "Cargo.lock",
                "rust-toolchain.toml",
                "deny.toml",
                ".gitignore",
                ".gitattributes",
                "references.lock.json",
            ]
            .contains(&name)
        {
            files.insert(name.into(), validate::regular(root, name)?);
        }
    }
    files.extend(public_payload(root)?);
    for name in [
        "skills.lock.json",
        "CONTRIBUTING.md",
        "SECURITY.md",
        "CHANGELOG.md",
    ] {
        files.insert(name.into(), validate::regular(root, name)?);
    }
    if !files.contains_key("Cargo.lock") || !files.keys().any(|name| name.starts_with("src/")) {
        return Err("incomplete committed source payload".into());
    }
    Ok(files)
}

pub fn verify(root: &Path, archive: &str, target: &str, sha: &str) -> Result<()> {
    let files = archives::read(Path::new(archive))?;
    let release: Value = serde_json::from_slice(
        files
            .get("release.json")
            .ok_or("release metadata missing")?,
    )?;
    if release["product"] != PRODUCT
        || release["version"] != VERSION
        || release["target"] != target
        || release["source_sha"] != sha
    {
        return Err("release identity mismatch".into());
    }
    let hashes = release["files"]
        .as_object()
        .ok_or("release file hashes missing")?;
    if hashes.len() + 1 != files.len() {
        return Err("unlisted distribution entry".into());
    }
    for (name, hash) in hashes {
        if hash.as_str()
            != files
                .get(name)
                .map(|bytes| validate::digest(bytes))
                .as_deref()
        {
            return Err(format!("payload hash mismatch: {name}").into());
        }
    }
    for name in REQUIRED {
        if files.get(name) != Some(&validate::regular(root, name)?) {
            return Err(format!("required source payload differs: {name}").into());
        }
    }
    if !files.contains_key("licenses/dependencies.json") {
        return Err("dependency license inventory absent".into());
    }
    let public = public_payload(root)?;
    let mut allowed: BTreeSet<_> = public.keys().cloned().collect();
    allowed.extend([
        "release.json".into(),
        "licenses/dependencies.json".into(),
        if target.contains("windows") {
            "bin/fiverr-agent-mcp.exe".into()
        } else {
            "bin/fiverr-agent-mcp".into()
        },
    ]);
    for name in files.keys() {
        if !allowed.contains(name) && !name.starts_with("licenses/dependencies/") {
            return Err(format!("entry outside package allowlist: {name}").into());
        }
    }
    for (name, bytes) in public {
        if files.get(&name) != Some(&bytes) {
            return Err(format!("public source payload differs: {name}").into());
        }
    }
    let scratch = root.join(".ci-work").join(format!(
        "package smoke {} {}",
        std::process::id(),
        chrono::Utc::now().timestamp_micros()
    ));
    fs::create_dir_all(scratch.parent().ok_or("smoke parent")?)?;
    archives::extract(&scratch, &files)?;
    let executable = scratch.join(if target.contains("windows") {
        "bin/fiverr-agent-mcp.exe"
    } else {
        "bin/fiverr-agent-mcp"
    });
    let result = (|| -> Result<()> {
        for args in [
            vec!["--version".into()],
            vec!["status".into()],
            vec!["desktop-config".into()],
        ] {
            let output = process::smoke(executable.to_str().ok_or("binary path")?, &args, root)?;
            if !output.status.success() {
                return Err("extracted native smoke failed".into());
            }
            if args[0] == "--version" {
                if !std::str::from_utf8(&output.stdout)?.contains(VERSION) {
                    return Err("native version mismatch".into());
                }
            } else {
                let value: Value = serde_json::from_slice(&output.stdout)?;
                if !value.is_object() {
                    return Err("native smoke must return JSON object".into());
                }
            }
        }
        Ok(())
    })();
    if result.is_ok() {
        fs::remove_dir_all(scratch)?;
    }
    result
}
