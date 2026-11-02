use crate::Result;
use sha2::{Digest, Sha256};

fn u16_at(bytes: &[u8], offset: usize) -> Result<usize> {
    Ok(u16::from_le_bytes(
        bytes
            .get(offset..offset + 2)
            .ok_or("PE integer outside file")?
            .try_into()?,
    ) as usize)
}
fn u32_at(bytes: &[u8], offset: usize) -> Result<usize> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset + 4)
            .ok_or("PE integer outside file")?
            .try_into()?,
    ) as usize)
}
pub struct Pe<'a> {
    pub cms: &'a [u8],
    pub digest: Vec<u8>,
    pub certificate_offset: usize,
}
pub fn parse(bytes: &[u8]) -> Result<Pe<'_>> {
    if bytes.len() < 512 || bytes.len() > 128 * 1024 * 1024 || bytes.get(..2) != Some(b"MZ") {
        return Err("expected bounded PE executable".into());
    }
    let pe = u32_at(bytes, 60)?;
    if pe < 64 || bytes.get(pe..pe + 4) != Some(b"PE\0\0") {
        return Err("invalid PE header".into());
    }
    let optional = pe + 24;
    let directory = optional
        + match u16_at(bytes, optional)? {
            0x20b => 112,
            0x10b => 96,
            _ => return Err("unsupported PE header".into()),
        };
    let security = directory + 32;
    let checksum = optional + 64;
    let headers = u32_at(bytes, optional + 60)?;
    let offset = u32_at(bytes, security)?;
    let size = u32_at(bytes, security + 4)?;
    if offset < headers
        || size < 8
        || offset.checked_add(size) != Some(bytes.len())
        || security + 8 > headers
        || headers > bytes.len()
    {
        return Err("missing or nonterminal certificate table".into());
    }
    let length = u32_at(bytes, offset)?;
    if length < 8
        || length > size
        || u16_at(bytes, offset + 4)? != 0x200
        || u16_at(bytes, offset + 6)? != 2
        || (length + 7) & !7 != size
    {
        return Err("invalid Authenticode record".into());
    }
    let mut hash = Sha256::new();
    hash.update(bytes.get(..checksum).ok_or("checksum bounds")?);
    hash.update(bytes.get(checksum + 4..security).ok_or("security bounds")?);
    hash.update(bytes.get(security + 8..headers).ok_or("header bounds")?);
    let table = optional + u16_at(bytes, pe + 20)?;
    let mut ranges = Vec::new();
    for i in 0..u16_at(bytes, pe + 6)? {
        let entry = table + 40 * i;
        if entry + 40 > headers {
            return Err("section header outside headers".into());
        }
        ranges.push((u32_at(bytes, entry + 20)?, u32_at(bytes, entry + 16)?));
    }
    ranges.sort_unstable();
    let mut end = headers;
    for (start, size) in ranges {
        if size == 0 {
            continue;
        }
        let next = start.checked_add(size).ok_or("section overflow")?;
        if start != end || next > offset {
            return Err("overlapping or gapped PE sections".into());
        }
        hash.update(&bytes[start..next]);
        end = next;
    }
    hash.update(&bytes[end..offset]);
    Ok(Pe {
        cms: &bytes[offset + 8..offset + length],
        digest: hash.finalize().to_vec(),
        certificate_offset: offset,
    })
}

fn value<'a>(bytes: &mut &'a [u8], tag: u8) -> Result<&'a [u8]> {
    if bytes.len() < 2 || bytes[0] != tag {
        return Err("unexpected DER tag".into());
    }
    let first = bytes[1];
    let (header, len) = if first < 128 {
        (2, usize::from(first))
    } else {
        let count = usize::from(first & 127);
        if count == 0 || count > 4 || bytes.len() < 2 + count || bytes[2] == 0 {
            return Err("invalid DER length".into());
        }
        let mut len = 0usize;
        for b in &bytes[2..2 + count] {
            len = (len << 8) | usize::from(*b);
        }
        if len < 128 {
            return Err("nonminimal DER length".into());
        }
        (2 + count, len)
    };
    let end = header.checked_add(len).ok_or("DER overflow")?;
    let result = bytes.get(header..end).ok_or("truncated DER")?;
    *bytes = &bytes[end..];
    Ok(result)
}
pub fn content_digest(mut content: &[u8]) -> Result<Vec<u8>> {
    let mut sequence = value(&mut content, 0x30)?;
    if !content.is_empty() {
        return Err("trailing signed content".into());
    }
    let mut data = value(&mut sequence, 0x30)?;
    if value(&mut data, 0x06)? != [0x2b, 6, 1, 4, 1, 0x82, 0x37, 2, 1, 0x0f] {
        return Err("not PE indirect data".into());
    }
    let mut digest = value(&mut sequence, 0x30)?;
    if !sequence.is_empty() {
        return Err("trailing indirect data".into());
    }
    let mut algorithm = value(&mut digest, 0x30)?;
    if value(&mut algorithm, 0x06)? != [0x60, 0x86, 0x48, 1, 0x65, 3, 4, 2, 1] {
        return Err("only SHA256 Authenticode permitted".into());
    }
    if !algorithm.is_empty() && !value(&mut algorithm, 0x05)?.is_empty() {
        return Err("invalid digest parameters".into());
    }
    let result = value(&mut digest, 0x04)?;
    if !algorithm.is_empty() || !digest.is_empty() || result.len() != 32 {
        return Err("invalid signed digest".into());
    }
    Ok(result.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_pe_and_der_are_rejected_without_panics() {
        for bytes in [vec![], vec![0; 512], b"MZ".to_vec(), vec![255; 1024]] {
            assert!(parse(&bytes).is_err());
        }
        for bytes in [
            &b""[..],
            &b"\x30\x80"[..],
            &b"\x30\x84\xff\xff\xff\xff"[..],
            &b"\x30\x03\x30\x01\0"[..],
        ] {
            assert!(content_digest(bytes).is_err());
        }
    }
}
