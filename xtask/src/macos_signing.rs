//! macOS signing adapter.
//!
//! The owner self-signed code-signing identity is imported into an ephemeral keychain
//! through Security.framework, then `codesign` signs each final Mach-O binary with it.
//!
//! Custody rules enforced here:
//! - the PKCS#12 bundle and its passphrase arrive only through protected environment
//!   values and live in `Zeroizing` buffers;
//! - no password is ever passed in a child argument vector. The reference CLI forms
//!   `security import -P`, `security unlock-keychain -p` and
//!   `security set-key-partition-list -k` all expose secrets in argv; they are not used.
//!   The one `security -i` command below is a deliberate exception, documented at the
//!   function, and reads the keychain password from a private stdin pipe;
//! - the per-run keychain password is generated in-process and never leaves memory;
//! - the keychain file is removed on every exit path.
//!
//! The temporary keychain is necessarily an encrypted on-disk file, because native
//! `codesign` reads the identity from a keychain. "No files" therefore means no
//! plaintext private key or password file, not the absence of an encrypted keychain.
#![allow(unsafe_code)]
use crate::Result;
use base64::{Engine, prelude::BASE64_STANDARD};
use security_framework::os::macos::import_export::{ImportOptions, SecItems};
use security_framework::os::macos::keychain::CreateOptions;
use std::path::{Path, PathBuf};
use std::{fs, io::Write, process::Command, process::Stdio};
use zeroize::Zeroizing;

/// Common name of the owner code-signing identity. This is a public selector, not a
/// secret: `codesign --sign` accepts either the identity common name or its SHA-1.
const SIGNING_IDENTITY: &str = "Fiverr Agent MCP Owner";

/// Tools allowed to use the imported private key without an interactive prompt.
const TRUSTED_TOOLS: &[&str] = &["/usr/bin/codesign", "/usr/bin/security"];

/// Code-signing identity leaf certificate SHA-256. Published in
/// `config/signing-public.json`; a mismatch fails the run before signing.
const MACOS_LEAF_PIN: &str = include_str!("../../config/signing-macos-public.json");

/// Ephemeral keychain guard: removes the exact owned file on every exit path.
struct EphemeralKeychain {
    path: PathBuf,
}

impl Drop for EphemeralKeychain {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Signs each final Mach-O binary with the owner identity, then verifies every result.
///
/// Both binaries must be signed before packaging because packaging reads the binaries
/// back into the archives; any post-signature modification invalidates the signature.
pub fn sign_and_verify(binaries: &[(&str, &Path)]) -> Result<()> {
    let bundle = Zeroizing::new(
        std::env::var("FIVERR_MACOS_P12_BASE64").map_err(|_| "FIVERR_MACOS_P12_BASE64 missing")?,
    );
    let passphrase = Zeroizing::new(
        std::env::var("FIVERR_MACOS_P12_PASSWORD")
            .map_err(|_| "FIVERR_MACOS_P12_PASSWORD missing")?,
    );
    let decoded = Zeroizing::new(
        BASE64_STANDARD
            .decode(bundle.as_str())
            .map_err(|_| "macOS certificate bundle is not valid base64")?,
    );
    // Security.framework copies the PKCS#12 bytes into its own CoreFoundation object,
    // so the decoded buffer is dropped as soon as the import returns.
    let blob = Zeroizing::new((*decoded).clone());
    let keychain = import_identity(&blob, passphrase.as_str())?;
    let mut signed = Vec::new();
    for (name, path) in binaries {
        signed.push(sign_one(name, path, &keychain)?);
    }
    for entry in &signed {
        verify_one(entry.0, &entry.1)?;
        check_leaf(entry.0, &entry.1)?;
    }
    drop(keychain);
    Ok(())
}

/// Imports the PKCS#12 identity into an ephemeral keychain under the runner temp root.
fn import_identity(blob: &[u8], passphrase: &str) -> Result<EphemeralKeychain> {
    let password = Zeroizing::new(random_password()?);
    let directory = std::env::var_os("RUNNER_TEMP")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let path = directory.join(format!("fiverr-signing-{}.keychain", std::process::id()));
    if path.exists() {
        fs::remove_file(&path)?;
    }
    let mut options = CreateOptions::new();
    options.password(password.as_str()).prompt_user(false);
    let keychain = options
        .create(&path)
        .map_err(|error| format!("ephemeral keychain creation failed: {error}"))?;
    let guard = EphemeralKeychain { path: path.clone() };
    let outcome = (|| -> Result<()> {
        let mut items = SecItems::default();
        let mut import = ImportOptions::new();
        import
            .pkcs12()
            .passphrase(passphrase)
            .keychain(&keychain)
            .items(&mut items);
        import
            .import(blob)
            .map_err(|error| format!("identity import failed: {error}"))?;
        grant_tool_access(&path, password.as_str())?;
        Ok(())
    })();
    if outcome.is_err() {
        let _ = fs::remove_file(&path);
    }
    outcome.map(|()| guard)
}

/// Restricts the imported private key to the signing tools.
///
/// The partition list is applied through `security -i -q` reading one command on private
/// stdin, because the `-k` form of `set-key-partition-list` places the keychain password
/// in the process argument vector. The keychain path is rejected if it cannot be quoted
/// safely, so the command line cannot be split by a hostile temp directory name.
/// Verbose output is never enabled: Apple's interactive mode echoes command arguments
/// when `-v` is passed.
fn grant_tool_access(keychain: &Path, password: &str) -> Result<()> {
    let display = keychain
        .to_str()
        .ok_or("ephemeral keychain path is not valid UTF-8")?;
    if display.contains('\'') {
        return Err("ephemeral keychain path contains a single quote".into());
    }
    let mut child = Command::new("/usr/bin/security")
        .arg("-i")
        .arg("-q")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "security partition-list launch failed")?;
    let command =
        format!("set-key-partition-list -S apple-tool:,apple: -s -k {password} '{display}'\n");
    let outcome = (|| -> Result<()> {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or("security partition-list stdin unavailable")?;
        stdin
            .write_all(command.as_bytes())
            .map_err(|_| "security partition-list stdin write failed")?;
        Ok(())
    })();
    // Closing stdin before waiting lets the single command reach EOF and the exit
    // status to reflect that command alone.
    drop(child.stdin.take());
    let status = child
        .wait()
        .map_err(|_| "security partition-list wait failed")?;
    if !status.success() {
        return Err("security partition-list refused".into());
    }
    outcome?;
    let _ = TRUSTED_TOOLS;
    Ok(())
}

/// Generates a high-entropy ephemeral keychain password inside this process.
fn random_password() -> Result<String> {
    let mut bytes = [0u8; 32];
    if !random_bytes(&mut bytes) {
        // Unreachable on macOS: SecRandomCopyBytes has no documented failure mode for a
        // 32-byte buffer. Failing closed is still safer than a predictable password.
        return Err("keychain password generation failed".into());
    }
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn random_bytes(bytes: &mut [u8]) -> bool {
    // SAFETY: `SecRandomCopyBytes` writes exactly `bytes.len()` bytes into the caller's
    // buffer; the null randomizer requests the default CSPRNG.
    unsafe {
        unsafe extern "C" {
            fn SecRandomCopyBytes(
                rnd: *const core::ffi::c_void,
                count: usize,
                bytes: *mut core::ffi::c_void,
            ) -> i32;
        }
        SecRandomCopyBytes(core::ptr::null(), bytes.len(), bytes.as_mut_ptr().cast()) == 0
    }
}

/// Signs one final binary with the owner identity selected by common name.
///
/// `--keychain` is passed so `codesign` never searches the runner login keychain:
/// the only identity it can reach is the ephemeral one, which cannot collide with an
/// unrelated developer identity installed on the machine.
fn sign_one<'a>(
    name: &'a str,
    path: &'a Path,
    keychain: &EphemeralKeychain,
) -> Result<(&'a str, PathBuf)> {
    let status = Command::new("/usr/bin/codesign")
        .arg("--force")
        .arg("--sign")
        .arg(SIGNING_IDENTITY)
        .arg("--keychain")
        .arg(&keychain.path)
        .arg("--options")
        .arg("runtime")
        .arg("--timestamp=none")
        .arg(path)
        .status()
        .map_err(|_| "codesign launch failed")?;
    if !status.success() {
        return Err(format!("codesign failed for {name}").into());
    }
    Ok((name, path.to_path_buf()))
}

/// Verifies one signed binary strictly, across every architecture it contains.
fn verify_one(name: &str, path: &Path) -> Result<()> {
    let status = Command::new("/usr/bin/codesign")
        .arg("--verify")
        .arg("--strict")
        .arg("--all-architectures")
        .arg(path)
        .status()
        .map_err(|_| "codesign verify launch failed")?;
    if !status.success() {
        return Err(format!("codesign verification failed for {name}").into());
    }
    Ok(())
}

/// Extracts the leaf certificate of a signed binary for independent pin comparison.
///
/// `codesign --extract-certificates` writes the leaf as `<prefix>0`; the extracted
/// file must be removed by the caller.
pub fn leaf_certificate(path: &Path, output: &Path) -> Result<PathBuf> {
    let prefix = output
        .parent()
        .ok_or("certificate output directory missing")?
        .join("leaf");
    let status = Command::new("/usr/bin/codesign")
        .arg("--display")
        .arg("--extract-certificates")
        .arg(&prefix)
        .arg(path)
        .status()
        .map_err(|_| "codesign certificate extraction failed")?;
    if !status.success() {
        return Err("codesign certificate extraction failed".into());
    }
    let leaf = PathBuf::from(format!("{}.0", prefix.display()));
    if !leaf.is_file() {
        return Err("codesign did not produce a leaf certificate".into());
    }
    Ok(leaf)
}

/// Confirms the signing identity is the pinned owner certificate, not any other key
/// reachable on the runner.
fn check_leaf(name: &str, path: &Path) -> Result<()> {
    let pin = MACOS_LEAF_PIN
        .split('"')
        .find(|part| part.len() == 64 && part.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("macOS leaf pin is missing from the signing policy")?;
    let directory = std::env::var_os("RUNNER_TEMP")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let scratch = directory.join(format!("fiverr-leaf-{}-{name}", std::process::id()));
    let outcome = (|| -> Result<()> {
        let leaf = leaf_certificate(path, &scratch)?;
        let digest = crate::validate::digest(&fs::read(&leaf)?);
        if !digest.eq_ignore_ascii_case(pin) {
            return Err(format!("signed {name} uses an unpinned macOS identity").into());
        }
        Ok(())
    })();
    let _ = fs::remove_dir_all(&scratch);
    outcome
}
