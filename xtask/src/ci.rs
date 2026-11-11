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
    let run_tests = |wall, idle, args: &[&str]| -> Result<()> {
        let args = args
            .iter()
            .map(|value| (*value).to_owned())
            .chain([
                "--message-format".into(),
                "libtest-json-plus".into(),
                "--message-format-version".into(),
                "0.1".into(),
            ])
            .collect::<Vec<_>>();
        let output = process::test_run(&args, &root(), wall, idle)?;
        fs::write(root().join(".ci-work/test-events.jsonl"), &output.stdout)?;
        let receipt = crate::test_inventory::reconcile(&root(), target, &output.stdout)?;
        fs::write(
            root().join(".ci-work/test-inventory.json"),
            serde_json::to_vec_pretty(&receipt)?,
        )?;
        if !output.status.success() || receipt["failed"] != 0 {
            return Err("native test execution failed; full inventory retained".into());
        }
        Ok(())
    };
    if cfg!(target_os = "linux") {
        owned(120, 60, "fc-cache", &["--verbose"])?;
        run_tests(
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
        crate::sandbox::run()?;
        crate::sandbox::fuzz()?;
    } else {
        run_tests(
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
    cargo(
        600,
        180,
        &[
            "build",
            "--locked",
            "--release",
            "--package",
            "xtask",
            "--bin",
            "fiverr-verify",
            "--jobs",
            "2",
        ],
    )?;
    let binary = target_dir()?.join("release/fiverr-agent-mcp.exe");
    fs::create_dir_all(root().join(".ci-work/privacy acceptance"))?;
    #[cfg(windows)]
    if !process::privacy_fixture(&binary, &root().join(".ci-work/privacy acceptance"))?
        .status
        .success()
    {
        return Err("current-token privacy fixture startup failed".into());
    }
    #[cfg(not(windows))]
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
        "(validate_id|validate_relative|validate_kind|bounded_text|Money::validate|Provenance::validate).*with Ok",
    ];
    let mut args = vec!["mutants", "--list", "--json"];
    args.extend(scope);
    let inventory = owned(60, 30, executable, &args)?;
    fs::write(
        root().join(".ci-work/mutation-inventory.json"),
        &inventory.stdout,
    )?;
    let items: Vec<serde_json::Value> = serde_json::from_slice(&inventory.stdout)?;
    let expected = std::collections::BTreeSet::from([
        "validate_id",
        "validate_relative",
        "validate_kind",
        "bounded_text",
        "Money::validate",
        "Provenance::validate",
    ]);
    let discovered = items
        .iter()
        .filter_map(|v| v["function"]["function_name"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if items.len() != expected.len()
        || discovered != expected
        || items
            .iter()
            .any(|v| v["genre"] != "FnValue" || v["replacement"] != "Ok(())")
    {
        return Err(
            "mutation scope must discover exactly six distinct validator-return mutants".into(),
        );
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
        "--cargo-arg=--test=primitives",
        "--cargo-arg=--test=properties",
    ]);
    owned(1500, 240, executable, &args)?;
    let result: serde_json::Value = serde_json::from_slice(&fs::read(
        root().join(".ci-work/mutants/mutants.out/outcomes.json"),
    )?)?;
    if result["total_mutants"] != 6
        || result["caught"] != 6
        || ["missed", "timeout", "unviable"]
            .iter()
            .any(|k| result[k] != 0)
    {
        return Err("mutation evidence incomplete or inconclusive".into());
    }
    owned(15, 10, "git", &["diff", "--exit-code", "--", "src"])?;
    Ok(())
}

pub fn windows_diagnostics() -> Result<()> {
    if !cfg!(windows) {
        return Err("Windows diagnostic samples require a native Windows runner".into());
    }
    let mut outcomes = Vec::new();
    let initial = target_dir()?.join("nextest/ci/junit.xml");
    if !initial.is_file() {
        return Err("Normal native sample JUnit missing; diagnostic invocation not started".into());
    }
    fs::copy(initial, root().join(".ci-work/windows-diagnostics-1.xml"))?;
    // The normal matrix is sample 1. These are finite samples, never retries.
    for sample in 2..=3 {
        let result = cargo(
            600,
            180,
            &[
                "nextest",
                "run",
                "--locked",
                "--package",
                "fiverr-agent-mcp",
                "--profile",
                "ci",
                "--test-threads",
                "2",
                "--no-tests",
                "fail",
                "--no-fail-fast",
                "--success-output",
                "immediate",
                "-E",
                "test(native_browser_reads_fixed_fixture_without_exposing_credentials) | test(powershell_launcher_restores_modes_errors_and_rooted_utf8_logs)",
            ],
        );
        let junit = target_dir()?.join("nextest/ci/junit.xml");
        if junit.is_file() {
            fs::copy(
                junit,
                root().join(format!(".ci-work/windows-diagnostics-{sample}.xml")),
            )?;
        }
        outcomes.push(serde_json::json!({"sample":sample,"success":result.is_ok(),"error":result.err().map(|e| e.to_string())}));
    }
    fs::write(
        root().join(".ci-work/windows-diagnostics.json"),
        serde_json::to_vec_pretty(&outcomes)?,
    )?;
    if outcomes.iter().any(|v| v["success"] != true) {
        return Err(
            "Finite diagnostic sample failed; all outcomes retained, no retry performed".into(),
        );
    }
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
