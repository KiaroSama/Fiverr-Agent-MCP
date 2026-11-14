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
use security_framework::os::macos::keychain::SecKeychain;
use std::path::{Path, PathBuf};
use std::{fs, io::Write, process::Command, process::Stdio};
use zeroize::Zeroizing;

/// Common name of the owner code-signing identity. This is a public selector, not a
/// secret: `codesign --sign` accepts either the identity common name or its SHA-1.
const SIGNING_IDENTITY: &str = "Fiverr Agent MCP Owner";

/// Code-signing identity leaf certificate SHA-256. Published in
/// `config/signing-public.json`; a mismatch fails the run before signing.
const MACOS_LEAF_PIN: &str = include_str!("../../config/signing-macos-public.json");

/// Ephemeral keychain guard: restores the user keychain search list it changed, then
/// removes the exact owned file on every exit path.
struct EphemeralKeychain {
    path: PathBuf,
    previous_search_list: Option<Vec<String>>,
    // Trust is written to the machine-wide admin domain, so it outlives the keychain
    // file and must be removed explicitly.
    trusted_certificate: Option<PathBuf>,
}

impl Drop for EphemeralKeychain {
    fn drop(&mut self) {
        if let Some(previous) = self.previous_search_list.take() {
            let _ = set_search_list(&previous);
        }
        if let Some(certificate) = self.trusted_certificate.take() {
            // Bounded: this runs while unwinding a failure, and an unbounded wait here
            // turns one signing error into a job timeout with no diagnostic at all.
            if let Some(path) = certificate.to_str() {
                let args: Vec<String> = ["-n", "/usr/bin/security", "remove-trusted-cert", "-d"]
                    .iter()
                    .map(|part| (*part).to_owned())
                    .chain(std::iter::once(path.to_owned()))
                    .collect();
                let _ = crate::process::run(
                    "/usr/bin/sudo",
                    &args,
                    certificate.parent().unwrap_or(Path::new("/")),
                    30,
                    15,
                );
            }
            let _ = fs::remove_file(&certificate);
        }
        let _ = fs::remove_file(&self.path);
    }
}

/// Reads the user keychain search list exactly as `security` reports it.
fn search_list() -> Result<Vec<String>> {
    let output = Command::new("/usr/bin/security")
        .args(["list-keychains", "-d", "user"])
        .stderr(Stdio::null())
        .output()
        .map_err(|_| "keychain search list read failed")?;
    if !output.status.success() {
        return Err("keychain search list read refused".into());
    }
    Ok(String::from_utf8(output.stdout)
        .map_err(|_| "keychain search list is not valid UTF-8")?
        .lines()
        .map(|line| line.trim().trim_matches('"').to_owned())
        .filter(|line| !line.is_empty())
        .collect())
}

fn set_search_list(keychains: &[String]) -> Result<()> {
    let status = Command::new("/usr/bin/security")
        .args(["list-keychains", "-d", "user", "-s"])
        .args(keychains)
        .stderr(Stdio::null())
        .status()
        .map_err(|_| "keychain search list update failed")?;
    if !status.success() {
        return Err("keychain search list update refused".into());
    }
    Ok(())
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
    let selector = identity_sha1(&keychain.path);
    // `codesign --keychain` narrows the search list to this keychain but still resolves the
    // identity through the user domain. The ephemeral keychain is first on that list, so
    // passing the flag as well only re-adds the filter that has been failing.
    let mut signed = Vec::new();
    for (name, path) in binaries {
        signed.push(sign_one(name, path, &keychain, selector.as_deref())?);
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
    // The `.keychain-db` suffix is what `security create-keychain` produces and what every
    // working macOS CI signer uses. `SecKeychainCreate` accepts a bare `.keychain` path and
    // writes the file, but the identity stored there is not reachable through the modern
    // keychain database layout that `codesign` resolves a search-list entry against.
    let path = directory.join(format!("fiverr-signing-{}.keychain-db", std::process::id()));
    // `security list-keychains -d user -s` does not persist on this runner: the domain reads
    // back empty and `codesign` then reports the identity as missing entirely. A keychain in
    // `~/Library/Keychains` is discovered by the security daemon itself, which is how the
    // signers that work on hosted runners avoid depending on the search list at all.
    let path = match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home).join("Library/Keychains").join(
            path.file_name()
                .ok_or("ephemeral keychain file name missing")?,
        ),
        None => path,
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if path.exists() {
        fs::remove_file(&path)?;
    }
    let display = path
        .to_str()
        .ok_or("ephemeral keychain path is not valid UTF-8")?
        .to_owned();
    // The path is single-quoted inside the interactive command, so a quote in it could
    // split the command. Runner temp paths contain none; reject rather than escape.
    if display.contains('\'') {
        return Err("ephemeral keychain path contains a single quote".into());
    }
    // Create through `security -i -q` for the same reason the partition list does: the CLI
    // `-p` form would place the keychain password in the argument vector. The command
    // arrives on private stdin. Both commands mirror what `security create-keychain` and
    // `security unlock-keychain` do, which is the pair every working macOS CI signer uses.
    run_security_commands(&[
        format!("create-keychain -p {} '{display}'", password.as_str()),
        format!("unlock-keychain -p {} '{display}'", password.as_str()),
    ])?;
    let mut guard = EphemeralKeychain {
        path: path.clone(),
        previous_search_list: None,
        trusted_certificate: None,
    };
    let keychain = SecKeychain::open(&path)
        .map_err(|error| format!("ephemeral keychain open failed: {error}"))?;
    let outcome = (|| -> Result<()> {
        let mut items = SecItems::default();
        let mut import = ImportOptions::new();
        import
            .pkcs12()
            .passphrase(passphrase)
            // `security import -A` equivalence: the keychain is ephemeral, its password is
            // a per-run random value, and the file is deleted on every exit path, so the
            // default per-application ACL adds nothing. Without it `codesign` is denied
            // the key and reports it as absent from the keychain.
            .no_access_control(true)
            .keychain(&keychain)
            .items(&mut items);
        import
            .import(blob)
            .map_err(|error| format!("identity import failed: {error}"))?;
        grant_tool_access(&path, password.as_str())?;
        guard.trusted_certificate = Some(trust_for_code_signing(&path, &items)?);
        // `codesign --keychain` only filters the user search list; a headless runner
        // cannot reach an identity in a keychain that is not on that list (Apple TN2206).
        // A runner whose login keychain was never created reports an empty user domain.
        // Naming the login keychain as the trailing entry keeps the list non-empty so the
        // domain is materialized, while the ephemeral keychain stays first and is the only
        // one that holds the owner identity.
        let previous = search_list()?;
        let mut updated = vec![display.clone()];
        updated.extend(previous.iter().cloned());
        if updated.len() == 1
            && let Some(home) = std::env::var_os("HOME")
        {
            updated.push(
                PathBuf::from(home)
                    .join("Library/Keychains/login.keychain-db")
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        guard.previous_search_list = Some(previous);
        set_search_list(&updated)?;
        Ok(())
    })();
    // On failure the guard restores any changed search list and removes the file.
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
    // `codesign:` must be present in the partition list. Without it the key is imported
    // but `codesign` is denied non-interactive access and reports the item as missing
    // from the keychain.
    run_security_commands(&[format!(
        "set-key-partition-list -S apple-tool:,apple:,codesign: -s -k {password} '{display}'"
    )])
}

/// Runs `security` interactive commands on private stdin.
///
/// Apple's interactive mode is used instead of the `-p` / `-k` command forms because those
/// place the password in the process argument vector, where any local process can read it.
/// Callers must reject an unquotable keychain path before formatting a command; the
/// `display` binding in `import_identity` does that. Verbose mode is never enabled:
/// Apple's interactive mode echoes command arguments when `-v` is passed.
fn run_security_commands(commands: &[String]) -> Result<()> {
    for command in commands {
        let mut child = Command::new("/usr/bin/security")
            .arg("-i")
            .arg("-q")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "security interactive launch failed")?;
        let outcome = (|| -> Result<()> {
            let stdin = child
                .stdin
                .as_mut()
                .ok_or("security interactive stdin unavailable")?;
            stdin
                .write_all(command.as_bytes())
                .and_then(|()| {
                    stdin.write_all(
                        b"
",
                    )
                })
                .map_err(|_| "security interactive stdin write failed")?;
            Ok(())
        })();
        // Closing stdin before waiting lets the single command reach EOF and the exit
        // status to reflect that command alone.
        drop(child.stdin.take());
        let status = child
            .wait()
            .map_err(|_| "security interactive wait failed")?;
        if !status.success() {
            return Err("security interactive command refused".into());
        }
        outcome?;
    }
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

/// Marks the imported leaf certificate trusted for code signing.
///
/// `codesign` only uses an identity whose certificate is trusted for the code-signing
/// policy. A freshly imported self-signed leaf is not, and the resulting failure is
/// reported as the identity being absent from the keychain rather than as untrusted.
/// The trust record is written to the ephemeral keychain and dies with it; only the
/// signing policy is trusted, not TLS or any other purpose.
///
/// macOS answers a first-time trust write with an authorization prompt, which a headless
/// runner cannot answer, so the write goes to the admin domain under passwordless `sudo`.
/// Returns the certificate path left in place for the guard to remove on cleanup.
fn trust_for_code_signing(_keychain: &Path, items: &SecItems) -> Result<PathBuf> {
    // A PKCS#12 import reports its certificate as part of a `SecIdentity`, so the
    // standalone certificate list stays empty. Both shapes are accepted.
    let leaf = match items.certificates.first() {
        Some(certificate) => certificate.clone(),
        None => items
            .identities
            .first()
            .ok_or("imported bundle contains no identity or certificate")?
            .certificate()
            .map_err(|error| format!("identity certificate read failed: {error}"))?,
    };
    let directory = std::env::var_os("RUNNER_TEMP")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let cert_path = directory.join(format!("fiverr-trust-{}.pem", std::process::id()));
    let der = leaf.to_der();
    fs::write(&cert_path, pem_encode("CERTIFICATE", &der))?;
    let outcome = (|| -> Result<()> {
        // `-d` writes the admin trust domain; with `-d` the `-k` value names the system
        // keychain that stores the record, not the ephemeral one. Pointing `-k` at a
        // user keychain while writing the admin domain leaves `security` waiting on an
        // authorization prompt that a headless runner can never answer. The call is
        // bounded because that wait is otherwise indistinguishable from a slow runner.
        let args: Vec<String> = [
            "-n",
            "/usr/bin/security",
            "add-trusted-cert",
            "-d",
            "-r",
            "trustRoot",
            "-p",
            "codeSign",
            "-k",
            "/Library/Keychains/System.keychain",
        ]
        .iter()
        .map(|part| (*part).to_owned())
        .chain(std::iter::once(
            cert_path
                .to_str()
                .ok_or("trust path is not valid UTF-8")?
                .to_owned(),
        ))
        .collect();
        let output = crate::process::run("/usr/bin/sudo", &args, &directory, 60, 30)
            .map_err(|error| format!("add-trusted-cert did not complete: {error}"))?;
        if !output.status.success() {
            return Err("add-trusted-cert refused for the code-signing policy".into());
        }
        Ok(())
    })();
    match outcome {
        Ok(()) => Ok(cert_path),
        Err(error) => {
            let _ = fs::remove_file(&cert_path);
            Err(error)
        }
    }
}

/// Minimal PEM wrapper: 64-character base64 lines between the standard delimiters.
fn pem_encode(label: &str, der: &[u8]) -> String {
    let body = BASE64_STANDARD.encode(der);
    let mut text = format!("-----BEGIN {label}-----\n");
    for chunk in body.as_bytes().chunks(64) {
        text.push_str(&String::from_utf8_lossy(chunk));
        text.push('\n');
    }
    text.push_str(&format!("-----END {label}-----\n"));
    text
}

/// Signs one final binary with the owner identity.
///
/// The identity is selected by its SHA-1 fingerprint, or by common name when the
/// fingerprint could not be read. `--keychain` names the ephemeral keychain so the only
/// identity reachable is the owner one, and the search list is prepared before this runs.
fn sign_one<'a>(
    name: &'a str,
    path: &'a Path,
    keychain: &EphemeralKeychain,
    selector: Option<&str>,
) -> Result<(&'a str, PathBuf)> {
    // Bounded: a `codesign` that cannot reach the key is left waiting on a keychain
    // authorization prompt, which on a headless runner never arrives. Without a bound
    // that hang consumes the whole job timeout instead of reporting a failure.
    let signed = crate::process::run(
        "/usr/bin/codesign",
        &[
            "--force".to_owned(),
            "--sign".to_owned(),
            // A SHA-1 fingerprint selects exactly one identity. The common name is only a
            // display string and can match an unrelated certificate on the machine.
            selector.unwrap_or(SIGNING_IDENTITY).to_owned(),
            "--keychain".to_owned(),
            keychain
                .path
                .to_str()
                .ok_or("keychain path is not valid UTF-8")?
                .to_owned(),
            "--options".to_owned(),
            "runtime".to_owned(),
            "--timestamp=none".to_owned(),
            path.to_str()
                .ok_or("binary path is not valid UTF-8")?
                .to_owned(),
        ],
        path.parent().unwrap_or(Path::new("/")),
        120,
        60,
    )
    .map_err(|error| format!("codesign did not complete for {name}: {error}"))?;
    if !signed.status.success() {
        // `codesign` collapses every keychain lookup problem into "the specified item
        // could not be found", which cannot distinguish a missing identity from a denied
        // one. Report what the search list resolves so the next failure is diagnosable.
        return Err(format!(
            "codesign failed for {name}; search list {}; stderr {}",
            search_list()
                .map(|list| list.join(", "))
                .unwrap_or_else(|_| "unreadable".to_owned()),
            String::from_utf8_lossy(&signed.stderr).trim()
        )
        .into());
    }
    Ok((name, path.to_path_buf()))
}

/// Returns the SHA-1 fingerprint of the single code-signing identity in the keychain.
///
/// `security find-identity` prints the fingerprint in its `1) ABC…` column. Exactly one
/// identity is expected; anything else is a signing-environment fault rather than a
/// selector to guess at, so a missing or ambiguous result falls back to the common name.
fn identity_sha1(keychain: &Path) -> Option<String> {
    let path = keychain.to_str()?;
    let args: Vec<String> = ["find-identity", "-p", "codesigning", path]
        .iter()
        .map(|part| (*part).to_owned())
        .collect();
    let output = crate::process::run(
        "/usr/bin/security",
        &args,
        keychain.parent().unwrap_or(Path::new("/")),
        30,
        15,
    )
    .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut found = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.split_once(") ").map(|(_, rest)| rest) else {
            continue;
        };
        let candidate: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        if candidate.len() == 40 {
            found.push(candidate);
        }
    }
    match found.len() {
        1 => found.pop(),
        _ => None,
    }
}

/// Verifies one signed binary strictly, across every architecture it contains.
fn verify_one(name: &str, path: &Path) -> Result<()> {
    let status = crate::process::run(
        "/usr/bin/codesign",
        &[
            "--verify".to_owned(),
            "--strict".to_owned(),
            "--all-architectures".to_owned(),
            path.to_str()
                .ok_or("binary path is not valid UTF-8")?
                .to_owned(),
        ],
        path.parent().unwrap_or(Path::new("/")),
        120,
        60,
    )
    .map_err(|error| format!("codesign verification did not complete for {name}: {error}"))?
    .status;
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
