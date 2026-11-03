use crate::archives::{self, Files};
use std::{fs, io::Write, path::PathBuf};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = crate::root().join(".ci-work").join(format!(
            "archive-corpus-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn archive_reader_accepts_valid_zip_and_tar() {
    let scratch = Scratch::new();
    let files = Files::from([
        ("bin/tool".into(), b"dummy executable".to_vec()),
        ("docs/guide.md".into(), b"Guide".to_vec()),
    ]);
    for name in ["valid.zip", "valid.tar.gz"] {
        let path = scratch.0.join(name);
        archives::write(&path, &files).unwrap();
        let read = archives::read(&path).unwrap();
        assert_eq!(read.len(), 2);
        assert_eq!(read["bin/tool"], b"dummy executable");
        assert_eq!(read["docs/guide.md"], b"Guide");
    }
}

#[test]
fn archive_reader_enforces_small_exact_boundaries() {
    assert_eq!(super::LIMIT, 256 * 1024 * 1024);
    assert_eq!(
        super::expanded_total(u64::MAX - 1, 1, u64::MAX).unwrap(),
        u64::MAX
    );
    assert!(super::expanded_total(u64::MAX, 1, u64::MAX).is_err());
    let scratch = Scratch::new();
    let files = Files::from([
        ("one.txt".into(), b"12".to_vec()),
        ("two.txt".into(), b"34".to_vec()),
    ]);
    for name in ["bounds.zip", "bounds.tar.gz"] {
        let path = scratch.0.join(name);
        archives::write(&path, &files).unwrap();
        assert_eq!(super::read_with_limits(&path, 2, 4).unwrap(), files);
        assert!(super::read_with_limits(&path, 1, 4).is_err());
        assert!(super::read_with_limits(&path, 2, 3).is_err());
    }
}

#[test]
fn archive_reader_rejects_non_utf8_tar_name_and_short_member() {
    let scratch = Scratch::new();
    for kind in ["non_utf8", "short_member"] {
        let path = scratch.0.join(format!("{kind}.tar.gz"));
        let mut header = tar::Header::new_gnu();
        header.as_mut_bytes()[0] = if kind == "non_utf8" { 0xff } else { b'a' };
        header.set_size(if kind == "short_member" { 1024 } else { 1 });
        header.set_mode(0o644);
        header.set_cksum();
        let mut encoder = flate2::write::GzEncoder::new(
            fs::File::create(&path).unwrap(),
            flate2::Compression::default(),
        );
        encoder.write_all(header.as_bytes()).unwrap();
        encoder.write_all(b"x").unwrap();
        if kind == "non_utf8" {
            encoder.write_all(&vec![0; 511 + 1024]).unwrap();
        }
        encoder.finish().unwrap();
        assert!(archives::read(&path).is_err(), "accepted {kind}");
    }
}

#[test]
fn archive_reader_rejects_unsafe_zip_members_without_extracting() {
    let scratch = Scratch::new();
    for (index, kind) in [
        "traversal",
        "directory",
        "symlink",
        "mode",
        "empty",
        "truncated",
    ]
    .into_iter()
    .enumerate()
    {
        let path = scratch.0.join(format!("case-{index}.zip"));
        let mut writer = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .system(zip::System::Unix)
            .unix_permissions(0o644);
        match kind {
            "directory" => writer.add_directory("docs/", options).unwrap(),
            "symlink" => writer.add_symlink("link", "outside", options).unwrap(),
            "empty" => {}
            _ => {
                let name = match kind {
                    "traversal" => "../outside",
                    "mode" => "bin/tool",
                    _ => "data.txt",
                };
                writer.start_file(name, options).unwrap();
                writer.write_all(b"bounded fixture").unwrap();
            }
        }
        writer.finish().unwrap();
        if kind == "truncated" {
            let mut bytes = fs::read(&path).unwrap();
            bytes.truncate(bytes.len() / 2);
            fs::write(&path, bytes).unwrap();
        }
        assert!(archives::read(&path).is_err(), "{kind}");
    }
    assert!(!scratch.0.parent().unwrap().join("outside").exists());
}

#[test]
fn archive_reader_rejects_tar_duplicate_special_mode_and_truncation() {
    let scratch = Scratch::new();
    for (index, kind) in [
        "duplicate",
        "symlink",
        "directory",
        "mode",
        "truncated",
        "empty",
    ]
    .into_iter()
    .enumerate()
    {
        let path = scratch.0.join(format!("case-{index}.tar.gz"));
        let encoder = flate2::write::GzEncoder::new(
            fs::File::create(&path).unwrap(),
            flate2::Compression::default(),
        );
        let mut builder = tar::Builder::new(encoder);
        if kind != "empty" {
            let mut header = tar::Header::new_gnu();
            header.set_size(4);
            header.set_mode(0o644);
            if kind == "symlink" {
                header.set_entry_type(tar::EntryType::Symlink);
                header.set_size(0);
                header.set_link_name("outside").unwrap();
            }
            if kind == "directory" {
                header.set_entry_type(tar::EntryType::Directory);
                header.set_size(0);
            }
            header.set_cksum();
            let name = if kind == "mode" {
                "bin/tool"
            } else {
                "data.txt"
            };
            let data = if ["symlink", "directory"].contains(&kind) {
                &b""[..]
            } else {
                &b"data"[..]
            };
            builder.append_data(&mut header, name, data).unwrap();
            if kind == "duplicate" {
                builder.append_data(&mut header, name, data).unwrap();
            }
        }
        builder.into_inner().unwrap().finish().unwrap();
        if kind == "truncated" {
            let bytes = fs::read(&path).unwrap();
            fs::write(&path, &bytes[..bytes.len() / 2]).unwrap();
        }
        assert!(archives::read(&path).is_err(), "{kind}");
    }
}
