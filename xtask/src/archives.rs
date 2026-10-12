use crate::{Result, validate};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Write},
    path::Path,
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

pub type Files = BTreeMap<String, Vec<u8>>;
const LIMIT: u64 = 256 * 1024 * 1024;

pub fn write(path: &Path, files: &Files) -> Result<()> {
    let file = File::options().write(true).create_new(true).open(path)?;
    if path.extension().is_some_and(|extension| extension == "zip") {
        let mut zip = ZipWriter::new(file);
        for (name, bytes) in files {
            validate::path(name)?;
            let options = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .last_modified_time(zip::DateTime::default())
                .unix_permissions(mode(name));
            zip.start_file(name, options)?;
            zip.write_all(bytes)?;
        }
        zip.finish()?.sync_all()?;
    } else {
        let mut tar = tar::Builder::new(GzEncoder::new(file, Compression::default()));
        for (name, bytes) in files {
            validate::path(name)?;
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(mode(name));
            header.set_mtime(0);
            header.set_cksum();
            tar.append_data(&mut header, name, bytes.as_slice())?;
        }
        tar.into_inner()?.finish()?.sync_all()?;
    }
    Ok(())
}

pub fn read(path: &Path) -> Result<Files> {
    let mut files = Files::new();
    let mut total = 0_u64;
    if path.extension().is_some_and(|extension| extension == "zip") {
        let mut zip = ZipArchive::new(File::open(path)?)?;
        if zip.len() > 4096 {
            return Err("archive entry ceiling exceeded".into());
        }
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index)?;
            let name = entry.name()?.into_owned();
            validate::path(&name)?;
            if !entry.is_file() || entry.is_symlink() {
                return Err("archive special entry refused".into());
            }
            if (name.starts_with("bin/") || name.ends_with(".sh"))
                && entry.unix_mode().is_none_or(|m| m & 0o111 == 0)
            {
                return Err("archive executable mode missing".into());
            }
            let size = entry.size();
            total = total
                .checked_add(size)
                .ok_or("expanded archive size overflow")?;
            if total > LIMIT {
                return Err("expanded archive ceiling exceeded".into());
            }
            let mut bytes = Vec::new();
            (&mut entry).take(size + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 != size || files.insert(name, bytes).is_some() {
                return Err("invalid or duplicate archive entry".into());
            }
        }
    } else {
        let mut archive = tar::Archive::new(GzDecoder::new(File::open(path)?));
        for item in archive.entries()? {
            let mut entry = item?;
            let name = entry
                .path()?
                .to_str()
                .ok_or("non-UTF8 archive path")?
                .to_owned();
            validate::path(&name)?;
            if !entry.header().entry_type().is_file() {
                return Err("archive special entry refused".into());
            }
            if (name.starts_with("bin/") || name.ends_with(".sh"))
                && entry.header().mode()? & 0o111 == 0
            {
                return Err("archive executable mode missing".into());
            }
            let size = entry.size();
            total = total
                .checked_add(size)
                .ok_or("expanded archive size overflow")?;
            if total > LIMIT || files.len() >= 4096 {
                return Err("expanded archive ceiling exceeded".into());
            }
            let mut bytes = Vec::new();
            (&mut entry).take(size + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 != size || files.insert(name, bytes).is_some() {
                return Err("invalid or duplicate archive entry".into());
            }
        }
    }
    if files.is_empty() {
        return Err("empty archive".into());
    }
    Ok(files)
}

pub fn extract(path: &Path, files: &Files) -> Result<()> {
    std::fs::create_dir(path)?;
    for (name, bytes) in files {
        validate::path(name)?;
        let destination = path.join(name);
        std::fs::create_dir_all(destination.parent().ok_or("missing archive parent")?)?;
        let mut file = File::options()
            .create_new(true)
            .write(true)
            .open(&destination)?;
        file.write_all(bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(destination, std::fs::Permissions::from_mode(mode(name)))?;
        }
    }
    Ok(())
}

fn mode(name: &str) -> u32 {
    if name.starts_with("bin/") || name.ends_with(".sh") {
        0o755
    } else {
        0o644
    }
}
