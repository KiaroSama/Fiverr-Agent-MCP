//! Read-only pinned CMS/PE verification. No signing or private-store access.
#![allow(unsafe_code)]
use crate::{Result, pe_signature};
use base64::{Engine, prelude::BASE64_STANDARD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{ffi::CStr, fs::File, io::Read, mem::size_of, path::Path};
use windows_sys::Win32::Security::Cryptography::*;

pub struct Certificate(pub *const CERT_CONTEXT);
impl Drop for Certificate {
    fn drop(&mut self) {
        unsafe {
            CertFreeCertificateContext(self.0);
        }
    }
}
pub fn check(value: i32) -> Result<()> {
    if value == 0 {
        Err("native cryptographic operation failed".into())
    } else {
        Ok(())
    }
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
pub fn embedded_policy() -> Result<Value> {
    let policy: Value = serde_json::from_str(include_str!("../../config/signing-public.json"))?;
    validate_policy(&policy)?;
    Ok(policy)
}
fn validate_policy(policy: &Value) -> Result<()> {
    if policy["schema_version"] != 1
        || policy["trust"] != "self_signed_private"
        || policy["certificate_sha256"]
            .as_str()
            .is_none_or(|pin| pin.len() != 64)
        || !policy["certificate_der_base64"].is_string()
    {
        return Err("unsupported signing policy".into());
    }
    Ok(())
}
/// # Safety
/// `cert` must reference a live Windows certificate context throughout this call.
pub unsafe fn certificate_matches(cert: *const CERT_CONTEXT, policy: &Value) -> Result<()> {
    validate_policy(policy)?;
    if cert.is_null() {
        return Err("signed certificate missing".into());
    }
    // SAFETY: caller owns cert; Windows supplies its encoded extent and usage data.
    unsafe {
        let bytes =
            std::slice::from_raw_parts((*cert).pbCertEncoded, (*cert).cbCertEncoded as usize);
        if policy["certificate_sha256"] != digest(bytes)
            || policy["certificate_der_base64"] != BASE64_STANDARD.encode(bytes)
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
pub fn verify(file: &Path, policy: &Value) -> Result<Value> {
    validate_policy(policy)?;
    const LIMIT: u64 = 128 * 1024 * 1024;
    let metadata = std::fs::metadata(file).map_err(|_| "verification input unavailable")?;
    if !metadata.is_file() || metadata.len() > LIMIT {
        return Err("expected bounded regular PE executable".into());
    }
    let input = File::open(file).map_err(|_| "verification input unavailable")?;
    let metadata = input
        .metadata()
        .map_err(|_| "verification input unavailable")?;
    if !metadata.is_file() || metadata.len() > LIMIT {
        return Err("expected bounded regular PE executable".into());
    }
    let mut bytes = Vec::new();
    input
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "verification input unreadable")?;
    if bytes.len() as u64 > LIMIT {
        return Err("expected bounded regular PE executable".into());
    }
    let pe = pe_signature::parse(&bytes)?;
    // SAFETY: bounded buffers remain alive; returned certificate uses RAII ownership.
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
            let mut count = 0u32;
            let mut count_size = size_of::<u32>() as u32;
            check(CryptMsgGetParam(
                message,
                CMSG_SIGNER_COUNT_PARAM,
                0,
                &mut count as *mut _ as _,
                &mut count_size,
            ))?;
            if count != 1 || count_size != size_of::<u32>() as u32 {
                return Err("expected exactly one Authenticode signer".into());
            }
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
        certificate_matches(cert.0, policy)?;
        content.truncate(size as usize);
        if pe_signature::content_digest(&content)? != pe.digest {
            return Err("signed PE digest mismatch".into());
        }
    }
    Ok(
        json!({"file_sha256":digest(&bytes),"certificate_sha256":policy["certificate_sha256"],"signature_valid":true,"trust":"self_signed_private","publicly_trusted":false}),
    )
}
