//! Owner SSHSIG signatures for the final non-Windows release archives.
//! Command and stdin contracts: https://man.openbsd.org/ssh-keygen#Y~4
//! Verification uses OpenSSH, not the library's fail-closed parser-only verifier.
use crate::{PRODUCT, Result, VERSION, root, validate};
use base64::{Engine, prelude::BASE64_STANDARD};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    env, fs,
    io::Write,
    os::unix::{fs::DirBuilderExt, fs::OpenOptionsExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zeroize::Zeroizing;

const PRINCIPAL: &str = "fiverr-agent-mcp-release";
const NAMESPACE: &str = "fiverr-agent-mcp-release";
const KEY_ENV: &str = "FIVERR_RELEASE_SIGNING_KEY_BASE64";
const WRONG_NAMESPACE: &str = "fiverr-agent-mcp-release-wrong-namespace";
const NEGATIVE_TESTS: [&str; 2] = ["wrong-namespace-rejected", "wrong-signer-rejected"];

struct EphemeralFiles(PathBuf);
impl EphemeralFiles {
    fn create(base: &Path) -> Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let base = base.canonicalize()?;
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        for _ in 0..32 {
            let path = base.join(format!(
                "fiverr-release-signing-{}-{stamp}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err("cannot create a unique release-signing directory".into())
    }

    fn remove(&self) -> Result<()> {
        // Unlink the exact owned scratch tree, never move key material to a trash folder.
        fs::remove_dir_all(&self.0)?;
        Ok(())
    }
}
impl Drop for EphemeralFiles {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn ssh() -> Command {
    let mut command = Command::new("ssh-keygen");
    // Children need file paths, never the encoded owner key or other signing secrets.
    for name in [
        KEY_ENV,
        "FIVERR_RELEASE_SIGNING_PUBLIC_KEY",
        "FIVERR_MACOS_P12_BASE64",
        "FIVERR_MACOS_P12_PASSWORD",
        "FIVERR_WINDOWS_PFX_BASE64",
        "FIVERR_WINDOWS_PFX_PASSWORD",
        "SSH_AUTH_SOCK",
        "SSH_ASKPASS",
    ] {
        command.env_remove(name);
    }
    command
        .env("SSH_ASKPASS_REQUIRE", "never")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

fn run(mut command: Command) -> Result<bool> {
    let mut child = OwnedChild(command.spawn()?);
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(status) = child.0.try_wait()? {
            return status
                .code()
                .map(|code| code == 0)
                .ok_or_else(|| "ssh-keygen terminated by a signal".into());
        }
        if Instant::now() >= deadline {
            return Err("ssh-keygen exceeded the 60-second signing deadline".into());
        }
        // Child has no readiness channel; bounded exit polling also covers verification.
        thread::sleep(Duration::from_millis(10));
    }
}

fn private_key(value: Option<String>) -> Result<Zeroizing<Vec<u8>>> {
    let raw = Zeroizing::new(value.ok_or("FIVERR_RELEASE_SIGNING_KEY_BASE64 missing")?);
    if raw.len() > 64 * 1024 {
        return Err("release signing key exceeds its input limit".into());
    }
    let bytes = Zeroizing::new(
        BASE64_STANDARD
            .decode(raw.as_bytes())
            .map_err(|_| "release signing key is not valid base64")?,
    );
    if !bytes.starts_with(b"-----BEGIN OPENSSH PRIVATE KEY-----\n")
        || !bytes.ends_with(b"-----END OPENSSH PRIVATE KEY-----\n")
    {
        return Err("release signing key must be an OpenSSH private key".into());
    }
    Ok(bytes)
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn signature_path(file: &Path) -> PathBuf {
    let mut name = file.as_os_str().to_os_string();
    name.push(".sig");
    PathBuf::from(name)
}

fn sign(file: &Path, key: &Path) -> Result<()> {
    if fs::symlink_metadata(signature_path(file)).is_ok() {
        return Err("release signature already exists; refusing an overwrite prompt".into());
    }
    let mut command = ssh();
    command.args(["-Y", "sign", "-n", NAMESPACE, "-f"]);
    command.arg(key).arg(file);
    if !run(command)? {
        return Err("ssh-keygen release signing failed".into());
    }
    let metadata = fs::symlink_metadata(signature_path(file))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > 16 * 1024 {
        return Err("expected a bounded regular SSHSIG file".into());
    }
    Ok(())
}

fn verify(file: &Path, allowed: &Path, namespace: &str) -> Result<bool> {
    let mut command = ssh();
    command.args(["-Y", "verify", "-f"]);
    command.arg(allowed);
    command.args(["-I", PRINCIPAL, "-n", namespace, "-s"]);
    command.arg(signature_path(file));
    // OpenSSH reads the signed message from stdin, not a final filename argument.
    command.stdin(fs::File::open(file)?);
    run(command)
}

fn allowed_signers(path: &Path, public_key: &str) -> Result<()> {
    write_private(path, format!("{PRINCIPAL} {public_key}\n").as_bytes())
}

fn generate_key(path: &Path) -> Result<()> {
    let mut command = ssh();
    command.args(["-t", "ed25519", "-N", "", "-f"]);
    command.arg(path);
    if !run(command)? {
        return Err("ssh-keygen negative-control key generation failed".into());
    }
    Ok(())
}

fn public_policy(workspace: &Path) -> Result<(String, String)> {
    let policy: Value = serde_json::from_slice(&validate::regular(
        workspace,
        "config/signing-release-public.json",
    )?)?;
    if policy["principal"] != PRINCIPAL {
        return Err("release signing policy principal mismatch".into());
    }
    let key = policy["public_key"]
        .as_str()
        .ok_or("release signing policy public_key missing")?;
    if key.len() > 16 * 1024 || !key.starts_with("ssh-ed25519 ") || key.contains(['\r', '\n', '\0'])
    {
        return Err("release signing policy requires one Ed25519 public key line".into());
    }
    // The published policy preserves the public file's canonical UTF-8 bytes plus LF.
    // This is a file digest, deliberately not OpenSSH's decoded-key fingerprint.
    let fingerprint = validate::digest(format!("{key}\n").as_bytes());
    if policy["public_key_sha256"] != fingerprint {
        return Err("release signing policy public key fingerprint mismatch".into());
    }
    match env::var("FIVERR_RELEASE_SIGNING_PUBLIC_KEY") {
        // A secret the job's environment does not hold arrives as an empty string, not as
        // an unset variable. That is "not provisioned on this host", not a mismatch: the
        // pinned policy file has already verified itself above.
        Ok(value) if !value.trim().is_empty() && value.trim_end_matches(['\r', '\n']) != key => {
            return Err(
                "release signing public key environment differs from the pinned policy".into(),
            );
        }
        Err(env::VarError::NotUnicode(_)) => {
            return Err("release signing public key environment is not UTF-8".into());
        }
        _ => {}
    }
    Ok((key.to_owned(), fingerprint))
}

fn inputs(dist: &Path, sha: &str) -> Result<(Vec<PathBuf>, BTreeSet<String>)> {
    if !fs::symlink_metadata(dist)?.is_dir() {
        return Err("release distribution must be a regular directory".into());
    }
    let mut files = Vec::new();
    let mut targets = BTreeSet::new();
    for entry in fs::read_dir(dist)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or("non-UTF8 release distribution filename")?;
        let sums = name
            .strip_prefix("SHA256SUMS-")
            .and_then(|name| name.strip_suffix(".txt"));
        // Sign ALL tar.gz files, including corresponding source, not only native binaries.
        // Plugin ZIP bytes remain covered by the signed per-target SHA256SUMS file.
        if !name.ends_with(".tar.gz") && sums.is_none() {
            continue;
        }
        if !entry.file_type()?.is_file() {
            return Err("release signing input must be a regular file, not a link".into());
        }
        if let Some(target) = sums {
            crate::target(target)?;
            let metadata: Value = serde_json::from_slice(&validate::regular(
                dist,
                &format!("release-{target}.json"),
            )?)?;
            if metadata["source_sha"] != sha || metadata["target"] != target {
                return Err(
                    "release signing source SHA or target differs from packaged metadata".into(),
                );
            }
            targets.insert(target.to_owned());
        }
        if fs::symlink_metadata(signature_path(&entry.path())).is_ok() {
            return Err("release signature already exists; refusing to replace it".into());
        }
        files.push(entry.path());
    }
    if targets.is_empty()
        || !files
            .iter()
            .any(|file| file.to_string_lossy().ends_with(".tar.gz"))
    {
        return Err("release archives and per-target SHA256SUMS are required".into());
    }
    for target in &targets {
        if !files.contains(&dist.join(format!("{PRODUCT}-{VERSION}-{target}.tar.gz"))) {
            return Err("native release archive missing for checksum target".into());
        }
    }
    files.sort();
    Ok((files, targets))
}

pub fn archives(sha: &str) -> Result<()> {
    crate::sha(sha)?;
    let workspace = root();
    let dist = workspace.join("dist");
    let (public_key, fingerprint) = public_policy(&workspace)?;
    let (files, targets) = inputs(&dist, sha)?;
    let key = private_key(env::var(KEY_ENV).ok())?;
    let base = env::var_os("RUNNER_TEMP")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    let scratch = EphemeralFiles::create(&base)?;
    let key_path = scratch.0.join("owner-release");
    write_private(&key_path, &key)?;
    drop(key);
    let allowed = scratch.0.join("allowed_signers");
    allowed_signers(&allowed, &public_key)?;
    let wrong_key = scratch.0.join("wrong-owner");
    generate_key(&wrong_key)?;
    let wrong_allowed = scratch.0.join("wrong_allowed_signers");
    let wrong_public = fs::read_to_string(wrong_key.with_extension("pub"))?;
    allowed_signers(&wrong_allowed, wrong_public.trim_end())?;
    let mut signed = Vec::new();
    for file in files {
        sign(&file, &key_path)?;
        if !verify(&file, &allowed, NAMESPACE)? {
            return Err("release signature failed pinned-owner verification".into());
        }
        if verify(&file, &allowed, WRONG_NAMESPACE)? {
            return Err("release signature accepted the wrong namespace".into());
        }
        if verify(&file, &wrong_allowed, NAMESPACE)? {
            return Err("release signature accepted the wrong signer".into());
        }
        let signature = signature_path(&file);
        signed.push(json!({
            "file": file.file_name().and_then(|name| name.to_str()).ok_or("archive basename")?,
            "signature_file": signature.file_name().and_then(|name| name.to_str()).ok_or("signature basename")?,
            "signature_sha256": validate::digest(&fs::read(&signature)?),
        }));
    }
    // Cleanup failure is fatal before a success receipt can be published; Drop retries on errors.
    scratch.remove()?;
    let receipt = json!({
        "source_sha": sha,
        "targets": targets,
        "signed_files": signed,
        "namespace": NAMESPACE,
        "principal": PRINCIPAL,
        "public_key_sha256": fingerprint,
        "negative_tests": NEGATIVE_TESTS,
    });
    let work = workspace.join(".ci-work");
    fs::create_dir_all(&work)?;
    if !fs::symlink_metadata(&work)?.is_dir() {
        return Err("release receipt directory must not be a link".into());
    }
    let path = work.join("release-signing-receipt.json");
    let published = dist.join("release-signing-receipt.json");
    // Do not follow a stale receipt symlink while writing or copying public evidence.
    for destination in [&path, &published] {
        match fs::symlink_metadata(destination) {
            Ok(metadata) if !metadata.is_file() => {
                return Err("release signing receipt must be a regular file, not a link".into());
            }
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        }
    }
    fs::write(&path, serde_json::to_vec_pretty(&receipt)?)?;
    fs::copy(&path, published)?;
    eprintln!(
        "release signing verified targets={} files={}",
        receipt["targets"],
        signed.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        _scratch: EphemeralFiles,
        file: PathBuf,
        allowed: PathBuf,
        wrong_allowed: PathBuf,
    }

    fn fixture() -> Option<Fixture> {
        let scratch = EphemeralFiles::create(&env::temp_dir()).unwrap();
        let key = scratch.0.join("test-owner");
        if let Err(error) = generate_key(&key) {
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
            {
                eprintln!("SKIP release SSHSIG test: ssh-keygen is unavailable");
                return None;
            }
            panic!("throwaway signing key generation failed: {error}");
        }
        let file = scratch.0.join("archive with spaces.tar.gz");
        fs::write(&file, b"throwaway release-signing fixture\n").unwrap();
        sign(&file, &key).unwrap();
        let allowed = scratch.0.join("allowed_signers");
        allowed_signers(
            &allowed,
            fs::read_to_string(key.with_extension("pub"))
                .unwrap()
                .trim_end(),
        )
        .unwrap();
        let wrong = scratch.0.join("wrong-owner");
        generate_key(&wrong).unwrap();
        let wrong_allowed = scratch.0.join("wrong_allowed_signers");
        allowed_signers(
            &wrong_allowed,
            fs::read_to_string(wrong.with_extension("pub"))
                .unwrap()
                .trim_end(),
        )
        .unwrap();
        assert!(verify(&file, &allowed, NAMESPACE).unwrap());
        Some(Fixture {
            _scratch: scratch,
            file,
            allowed,
            wrong_allowed,
        })
    }

    #[test]
    fn rejects_wrong_namespace() {
        let Some(fixture) = fixture() else { return };
        assert!(!verify(&fixture.file, &fixture.allowed, WRONG_NAMESPACE).unwrap());
    }

    #[test]
    fn rejects_wrong_signer() {
        let Some(fixture) = fixture() else { return };
        assert!(!verify(&fixture.file, &fixture.wrong_allowed, NAMESPACE).unwrap());
    }

    #[test]
    fn missing_key_env_fails_closed() {
        // Exercise the same absent-env path without mutating process-global test state.
        let error = private_key(None).unwrap_err();
        assert_eq!(
            error.to_string(),
            "FIVERR_RELEASE_SIGNING_KEY_BASE64 missing"
        );
    }

    #[test]
    fn signature_filename_appends_without_replacing_extensions() {
        for name in [
            "native.tar.gz",
            "source.tar.gz",
            "SHA256SUMS-linux.txt",
            "archive ü.tar.gz",
        ] {
            assert_eq!(
                signature_path(Path::new(name)),
                PathBuf::from(format!("{name}.sig"))
            );
        }
    }
}
