//! Strict SSHSIG parsing and Ed25519 verification for release archives and SHA256SUMS.
//!
//! Verification is delegated to the pinned `ssh-key` crate after this module's own
//! structural validation, so a structurally valid but cryptographically invalid
//! signature is still rejected. Nothing here reports a signature as verified on the
//! strength of parsing alone.
//!
//! SSHSIG contains no principal. The caller MUST select `allowed_public_key`
//! from a trusted, allowed_signers-style principal-to-key pin before calling this
//! function. A public-key comment is not a principal binding. This function only
//! rejects an empty principal; it cannot authenticate a principal from SSHSIG.
//! Keys must be single OpenSSH `ssh-ed25519 BASE64 [comment]` lines, without
//! authorized_keys options or certificates. Only the SHA-512 hash is accepted.

use base64::{Engine, prelude::BASE64_STANDARD};

const MAGIC: &[u8] = b"SSHSIG";
const BEGIN: &str = "-----BEGIN SSH SIGNATURE-----";
const END: &str = "-----END SSH SIGNATURE-----";
const MAX_TEXT_BYTES: usize = 16 * 1024;

/// Validate the SSHSIG envelope, selected key pin and namespace, then fail closed.
///
/// The caller owns principal authorization as documented at module level.
/// Cryptographic verification uses the pinned `ssh-key` Ed25519 implementation;
/// every rejection path fails closed.
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
    // Present in the envelope and covered by the signature; `ssh-key` re-reads it
    // from the same PEM, so this module only needs to prove the field is well formed.
    let _reserved = envelope.string()?;
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

    // The signature covers `namespace || reserved || hash_alg || H(msg)` re-encoded
    // with length prefixes, not the PEM, the envelope or the raw message. `ssh-key`
    // rebuilds exactly those bytes from the parsed signature, so `reserved` must
    // reach it verbatim rather than being dropped here.
    let signature =
        ssh_key::SshSig::from_pem(signature_pem).map_err(|_| "unparseable SSHSIG signature")?;
    let key = ssh_key::PublicKey::from_openssh(allowed_public_key)
        .map_err(|_| "unparseable allowed SSHSIG public key")?;
    key.verify(namespace, message, &signature).map_err(|error| {
        use ssh_key::Error;
        match error {
            Error::PublicKey => "SSHSIG signer key does not match the allowed key blob",
            Error::Namespace => "SSHSIG namespace mismatch",
            Error::Version { .. } => "unsupported SSHSIG version; expected 1",
            _ => "SSHSIG signature verification failed",
        }
        .to_owned()
    })
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

// Retained for the tests, which build and re-shape envelopes byte by byte. The
// verification path no longer re-encodes the signed data itself.
#[cfg(test)]
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

    // Archive-level negatives for duplicate and traversal entries live where that
    // logic lives: `archive_tests.rs` covers duplicate, traversal, special-mode and
    // truncation; `release_tests.rs` covers an unexpected, duplicate, missing, extra
    // or unsafe checksum manifest entry. This module only owns the signature itself.
    //
    // A real keypair lets every assertion here rest on real Ed25519 arithmetic rather
    // than on a synthetic envelope that could only ever reach the parser.
    fn keypair(seed: u8) -> ssh_key::PrivateKey {
        ssh_key::PrivateKey::new(
            ssh_key::private::KeypairData::Ed25519(
                ssh_key::private::Ed25519Keypair::from_seed(&[seed; 32]),
            ),
            "test key",
        )
        .unwrap()
    }

    fn openssh(key: &ssh_key::PrivateKey) -> String {
        key.public_key().to_openssh().unwrap()
    }

    fn pem_for(seed: u8, namespace: &str, message: &[u8]) -> String {
        keypair(seed)
            .sign(namespace, ssh_key::HashAlg::Sha512, message)
            .unwrap()
            .to_pem(ssh_key::LineEnding::LF)
            .unwrap()
    }

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

    // A well-formed envelope whose signature bytes are still zeros. Every test using
    // this stops before verification, so it never stands in for a real signature.
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
        // Wrap at 70 columns — the same PEM_LINE_WIDTH the `ssh-encoding` crate
        // uses when encoding, so `ssh-key::SshSig::from_pem` can round-trip the
        // bytes we give it without a decode error.
        let b64 = BASE64_STANDARD.encode(blob);
        let wrapped: String = b64
            .as_bytes()
            .chunks(70)
            .map(|chunk| std::str::from_utf8(chunk).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        format!("{BEGIN}\n{wrapped}\n{END}\n")
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
            assert!(!error.is_empty());
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
    fn real_signature_is_accepted_and_principal_remains_caller_policy() {
        let key = keypair(0x11);
        let armor = pem_for(0x11, NAMESPACE, b"archive");
        verify_sshsig(b"archive", &armor, &openssh(&key), PRINCIPAL, NAMESPACE).unwrap();

        // SSHSIG carries no principal, so this function cannot authenticate one. It
        // rejects the empty principal and leaves the principal-to-key pin to the caller.
        let error = verify_sshsig(b"archive", &armor, &openssh(&key), "", NAMESPACE).unwrap_err();
        assert!(error.contains("principal"));
    }

    #[test]
    fn rejects_tampered_message_signature_and_wrong_key_bytes() {
        let key = keypair(0x11);
        let armor = pem_for(0x11, NAMESPACE, b"archive");
        let public = openssh(&key);

        let error = verify_sshsig(b"archivf", &armor, &public, PRINCIPAL, NAMESPACE).unwrap_err();
        assert_eq!(error, "SSHSIG signature verification failed");

        // Flip one bit of the real Ed25519 signature's R half, not the surrounding
        // fields. R has no canonical-encoding constraint, so the envelope still parses
        // and the rejection has to come from the verifier rather than from decoding.
        // (Flipping an S byte instead can fail canonicality and surface as a parse error.)
        let mut blob = decode_pem(&armor).unwrap();
        let flipped = blob.len() - 64;
        blob[flipped] ^= 1;
        let error =
            verify_sshsig(b"archive", &pem(&blob), &public, PRINCIPAL, NAMESPACE).unwrap_err();
        assert_eq!(error, "SSHSIG signature verification failed");

        // A different real key with the same namespace is still the wrong signer.
        let error = verify_sshsig(
            b"archive",
            &armor,
            &openssh(&keypair(0x22)),
            PRINCIPAL,
            NAMESPACE,
        )
        .unwrap_err();
        assert_eq!(error, "SSHSIG signer key does not match the allowed key blob");
    }

    #[test]
    fn rejects_a_real_signature_minted_under_another_namespace() {
        let key = keypair(0x11);
        let armor = pem_for(0x11, "fiverr-agent-mcp-release-wrong-namespace", b"archive");
        let error = verify_sshsig(b"archive", &armor, &openssh(&key), PRINCIPAL, NAMESPACE)
            .unwrap_err();
        assert_eq!(error, "SSHSIG namespace mismatch");
    }

    #[test]
    fn rejects_legacy_version_and_cross_namespace_after_a_real_sign() {
        // Version 2 is what `ssh-key` treats as a future/legacy envelope it must refuse.
        let key = keypair(0x11);
        let mut blob = decode_pem(&pem_for(0x11, NAMESPACE, b"archive")).unwrap();
        blob[..6].copy_from_slice(MAGIC);
        blob[6..10].copy_from_slice(&2_u32.to_be_bytes());
        let error = verify_sshsig(b"archive", &pem(&blob), &openssh(&key), PRINCIPAL, NAMESPACE)
            .unwrap_err();
        assert_eq!(error, "unsupported SSHSIG version; expected 1");
    }
}
