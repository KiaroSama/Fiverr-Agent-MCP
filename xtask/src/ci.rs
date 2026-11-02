use crate::{Result, process, root};
use std::{env, fs, path::PathBuf};

pub fn owned(wall: u64, idle: u64, executable: &str, args: &[&str]) -> Result<process::Output> {
    let args = args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let output = process::run(executable, &args, &root(), wall, idle)?;
    if !output.status.success() {
        return Err(format!("bounded {executable} failed: {}", output.status).into());
    }
    Ok(output)
}
fn cargo(wall: u64, idle: u64, args: &[&str]) -> Result<()> {
    owned(wall, idle, "cargo", args).map(drop)
}
fn target_dir() -> Result<PathBuf> {
    Ok(PathBuf::from(
        env::var_os("CARGO_TARGET_DIR").ok_or("CARGO_TARGET_DIR missing")?,
    ))
}

pub fn native(target: &str, sha: &str, validation_only: bool) -> Result<()> {
    crate::target(target)?;
    crate::sha(sha)?;
    let host = owned(30, 15, "rustc", &["-vV"])?;
    if !std::str::from_utf8(&host.stdout)?
        .lines()
        .any(|line| line == format!("host: {target}"))
    {
        return Err("actual native Rust target mismatch".into());
    }
    cargo(60, 30, &["fmt", "--all", "--", "--check"])?;
    cargo(
        1200,
        240,
        &[
            "clippy",
            "--locked",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--jobs",
            "2",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    crate::validate::manifests(&root())?;
    crate::validate::capabilities(&root())?;
    let candidates: &[&str] = if cfg!(windows) {
        &[
            "C:/Program Files/Google/Chrome/Application/chrome.exe",
            "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
        ]
    } else if cfg!(target_os = "macos") {
        &["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"]
    } else {
        &[
            "/usr/bin/google-chrome",
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
        ]
    };
    let browser = candidates
        .iter()
        .find(|p| std::path::Path::new(p).is_file())
        .ok_or("native browser prerequisite missing; required fixture cannot be skipped")?;
    // Pass child-only configuration without mutating a threaded parent's environment.
    let config = root().join(".ci-work/browser-path");
    fs::write(&config, browser.as_bytes())?;
    if cfg!(target_os = "linux") {
        owned(120, 60, "fc-cache", &["--verbose"])?;
        cargo(
            1500,
            300,
            &[
                "llvm-cov",
                "nextest",
                "--locked",
                "--workspace",
                "--all-features",
                "--json",
                "--output-path",
                ".ci-work/coverage.json",
                "--profile",
                "ci",
                "--test-threads",
                "2",
                "--no-tests",
                "fail",
                "--no-fail-fast",
            ],
        )?;
        crate::coverage::report(
            root()
                .join(".ci-work/coverage.json")
                .to_str()
                .ok_or("coverage path")?,
            root()
                .join(".ci-work/coverage-summary.json")
                .to_str()
                .ok_or("coverage path")?,
        )?;
        cargo(
            900,
            240,
            &[
                "install",
                "cargo-mutants",
                "--version",
                "27.1.0",
                "--locked",
                "--root",
                ".ci-cache/mutation-tool",
                "--jobs",
                "2",
            ],
        )?;
        mutation()?;
    } else {
        cargo(
            1200,
            240,
            &[
                "nextest",
                "run",
                "--locked",
                "--workspace",
                "--all-features",
                "--profile",
                "ci",
                "--test-threads",
                "2",
                "--no-tests",
                "fail",
                "--no-fail-fast",
            ],
        )?;
    }
    cargo(
        300,
        90,
        &[
            "test",
            "--locked",
            "--workspace",
            "--doc",
            "--all-features",
            "--jobs",
            "2",
        ],
    )?;
    if validation_only || !cfg!(windows) {
        println!(
            "Native source validation completed; no signed distribution requested for this target."
        );
        return Ok(());
    }
    cargo(
        1800,
        300,
        &[
            "build",
            "--locked",
            "--release",
            "--package",
            "fiverr-agent-mcp",
            "--jobs",
            "2",
        ],
    )?;
    let binary = target_dir()?.join("release/fiverr-agent-mcp.exe");
    fs::create_dir_all(root().join(".ci-work/privacy acceptance"))?;
    owned(
        30,
        15,
        binary.to_str().ok_or("binary path")?,
        &[
            "--root",
            root()
                .join(".ci-work/privacy acceptance")
                .to_str()
                .ok_or("privacy path")?,
            "status",
        ],
    )?;
    #[cfg(windows)]
    crate::windows_privacy::verify(&root().join(".ci-work/privacy acceptance"))?;
    println!("Windows release compiled; protected signing follows.");
    Ok(())
}
fn mutation() -> Result<()> {
    let executable = root().join(".ci-cache/mutation-tool/bin/cargo-mutants");
    let executable = executable.to_str().ok_or("mutants path")?;
    let scope = [
        "--package",
        "fiverr-agent-mcp",
        "--file",
        "src/core/mod.rs",
        "--file",
        "src/storage/root.rs",
        "--re",
        "(validate_id|validate_relative).*with Ok",
    ];
    let mut args = vec!["mutants", "--list", "--json"];
    args.extend(scope);
    let inventory = owned(60, 30, executable, &args)?;
    fs::write(
        root().join(".ci-work/mutation-inventory.json"),
        &inventory.stdout,
    )?;
    let items: Vec<serde_json::Value> = serde_json::from_slice(&inventory.stdout)?;
    if items.len() != 2
        || items
            .iter()
            .any(|v| v["genre"] != "FnValue" || v["replacement"] != "Ok(())")
        || items[0]["file"] == items[1]["file"]
    {
        return Err("mutation scope must discover exactly two validator-return mutants".into());
    }
    let mut args = vec!["mutants", "--in-place", "--baseline", "skip"];
    args.extend(scope);
    args.extend([
        "--timeout",
        "30",
        "--build-timeout",
        "600",
        "--test-tool",
        "nextest",
        "--output",
        ".ci-work/mutants",
        "--cargo-arg=--locked",
        "--cargo-test-arg=--profile=ci",
        "--cargo-test-arg=--test-threads=2",
        "--cargo-test-arg=--test=primitives",
        "--cargo-test-arg=--test=properties",
    ]);
    owned(1500, 240, executable, &args)?;
    let result: serde_json::Value = serde_json::from_slice(&fs::read(
        root().join(".ci-work/mutants/mutants.out/outcomes.json"),
    )?)?;
    if result["total_mutants"] != 2
        || result["caught"] != 2
        || ["missed", "timeout", "unviable"]
            .iter()
            .any(|k| result[k] != 0)
    {
        return Err("mutation evidence incomplete or inconclusive".into());
    }
    owned(15, 10, "git", &["diff", "--exit-code", "--", "src"])?;
    Ok(())
}

pub fn dependencies() -> Result<()> {
    for (tool, version) in [("cargo-audit", "0.22.2"), ("cargo-deny", "0.20.2")] {
        let output = owned(30, 15, tool, &["--version"])?;
        if !std::str::from_utf8(&output.stdout)?.contains(version) {
            return Err("dependency tool version mismatch".into());
        }
    }
    owned(300, 120, "cargo-audit", &["audit", "--deny", "warnings"])?;
    owned(
        300,
        120,
        "cargo-deny",
        &[
            "--locked",
            "check",
            "advisories",
            "bans",
            "licenses",
            "sources",
        ],
    )?;
    Ok(())
}
