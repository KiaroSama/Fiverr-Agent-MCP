//! Explicit, Linux-only, scratch-contained OS denial qualification.
use crate::{Result, process, root};
use serde_json::Value;
use std::fs;

fn docker(args: &[&str], wall: u64) -> Result<process::Output> {
    let args = args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let out = process::run("docker", &args, &root(), wall, wall)?;
    if !out.status.success() {
        return Err("bounded Docker operation failed".into());
    }
    Ok(out)
}

struct Container(String);
impl Container {
    fn remove(&self) -> Result<()> {
        docker(&["rm", "--force", &self.0], 20)?;
        let out = process::run(
            "docker",
            &["inspect".into(), self.0.clone()],
            &root(),
            10,
            10,
        )?;
        if out.status.success() {
            return Err("owned sandbox survived removal".into());
        }
        Ok(())
    }
}
impl Drop for Container {
    fn drop(&mut self) {
        if let Err(e) = self.remove() {
            eprintln!("sandbox cleanup failed: {e}");
        }
    }
}

pub fn run() -> Result<()> {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return Err("OS denial qualification requires native Linux x86_64".into());
    }
    // Dependency preparation/build is ordinary CI, outside target execution. The
    // actual qualified binary runs only after all container controls are checked.
    let build = process::run(
        "cargo",
        &[
            "test".into(),
            "--locked".into(),
            "--package".into(),
            "xtask".into(),
            "--no-run".into(),
            "--jobs".into(),
            "2".into(),
            "--message-format=json".into(),
        ],
        &root(),
        900,
        180,
    )?;
    if !build.status.success() {
        return Err("sandbox fixture preparation failed".into());
    }
    let binary = std::str::from_utf8(&build.stdout)?
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find(|v| {
            v["reason"] == "compiler-artifact"
                && v["target"]["name"] == "xtask"
                && v["target"]["kind"] == serde_json::json!(["bin"])
                && v["target"]["src_path"]
                    .as_str()
                    .is_some_and(|p| p.ends_with("/xtask/src/main.rs"))
                && v["profile"]["test"] == true
        })
        .and_then(|v| v["executable"].as_str().map(str::to_owned))
        .ok_or("fixture executable missing")?;
    let binary = fs::canonicalize(binary)?;
    // Resolve the already available runner image to an immutable local image ID.
    let image = docker(
        &["image", "inspect", "ubuntu:22.04", "--format", "{{.Id}}"],
        20,
    )?;
    let image = std::str::from_utf8(&image.stdout)?.trim();
    if !image.starts_with("sha256:") || image.len() != 71 {
        return Err("sandbox image identity invalid".into());
    }
    let input = binary.parent().ok_or("fixture parent missing")?;
    let mount = format!("type=bind,src={},dst=/input,readonly", input.display());
    let exe = format!(
        "/input/{}",
        binary
            .file_name()
            .ok_or("fixture filename missing")?
            .to_str()
            .ok_or("fixture filename encoding")?
    );
    let created = docker(
        &[
            "create",
            "--pull=never",
            "--network=none",
            "--read-only",
            "--user=65532:65532",
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges",
            "--cpus=1",
            "--memory=512m",
            "--memory-swap=512m",
            "--pids-limit=32",
            "--ulimit=cpu=30:30",
            "--ulimit=fsize=8388608:8388608",
            "--ulimit=nofile=128:128",
            "--tmpfs=/scratch:rw,nosuid,nodev,noexec,size=16777216,uid=65532,gid=65532,mode=700",
            "--tmpfs=/dev/shm:ro,nosuid,nodev,noexec,size=4096",
            "--mount",
            &mount,
            "--workdir=/scratch",
            image,
            "/usr/bin/env",
            "-i",
            "HOME=/scratch",
            "TMPDIR=/scratch",
            "XDG_CACHE_HOME=/scratch",
            "PATH=/usr/bin:/bin",
            "FIVERR_SANDBOX=required",
            &exe,
            "--exact",
            "bootstrap_owner_tests::owner_source::terminal_tests::denial::sandboxed_kernel_cleanup_denial",
            "--nocapture",
        ],
        20,
    )?;
    let id = std::str::from_utf8(&created.stdout)?.trim();
    if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("sandbox container identity invalid".into());
    }
    let container = Container(id.into());
    let inspection = docker(&["inspect", id], 20)?;
    let value: Value = serde_json::from_slice(&inspection.stdout)?;
    let host = &value[0]["HostConfig"];
    if host["NetworkMode"] != "none"
        || host["ReadonlyRootfs"] != true
        || host["Memory"] != 536870912u64
        || host["MemorySwap"] != 536870912u64
        || host["NanoCpus"] != 1000000000u64
        || host["PidsLimit"] != 32
        || value[0]["Config"]["User"] != "65532:65532"
        || !host["CapDrop"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v == "ALL"))
        || !host["SecurityOpt"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v == "no-new-privileges"))
        || !value[0]["Mounts"]
            .as_array()
            .is_some_and(|a| a.len() == 1 && a[0]["RW"] == false && a[0]["Destination"] == "/input")
    {
        return Err("sandbox controls failed pre-start inspection".into());
    }
    let output = docker(&["start", "--attach", id], 45)?;
    let text = std::str::from_utf8(&output.stdout)?;
    if !text.contains("actual EPERM, bounded refusal, adopted child reaped") {
        return Err("sandbox semantic evidence absent".into());
    }
    let state = docker(&["inspect", id, "--format", "{{json .State}}"], 10)?;
    let state: Value = serde_json::from_slice(&state.stdout)?;
    if state["Running"] != false || state["ExitCode"] != 0 || state["OOMKilled"] != false {
        return Err("sandbox did not exit cleanly".into());
    }
    container.remove()?;
    std::mem::forget(container);
    fs::write(
        root().join(".ci-work/sandbox-acceptance.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "scenario":"kernel_cleanup_denial", "native_target":"x86_64-unknown-linux-gnu",
            "image":image, "network":"none", "source_read_only":true, "scratch_bytes":16777216,
            "memory_bytes":536870912, "cpus":1, "pids":32, "file_bytes":8388608,
            "cpu_seconds":30, "wall_seconds":45, "actual_eperm":true, "owned_child_reaped":true,
            "container_removed":true, "scope":"controlled fixture, not arbitrary race safety"
        }))?,
    )?;
    Ok(())
}
