//! Windows cryptographic adapter. Private key material never reaches child argv or files.
#![allow(unsafe_code)]
use crate::{Result, pe_signature, root, validate};
use base64::{Engine, prelude::BASE64_STANDARD};
use serde_json::{Value, json};
use std::{ffi::CStr, fs, mem::size_of, os::windows::ffi::OsStrExt, path::Path};
use windows_sys::Win32::Security::Cryptography::*;
use zeroize::Zeroizing;

struct Store(HCERTSTORE);
impl Drop for Store {
    fn drop(&mut self) {
        unsafe {
            CertCloseStore(self.0, 0);
        }
    }
}
struct Certificate(*const CERT_CONTEXT);
impl Drop for Certificate {
    fn drop(&mut self) {
        unsafe {
            CertFreeCertificateContext(self.0);
        }
    }
}
fn check(value: i32) -> Result<()> {
    if value == 0 {
        Err("native cryptographic operation failed".into())
    } else {
        Ok(())
    }
}
fn policy() -> Result<Value> {
    let p: Value =
        serde_json::from_slice(&validate::regular(&root(), "config/signing-public.json")?)?;
    if p["schema_version"] != 1 || p["trust"] != "self_signed_private" {
        return Err("unsupported signing policy".into());
    }
    Ok(p)
}
unsafe fn certificate_matches(cert: *const CERT_CONTEXT, p: &Value) -> Result<()> {
    // SAFETY: caller owns cert for this entire call; Windows supplies its encoded extent.
    unsafe {
        let bytes =
            std::slice::from_raw_parts((*cert).pbCertEncoded, (*cert).cbCertEncoded as usize);
        if p["certificate_sha256"] != validate::digest(bytes)
            || p["certificate_der_base64"] != BASE64_STANDARD.encode(bytes)
        {
            return Err("certificate does not match owner pin".into());
        }
        if CertVerifyTimeValidity(std::ptr::null(), (*cert).pCertInfo) != 0 {
            return Err("signing certificate expired or not yet valid".into());
        }
        let mut size = 0;
        check(CertGetEnhancedKeyUsage(
            cert,
            0,
            std::ptr::null_mut(),
            &mut size,
        ))?;
        if (size as usize) < size_of::<CTL_USAGE>() || size > 65536 {
            return Err("invalid certificate usage size".into());
        }
        let mut storage = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
        let usage = storage.as_mut_ptr() as *mut CTL_USAGE;
        check(CertGetEnhancedKeyUsage(cert, 0, usage, &mut size))?;
        let usages = std::slice::from_raw_parts(
            (*usage).rgpszUsageIdentifier,
            (*usage).cUsageIdentifier as usize,
        );
        if !usages
            .iter()
            .any(|oid| CStr::from_ptr(*oid as _).to_bytes() == b"1.3.6.1.5.5.7.3.3")
        {
            return Err("certificate lacks code signing EKU".into());
        }
    }
    Ok(())
}
pub fn verify(file: &Path, p: &Value) -> Result<Value> {
    let bytes = fs::read(file)?;
    let pe = pe_signature::parse(&bytes)?;
    // SAFETY: bounded input/output buffers remain alive; returned context has RAII ownership.
    unsafe {
        let message = CryptMsgOpenToDecode(
            X509_ASN_ENCODING | PKCS_7_ASN_ENCODING,
            0,
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
        );
        if message.is_null() {
            return Err("CMS message decode failed".into());
        }
        let typed = (|| -> Result<()> {
            check(CryptMsgUpdate(
                message,
                pe.cms.as_ptr(),
                pe.cms.len() as u32,
                1,
            ))?;
            let mut oid = [0u8; 128];
            let mut length = oid.len() as u32;
            check(CryptMsgGetParam(
                message,
                CMSG_INNER_CONTENT_TYPE_PARAM,
                0,
                oid.as_mut_ptr() as _,
                &mut length,
            ))?;
            if length == 0
                || length as usize > oid.len()
                || &oid[..length as usize] != b"1.3.6.1.4.1.311.2.1.4\0"
            {
                return Err("CMS content type is not Authenticode".into());
            }
            Ok(())
        })();
        CryptMsgClose(message);
        typed?;
        let params = CRYPT_VERIFY_MESSAGE_PARA {
            cbSize: size_of::<CRYPT_VERIFY_MESSAGE_PARA>() as u32,
            dwMsgAndCertEncodingType: X509_ASN_ENCODING | PKCS_7_ASN_ENCODING,
            ..Default::default()
        };
        let mut size = 0;
        check(CryptVerifyMessageSignature(
            &params,
            0,
            pe.cms.as_ptr(),
            pe.cms.len() as u32,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
        ))?;
        if size == 0 || size > 1024 * 1024 {
            return Err("signed content missing or oversized".into());
        }
        let mut content = vec![0u8; size as usize];
        let mut cert = std::ptr::null_mut();
        check(CryptVerifyMessageSignature(
            &params,
            0,
            pe.cms.as_ptr(),
            pe.cms.len() as u32,
            content.as_mut_ptr(),
            &mut size,
            &mut cert,
        ))?;
        let cert = Certificate(cert);
        if cert.0.is_null() {
            return Err("signed certificate missing".into());
        }
        certificate_matches(cert.0, p)?;
        content.truncate(size as usize);
        if pe_signature::content_digest(&content)? != pe.digest {
            return Err("signed PE digest mismatch".into());
        }
        let mut second_size = 0;
        if CryptVerifyMessageSignature(
            &params,
            1,
            pe.cms.as_ptr(),
            pe.cms.len() as u32,
            std::ptr::null_mut(),
            &mut second_size,
            std::ptr::null_mut(),
        ) != 0
        {
            return Err("multiple signers refused".into());
        }
    }
    Ok(
        json!({"file_sha256":validate::digest(&bytes),"certificate_sha256":p["certificate_sha256"],"signature_valid":true,"trust":"self_signed_private","publicly_trusted":false}),
    )
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
pub fn package(sha: &str) -> Result<()> {
    let target = std::env::var_os("CARGO_TARGET_DIR").ok_or("target directory missing")?;
    let binary = Path::new(&target).join("release/fiverr-agent-mcp.exe");
    let p = policy()?;
    sign(&binary, &p)?;
    let mut receipt = verify(&binary, &p)?;
    let scratch = root().join(".ci-work/signing-negatives");
    fs::create_dir(&scratch)?;
    let result = (|| -> Result<()> {
        let file = scratch.join("tampered.exe");
        let mut bytes = fs::read(&binary)?;
        bytes[512] ^= 1;
        fs::write(&file, &bytes)?;
        if verify(&file, &p).is_ok() {
            return Err("tampered PE accepted".into());
        }
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
            "tampered_pe_rejected",
            "malformed_certificate_rejected",
            "wrong_signer_rejected"
        ]);
        fs::write(
            root().join(".ci-work/windows-signing-receipt.json"),
            serde_json::to_vec_pretty(&receipt)?,
        )?;
        Ok(())
    })();
    fs::remove_dir_all(&scratch)?;
    result?;
    // process::run strips signing credentials from every packaging child environment.
    crate::package::build(
        &root(),
        binary.to_str().ok_or("binary path")?,
        root().join("dist").to_str().ok_or("dist path")?,
        "x86_64-pc-windows-msvc",
        sha,
    )?;
    let archive = root().join("dist/fiverr-agent-mcp-0.1.0-x86_64-pc-windows-msvc.zip");
    let files = crate::archives::read(&archive)?;
    let check = root().join(".ci-work/signed-package.exe");
    fs::write(
        &check,
        files
            .get("bin/fiverr-agent-mcp.exe")
            .ok_or("package executable missing")?,
    )?;
    let verified = verify(&check, &p);
    fs::remove_file(&check)?;
    verified?;
    fs::copy(
        root().join(".ci-work/windows-signing-receipt.json"),
        root().join("dist/windows-signing-receipt.json"),
    )?;
    Ok(())
}
