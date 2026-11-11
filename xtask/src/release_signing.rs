//! Strict SSHSIG parsing for release archives and SHA256SUMS.
//!
//! No approved Ed25519 verifier is available in the locked dependencies, so this
//! module fails closed with `unimplemented-verifier` after structural validation.
//! It never reports a signature as verified.
//!
//! SSHSIG contains no principal. The caller MUST select `allowed_public_key`
//! from a trusted, allowed_signers-style principal-to-key pin before calling this
//! function. A public-key comment is not a principal binding. This function only
//! rejects an empty principal; it cannot authenticate a principal from SSHSIG.
//! Keys must be single OpenSSH `ssh-ed25519 BASE64 [comment]` lines, without
//! authorized_keys options or certificates. Only the SHA-512 hash is accepted.

use base64::{Engine, prelude::BASE64_STANDARD};
use sha2::{Digest, Sha512};

const MAGIC: &[u8] = b"SSHSIG";
const BEGIN: &str = "-----BEGIN SSH SIGNATURE-----";
const END: &str = "-----END SSH SIGNATURE-----";
const MAX_TEXT_BYTES: usize = 16 * 1024;

/// Validate the SSHSIG envelope, selected key pin and namespace, then fail closed.
///
/// The caller owns principal authorization as documented at module level.
/// Cryptographic verification is unavailable, so even a structurally valid
/// signature returns an `unimplemented-verifier` error.
pub fn verify_sshsig(
    message: &[u8],
    signature_pem: &str,
    allowed_public_key: &str,
    principal: &str,
    namespace: &str,
) -> Result<(), String> {
    if principal.is_empty() {
        return Err("missing SSHSIG principal; caller must authorize the key pin".into());
    }
    if namespace.is_empty() {
        return Err("empty SSHSIG namespace".into());
    }
    let blob = decode_pem(signature_pem)?;
    let mut envelope = Reader(&blob);
    if envelope.take(MAGIC.len())? != MAGIC {
        return Err("invalid SSHSIG magic".into());
    }
    if envelope.u32()? != 1 {
        return Err("unsupported SSHSIG version; expected 1".into());
    }
    let signer_key = envelope.string()?;
    let signed_namespace = envelope.string()?;
    let reserved = envelope.string()?;
    let hash_algorithm = envelope.string()?;
    let signature_blob = envelope.string()?;
    envelope.finish("SSHSIG envelope")?;
    if signed_namespace != namespace.as_bytes() {
        return Err("SSHSIG namespace mismatch".into());
    }
    if hash_algorithm != b"sha512" {
        return Err("unsupported SSHSIG hash algorithm; expected sha512".into());
    }
    check_ed25519_key(signer_key)?;
    let allowed_key = decode_public_key(allowed_public_key)?;
    if signer_key != allowed_key.as_slice() {
        return Err("SSHSIG signer key does not match the allowed key blob".into());
    }
    let mut signature = Reader(signature_blob);
    if signature.string()? != b"ssh-ed25519" {
        return Err("unsupported SSHSIG signature algorithm; expected ssh-ed25519".into());
    }
    if signature.string()?.len() != 64 {
        return Err("invalid SSHSIG Ed25519 signature length".into());
    }
    signature.finish("SSHSIG signature")?;

    // SSHSIG signs these bytes, not the envelope, PEM, principal or raw message.
    // Retain reserved bytes verbatim: they are covered by the signature.
    let digest = Sha512::digest(message);
    let mut signed_data = MAGIC.to_vec();
    for field in [signed_namespace, reserved, hash_algorithm, &digest[..]] {
        push_string(&mut signed_data, field)?;
    }
    // Do not substitute parsing or hashing success for signature verification.
    Err("unimplemented-verifier: no approved Ed25519 verification backend is enabled".into())
}

fn decode_pem(pem: &str) -> Result<Vec<u8>, String> {
    if pem.len() > MAX_TEXT_BYTES {
        return Err("SSHSIG PEM exceeds the 16 KiB limit".into());
    }
    let mut lines = pem.lines();
    if lines.next() != Some(BEGIN) {
        return Err("invalid SSHSIG PEM header".into());
    }
    let mut encoded = String::new();
    for line in lines.by_ref() {
        if line == END {
            if lines.next().is_some() {
                return Err("trailing data after SSHSIG PEM footer".into());
            }
            return decode_base64(&encoded, "SSHSIG PEM");
        }
        if line.is_empty()
            || !line
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
        {
            return Err("invalid SSHSIG PEM body".into());
        }
        encoded.push_str(line);
    }
    Err("missing SSHSIG PEM footer".into())
}

fn decode_base64(encoded: &str, context: &str) -> Result<Vec<u8>, String> {
    if encoded.is_empty() {
        return Err(format!("empty {context} base64"));
    }
    let decoded = BASE64_STANDARD
        .decode(encoded)
        .map_err(|_| format!("invalid {context} base64"))?;
    if BASE64_STANDARD.encode(&decoded) != encoded {
        return Err(format!("noncanonical {context} base64"));
    }
    Ok(decoded)
}

fn decode_public_key(key: &str) -> Result<Vec<u8>, String> {
    let key = key
        .strip_suffix("\r\n")
        .or_else(|| key.strip_suffix('\n'))
        .unwrap_or(key);
    if key.len() > MAX_TEXT_BYTES || key.contains(['\r', '\n', '\0']) {
        return Err("allowed SSHSIG key must be one bounded OpenSSH key line".into());
    }
    let mut fields = key.split_ascii_whitespace();
    if fields.next() != Some("ssh-ed25519") {
        return Err("allowed SSHSIG key type must be ssh-ed25519".into());
    }
    let encoded = fields.next().ok_or("missing allowed SSHSIG public key")?;
    let blob = decode_base64(encoded, "allowed SSHSIG key")?;
    check_ed25519_key(&blob)?;
    Ok(blob)
}

fn check_ed25519_key(blob: &[u8]) -> Result<(), String> {
    let mut key = Reader(blob);
    if key.string()? != b"ssh-ed25519" {
        return Err("SSHSIG public key algorithm must be ssh-ed25519".into());
    }
    if key.string()?.len() != 32 {
        return Err("invalid SSHSIG Ed25519 public key length".into());
    }
    key.finish("SSHSIG public key")
}

fn push_string(output: &mut Vec<u8>, value: &[u8]) -> Result<(), String> {
    let len = u32::try_from(value.len()).map_err(|_| "SSHSIG field exceeds uint32 length")?;
    output.extend_from_slice(&len.to_be_bytes());
    output.extend_from_slice(value);
    Ok(())
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], String> {
        let bytes = self.0.get(..len).ok_or("truncated SSHSIG field")?;
        self.0 = &self.0[len..];
        Ok(bytes)
    }

    fn u32(&mut self) -> Result<u32, String> {
        let bytes: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| "truncated SSHSIG uint32")?;
        Ok(u32::from_be_bytes(bytes))
    }

    fn string(&mut self) -> Result<&'a [u8], String> {
        let len = usize::try_from(self.u32()?).map_err(|_| "SSHSIG field length overflow")?;
        self.take(len)
    }

    fn finish(self, context: &str) -> Result<(), String> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(format!("trailing bytes in {context}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAMESPACE: &str = "fiverr-agent-mcp-release";
    const PRINCIPAL: &str = "release-test-principal";

    fn key_blob(marker: u8) -> Vec<u8> {
        let mut key = Vec::new();
        push_string(&mut key, b"ssh-ed25519").unwrap();
        push_string(&mut key, &[marker; 32]).unwrap();
        key
    }

    fn public_key(marker: u8) -> String {
        format!(
            "ssh-ed25519 {} comment-is-not-a-principal",
            BASE64_STANDARD.encode(key_blob(marker))
        )
    }

    // Synthetic parser input only: these zero signature bytes are NOT a valid signature.
    fn envelope(hash: &[u8], signature_algorithm: &[u8]) -> Vec<u8> {
        let mut signature = Vec::new();
        push_string(&mut signature, signature_algorithm).unwrap();
        push_string(&mut signature, &[0; 64]).unwrap();
        let key = key_blob(0x11);
        let mut blob = MAGIC.to_vec();
        blob.extend_from_slice(&1_u32.to_be_bytes());
        for field in [
            key.as_slice(),
            NAMESPACE.as_bytes(),
            b"reserved".as_slice(),
            hash,
            signature.as_slice(),
        ] {
            push_string(&mut blob, field).unwrap();
        }
        blob
    }

    fn pem(blob: &[u8]) -> String {
        format!("{BEGIN}\n{}\n{END}\n", BASE64_STANDARD.encode(blob))
    }

    fn reject(blob: &[u8], expected: &str) {
        let error = verify_sshsig(
            b"archive",
            &pem(blob),
            &public_key(0x11),
            PRINCIPAL,
            NAMESPACE,
        )
        .unwrap_err();
        assert!(error.contains(expected), "unexpected error: {error}");
    }

    #[test]
    fn rejects_magic_and_version() {
        let mut blob = envelope(b"sha512", b"ssh-ed25519");
        blob[0] = b'X';
        reject(&blob, "magic");
        blob[0] = b'S';
        blob[9] = 2;
        reject(&blob, "version");
    }

    #[test]
    fn rejects_namespace_and_key_mismatch() {
        let armor = pem(&envelope(b"sha512", b"ssh-ed25519"));
        let error =
            verify_sshsig(b"archive", &armor, &public_key(0x11), PRINCIPAL, "other").unwrap_err();
        assert!(error.contains("namespace mismatch"));
        let error =
            verify_sshsig(b"archive", &armor, &public_key(0x22), PRINCIPAL, NAMESPACE).unwrap_err();
        assert!(error.contains("allowed key blob"));
    }

    #[test]
    fn rejects_unsupported_algorithms() {
        reject(&envelope(b"sha256", b"ssh-ed25519"), "hash algorithm");
        reject(&envelope(b"sha512", b"ssh-rsa"), "signature algorithm");
    }

    #[test]
    fn rejects_every_truncated_envelope_and_trailing_bytes() {
        let mut blob = envelope(b"sha512", b"ssh-ed25519");
        for len in 1..blob.len() {
            reject(&blob[..len], "truncated");
        }
        blob.push(0);
        reject(&blob, "trailing bytes");
        blob[10..14].copy_from_slice(&u32::MAX.to_be_bytes());
        reject(&blob, "truncated");
    }

    #[test]
    fn rejects_malformed_armor_and_base64() {
        for armor in [
            "",
            "-----BEGIN WRONG-----\nAAAA\n-----END WRONG-----\n",
            "-----BEGIN SSH SIGNATURE-----\n!!!!\n-----END SSH SIGNATURE-----\n",
            "-----BEGIN SSH SIGNATURE-----\nA===\n-----END SSH SIGNATURE-----\n",
            "-----BEGIN SSH SIGNATURE-----\nAB==\n-----END SSH SIGNATURE-----\n",
            "-----BEGIN SSH SIGNATURE-----\nAAAA\n",
            "-----BEGIN SSH SIGNATURE-----\nAAAA\n-----END SSH SIGNATURE-----\nextra\n",
        ] {
            let error =
                verify_sshsig(b"", armor, &public_key(0x11), PRINCIPAL, NAMESPACE).unwrap_err();
            assert!(!error.contains("unimplemented-verifier"));
        }
    }

    #[test]
    fn rejects_key_structure_and_signature_trailing_bytes() {
        let mut key = key_blob(0x11);
        key.push(0);
        assert!(
            check_ed25519_key(&key)
                .unwrap_err()
                .contains("trailing bytes")
        );
        assert!(check_ed25519_key(&key[..key.len() - 2]).is_err());
        let mut signature = Vec::new();
        push_string(&mut signature, b"ssh-ed25519").unwrap();
        push_string(&mut signature, &[0; 64]).unwrap();
        signature.push(0);
        let key = key_blob(0x11);
        let mut blob = MAGIC.to_vec();
        blob.extend_from_slice(&1_u32.to_be_bytes());
        for field in [
            key.as_slice(),
            NAMESPACE.as_bytes(),
            b"".as_slice(),
            b"sha512".as_slice(),
            signature.as_slice(),
        ] {
            push_string(&mut blob, field).unwrap();
        }
        reject(&blob, "trailing bytes in SSHSIG signature");
    }

    #[test]
    fn structurally_valid_input_never_succeeds_and_principal_is_caller_policy() {
        reject(
            &envelope(b"sha512", b"ssh-ed25519"),
            "unimplemented-verifier",
        );
        let error = verify_sshsig(
            b"archive",
            &pem(&envelope(b"sha512", b"ssh-ed25519")),
            &public_key(0x11),
            "",
            NAMESPACE,
        )
        .unwrap_err();
        assert!(error.contains("principal"));
    }
}
