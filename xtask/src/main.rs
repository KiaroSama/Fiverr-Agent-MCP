mod archives;
mod package;
mod process;
mod validate;

use std::{collections::BTreeMap, env, path::PathBuf};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const PRODUCT: &str = "fiverr-agent-mcp";
const VERSION: &str = "0.1.0";
const TARGETS: [&str; 4] = [
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask workspace parent")
        .to_path_buf()
}

fn options(args: &[String]) -> Result<BTreeMap<String, String>> {
    if !args.len().is_multiple_of(2) {
        return Err("options require --name value pairs".into());
    }
    let mut values = BTreeMap::new();
    for pair in args.as_chunks::<2>().0 {
        if !pair[0].starts_with("--") || values.insert(pair[0].clone(), pair[1].clone()).is_some() {
            return Err("invalid or duplicate option".into());
        }
    }
    Ok(values)
}

fn required<'a>(values: &'a BTreeMap<String, String>, name: &str) -> Result<&'a str> {
    values
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| format!("missing {name}").into())
}

fn sha(value: &str) -> Result<()> {
    if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("source SHA must contain exactly 40 hexadecimal characters".into());
    }
    Ok(())
}

fn target(value: &str) -> Result<()> {
    if !TARGETS.contains(&value) {
        return Err("unsupported native target".into());
    }
    Ok(())
}

fn execute() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    let command = args.first().ok_or("expected xtask command")?;
    match command.as_str() {
        "validate-manifests" => validate::manifests(&root()),
        "verify-capabilities" => validate::capabilities(&root()),
        "package" | "verify-package" => {
            let values = options(&args[1..])?;
            let revision = required(&values, "--sha")?;
            let platform = required(&values, "--target")?;
            sha(revision)?;
            target(platform)?;
            if command == "package" {
                package::build(
                    &root(),
                    required(&values, "--binary")?,
                    required(&values, "--out")?,
                    platform,
                    revision,
                )
            } else {
                package::verify(&root(), required(&values, "--archive")?, platform, revision)
            }
        }
        "run" => {
            let split = args
                .iter()
                .position(|arg| arg == "--")
                .ok_or("run requires -- EXEC ARGS")?;
            let values = options(&args[1..split])?;
            let wall: u64 = required(&values, "--wall")?.parse()?;
            let idle: u64 = required(&values, "--idle")?.parse()?;
            if wall == 0 || idle == 0 || idle > wall || wall > 3600 {
                return Err("invalid wall/idle bounds".into());
            }
            let executable = args.get(split + 1).ok_or("missing executable")?;
            let output = process::run(executable, &args[split + 2..], &root(), wall, idle)?;
            if !output.status.success() {
                return Err(format!("owned command failed: {}", output.status).into());
            }
            Ok(())
        }
        _ => Err("unknown xtask command".into()),
    }
}

fn main() {
    use std::io::Write;
    let now = chrono::Utc::now();
    let logs = root().join(".ci-work/logs");
    let mut log = std::fs::create_dir_all(&logs)
        .and_then(|()| {
            std::fs::File::options()
                .create_new(true)
                .write(true)
                .open(logs.join(format!(
                    "xtask_{}_UTC_{}_{}.log",
                    now.format("%Y-%m-%d_%H-%M-%S"),
                    std::process::id(),
                    now.timestamp_subsec_nanos()
                )))
        })
        .ok();
    if let Some(file) = log.as_mut() {
        let _ = writeln!(
            file,
            "[{} UTC] [INFO] [XTASK] Started",
            now.format("%Y-%m-%d %H:%M:%S")
        );
    } else {
        eprintln!("xtask: file logging unavailable; using stderr");
    }
    let result = execute();
    if let Some(file) = log.as_mut() {
        let _ = writeln!(
            file,
            "[{} UTC] [{}] [XTASK] Completed success={}",
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"),
            if result.is_ok() { "INFO" } else { "ERROR" },
            result.is_ok()
        );
        let _ = file.flush();
    }
    if let Err(error) = result {
        eprintln!("xtask: {error}");
        std::process::exit(1);
    }
}
