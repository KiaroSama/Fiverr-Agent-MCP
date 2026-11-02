use std::{env, fs, io::Write, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env::var_os("GITHUB_WORKSPACE").ok_or("GITHUB_WORKSPACE missing")?)
        .canonicalize()?;
    let mut output = fs::OpenOptions::new()
        .append(true)
        .open(env::var_os("GITHUB_ENV").ok_or("GITHUB_ENV missing")?)?;
    for (name, relative) in [
        ("CARGO_HOME", ".ci-cache/cargo"),
        ("RUSTUP_HOME", ".ci-cache/rustup"),
        ("CARGO_TARGET_DIR", "target"),
        ("TEMP", ".ci-work/tmp"),
        ("TMP", ".ci-work/tmp"),
        ("TMPDIR", ".ci-work/tmp"),
        ("XDG_CACHE_HOME", ".ci-cache/xdg"),
        ("HOME", ".ci-work/home"),
    ] {
        let path = root.join(relative);
        fs::create_dir_all(&path)?;
        let meta = fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            return Err("linked CI storage refused".into());
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if meta.file_attributes() & 0x400 != 0 {
                return Err("reparse CI storage refused".into());
            }
        }
        if !path.canonicalize()?.starts_with(&root) {
            return Err("CI storage escaped workspace".into());
        }
        let text = path.to_str().ok_or("non-UTF8 CI storage path")?;
        let text = text.strip_prefix(r"\\?\").unwrap_or(text);
        writeln!(output, "{name}={text}")?;
    }
    writeln!(output, "CARGO_BUILD_JOBS=2\nHOOKMAKER_MAX_TEST_WORKERS=2")?;
    output.flush()?;
    Ok(())
}
