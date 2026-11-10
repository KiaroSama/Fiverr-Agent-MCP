//! Windows signing adapter. Private key material never reaches child argv or files.
#![allow(unsafe_code)]
use crate::{Result, root};
use base64::{Engine, prelude::BASE64_STANDARD};
use serde_json::{Value, json};
use std::{fs, mem::size_of, os::windows::ffi::OsStrExt, path::Path};
use windows_sys::Win32::Security::Cryptography::*;
use xtask::{
    pe_signature,
    windows_verification::{Certificate, certificate_matches, embedded_policy, verify},
};
use zeroize::Zeroizing;

struct Store(HCERTSTORE);
impl Drop for Store {
    fn drop(&mut self) {
        unsafe {
            CertCloseStore(self.0, 0);
        }
    }
}
fn sign(file: &Path, p: &Value) -> Result<()> {
    let raw = Zeroizing::new(
        std::env::var("FIVERR_WINDOWS_PFX_BASE64")
            .map_err(|_| "protected signing input missing")?,
    );
    let password = Zeroizing::new(
        std::env::var("FIVERR_WINDOWS_PFX_PASSWORD")
            .map_err(|_| "protected signing input missing")?,
    );
    let mut bytes = Zeroizing::new(
        BASE64_STANDARD
            .decode(raw.as_bytes())
            .map_err(|_| "invalid protected signing input")?,
    );
    let wide = Zeroizing::new(password.encode_utf16().chain(Some(0)).collect::<Vec<_>>());
    // SAFETY: SDK structures/pointers refer to owned live buffers; no key is persisted or exported.
    unsafe {
        let blob = CRYPT_INTEGER_BLOB {
            cbData: bytes.len().try_into()?,
            pbData: bytes.as_mut_ptr(),
        };
        let store = Store(PFXImportCertStore(
            &blob,
            wide.as_ptr(),
            PKCS12_NO_PERSIST_KEY | CRYPT_USER_KEYSET,
        ));
        if store.0.is_null() {
            return Err("private signing store import failed".into());
        }
        let cert = Certificate(CertEnumCertificatesInStore(store.0, std::ptr::null()));
        if cert.0.is_null() {
            return Err("private signing certificate absent".into());
        }
        certificate_matches(cert.0, p)?;
        let name = file
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let mut file_info = SIGNER_FILE_INFO {
            cbSize: size_of::<SIGNER_FILE_INFO>() as u32,
            pwszFileName: name.as_ptr(),
            ..Default::default()
        };
        let mut index = 0;
        let subject = SIGNER_SUBJECT_INFO {
            cbSize: size_of::<SIGNER_SUBJECT_INFO>() as u32,
            pdwIndex: &mut index,
            dwSubjectChoice: SIGNER_SUBJECT_FILE,
            Anonymous: SIGNER_SUBJECT_INFO_0 {
                pSignerFileInfo: &mut file_info,
            },
        };
        let mut info = SIGNER_CERT_STORE_INFO {
            cbSize: size_of::<SIGNER_CERT_STORE_INFO>() as u32,
            pSigningCert: cert.0,
            dwCertPolicy: SIGNER_CERT_POLICY_STORE,
            hCertStore: store.0,
        };
        let signer = SIGNER_CERT {
            cbSize: size_of::<SIGNER_CERT>() as u32,
            dwCertChoice: SIGNER_CERT_STORE,
            Anonymous: SIGNER_CERT_0 {
                pCertStoreInfo: &mut info,
            },
            ..Default::default()
        };
        let signature = SIGNER_SIGNATURE_INFO {
            cbSize: size_of::<SIGNER_SIGNATURE_INFO>() as u32,
            algidHash: CALG_SHA_256,
            dwAttrChoice: SIGNER_NO_ATTR,
            ..Default::default()
        };
        let result = SignerSignEx(
            0,
            &subject,
            &signer,
            &signature,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
        );
        if result < 0 {
            return Err("native Authenticode signing failed".into());
        }
    }
    Ok(())
}

// Persistent acceptance runs only in the protected signing/package phase with signed inputs.
pub(crate) fn verifier_acceptance(
    utility: &Path,
    application: &Path,
    scratch: &Path,
) -> Result<()> {
    let invoke = |file: &Path, expected: Option<&str>| -> Result<()> {
        let output = crate::process::smoke(
            utility.to_str().ok_or("verifier path")?,
            &[file.to_str().ok_or("verification input path")?.to_owned()],
            scratch,
        )?;
        let value: Value = serde_json::from_slice(&output.stdout)?;
        if value["publicly_trusted"] != false {
            return Err("verifier must never imply public trust".into());
        }
        if let Some(expected) = expected {
            if output.status.success()
                || value["signature_valid"] != false
                || !value["error"]
                    .as_str()
                    .is_some_and(|error| error.contains(expected))
            {
                return Err(format!("verifier rejection contract failed: {expected}").into());
            }
        } else if !output.status.success()
            || value["signature_valid"] != true
            || value["trust"] != "self_signed_private"
            || value["file_sha256"] != crate::validate::digest(&fs::read(file)?)
        {
            return Err("signed verifier positive contract failed".into());
        }
        Ok(())
    };
    invoke(application, None)?;
    invoke(utility, None)?;
    let tampered = scratch.join("tampered.exe");
    let mut bytes = fs::read(application)?;
    let last_hashed = pe_signature::parse(&bytes)?.certificate_offset - 1;
    bytes[last_hashed] ^= 1;
    fs::write(&tampered, bytes)?;
    invoke(&tampered, Some("signed PE digest mismatch"))?;
    let system = std::env::var_os("SystemRoot").ok_or("SystemRoot missing")?;
    invoke(
        &Path::new(&system).join("System32/where.exe"),
        Some("certificate does not match owner pin"),
    )?;
    invoke(
        &scratch.join("missing.exe"),
        Some("verification input unavailable"),
    )?;
    let oversized = scratch.join("oversized.exe");
    let oversized_file = fs::File::create(&oversized)?;
    oversized_file.set_len(128 * 1024 * 1024 + 1)?;
    drop(oversized_file);
    invoke(&oversized, Some("expected bounded regular PE executable"))?;
    Ok(())
}

pub fn package(sha: &str) -> Result<()> {
    let target = std::env::var_os("CARGO_TARGET_DIR").ok_or("target directory missing")?;
    let release = Path::new(&target).join("release");
    let binary = release.join("fiverr-agent-mcp.exe");
    let utility = release.join("fiverr-verify.exe");
    let p = embedded_policy()?;
    eprintln!("signing phase=application_start");
    sign(&binary, &p)?;
    eprintln!("signing phase=utility_start");
    sign(&utility, &p)?;
    let mut receipt = verify(&binary, &p)?;
    receipt["verifier"] = verify(&utility, &p)?;
    eprintln!("signing phase=both_verified");
    let scratch = root().join(".ci-work/signing-negatives");
    fs::create_dir(&scratch)?;
    let result = (|| -> Result<()> {
        let file = scratch.join("malformed.exe");
        let mut bytes = fs::read(&binary)?;
        let offset = pe_signature::parse(&bytes)?.certificate_offset;
        bytes[offset + 4] = 0;
        bytes[offset + 5] = 0;
        fs::write(&file, &bytes)?;
        if verify(&file, &p).is_ok() {
            return Err("malformed certificate record accepted".into());
        }
        let mut wrong = p.clone();
        wrong["certificate_sha256"] = json!("0".repeat(64));
        if verify(&binary, &wrong).is_ok() {
            return Err("wrong signer accepted".into());
        }
        receipt["source_sha"] = json!(sha);
        receipt["negative_tests"] = json!([
            "cli_tampered_pe_rejected",
            "malformed_certificate_rejected",
            "wrong_pin_rejected",
            "cli_wrong_signer_rejected",
            "cli_missing_input_rejected",
            "cli_oversized_input_rejected"
        ]);
        Ok(())
    })();
    if result.is_ok() {
        fs::remove_dir_all(&scratch)?;
    }
    result?;
    // process::run strips signing credentials from every packaging child environment.
    eprintln!("signing phase=package_start");
    crate::package::build(
        &root(),
        binary.to_str().ok_or("binary path")?,
        root().join("dist").to_str().ok_or("dist path")?,
        "x86_64-pc-windows-msvc",
        sha,
    )?;
    // Package verification ran every CLI negative on the extracted signed utility.
    fs::write(
        root().join(".ci-work/windows-signing-receipt.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    fs::copy(
        root().join(".ci-work/windows-signing-receipt.json"),
        root().join("dist/windows-signing-receipt.json"),
    )?;
    Ok(())
}
