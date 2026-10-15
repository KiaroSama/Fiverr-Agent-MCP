use crate::core::{ErrorCode, Result, error, storage_error};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct Root(PathBuf);
impl Root {
    pub fn explicit(path: &Path) -> Result<Self> {
        if !path.is_absolute() {
            return Err(error(
                ErrorCode::FilePolicyViolation,
                "Root must be an absolute writable directory.",
            ));
        }
        reject_links(path)?;
        let root = fs::canonicalize(path).map_err(|_| storage_error())?;
        let result = Self(root);
        for folder in [
            ".cache/tmp",
            ".cache/http",
            ".cache/browser",
            ".state/workspace",
            ".state/audit",
            ".state/browser",
            "config",
            "logs",
            "exports",
        ] {
            let target = result.path().join(folder);
            reject_links(&target)?;
            fs::create_dir_all(&target).map_err(|_| storage_error())?;
            let mut ancestor = target.as_path();
            while ancestor != result.path() {
                private_permissions(ancestor)?;
                ancestor = ancestor.parent().ok_or_else(storage_error)?;
            }
        }
        Ok(result)
    }
    pub fn discover(override_path: Option<&Path>) -> Result<Self> {
        if let Some(path) = override_path {
            return Self::explicit(path);
        }
        let executable = std::env::current_exe().map_err(|_| storage_error())?;
        let bin = executable.parent().ok_or_else(storage_error)?;
        if bin.file_name().and_then(|s| s.to_str()) != Some("bin") {
            return Err(error(
                ErrorCode::StorageUnavailable,
                "Nonstandard executable layout: pass --root with an absolute writable installation directory.",
            ));
        }
        let root = bin.parent().ok_or_else(storage_error)?;
        reject_links(&root.join("fiverr.install.json"))?;
        let bytes=fs::read(root.join("fiverr.install.json")).map_err(|_|error(ErrorCode::StorageUnavailable,"Installation marker missing; restore the complete native package or specify --root."))?;
        let marker: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| storage_error())?;
        if marker["product"] != "fiverr-agent-mcp" || marker["layout_version"] != 1 {
            return Err(error(
                ErrorCode::StorageUnavailable,
                "Installation marker is invalid.",
            ));
        }
        Self::explicit(root)
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
    pub fn safe_path(&self, area: &str, relative: &str) -> Result<PathBuf> {
        if ![
            "exports",
            "config",
            ".cache/tmp",
            ".cache/http",
            ".state/workspace",
            ".state/audit",
            "logs",
            ".state/browser",
        ]
        .contains(&area)
        {
            return Err(error(
                ErrorCode::FilePolicyViolation,
                "File area is not permitted.",
            ));
        }
        validate_relative(relative)?;
        let path = self.0.join(area).join(relative);
        reject_links(&path)?;
        let mut ancestor = path.as_path();
        while !ancestor.exists() {
            ancestor = ancestor.parent().ok_or_else(storage_error)?;
        }
        if !fs::canonicalize(ancestor)
            .map_err(|_| storage_error())?
            .starts_with(&self.0)
        {
            return Err(error(
                ErrorCode::FilePolicyViolation,
                "File escapes the installation root.",
            ));
        }
        Ok(path)
    }
    pub fn write_new(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        self.assert_owned(path)?;
        reject_links(path)?;
        let parent = path.parent().ok_or_else(storage_error)?;
        fs::create_dir_all(parent).map_err(|_| storage_error())?;
        private_permissions(parent)?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path).map_err(|_| storage_error())?;
        private_permissions(path)?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| storage_error())
    }
    pub fn assert_owned(&self, path: &Path) -> Result<()> {
        // Windows canonical roots use verbatim disk prefixes; owner input may not.
        // Simplify only safe spellings, without resolving traversal or links.
        let (lexical_path, lexical_root) = if path.starts_with(&self.0) {
            (path, self.0.as_path())
        } else {
            (dunce::simplified(path), dunce::simplified(&self.0))
        };
        if !lexical_path.starts_with(lexical_root) {
            return Err(error(
                ErrorCode::FilePolicyViolation,
                "Path is outside the installation root.",
            ));
        }
        let relative = lexical_path
            .strip_prefix(lexical_root)
            .map_err(|_| storage_error())?;
        let lexical = path.to_string_lossy();
        if lexical
            .split(['/', '\\'])
            .any(|part| part == "." || part == "..")
            || relative
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(error(
                ErrorCode::FilePolicyViolation,
                "Non-normal path components are refused.",
            ));
        }
        reject_links(path)?;
        let mut ancestor = path;
        while !ancestor.exists() {
            ancestor = ancestor.parent().ok_or_else(storage_error)?;
        }
        if !fs::canonicalize(ancestor)
            .map_err(|_| storage_error())?
            .starts_with(&self.0)
        {
            return Err(error(
                ErrorCode::FilePolicyViolation,
                "Physical path escapes the installation root.",
            ));
        }
        Ok(())
    }
    pub fn read_bounded(&self, path: &Path, max: u64) -> Result<Vec<u8>> {
        self.assert_owned(path)?;
        let metadata = fs::metadata(path)
            .map_err(|_| error(ErrorCode::ResourceNotFound, "File not found."))?;
        if !metadata.is_file() || metadata.len() > max {
            return Err(error(
                ErrorCode::FilePolicyViolation,
                "File type or size is refused.",
            ));
        }
        use std::io::Read;
        let mut data = Vec::new();
        File::open(path)
            .map_err(|_| storage_error())?
            .take(max + 1)
            .read_to_end(&mut data)
            .map_err(|_| storage_error())?;
        if data.len() as u64 > max {
            return Err(error(
                ErrorCode::FilePolicyViolation,
                "File grew beyond its size limit.",
            ));
        }
        Ok(data)
    }
}
pub fn validate_relative(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 240
        || value.contains(['\\', ':', '\0'])
        || value.starts_with('/')
        || value.ends_with('/')
    {
        return Err(error(
            ErrorCode::FilePolicyViolation,
            "Use a bounded relative file path; absolute, ADS and device paths are refused.",
        ));
    }
    if value
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(error(
            ErrorCode::FilePolicyViolation,
            "Empty/dot/traversal components are refused.",
        ));
    }
    let path = Path::new(value);
    for component in path.components() {
        let Component::Normal(segment) = component else {
            return Err(error(
                ErrorCode::FilePolicyViolation,
                "Traversal is refused.",
            ));
        };
        let segment = segment.to_str().ok_or_else(|| {
            error(
                ErrorCode::FilePolicyViolation,
                "Filename is not valid UTF-8.",
            )
        })?;
        let base = segment.split('.').next().unwrap_or("").to_ascii_uppercase();
        if segment.ends_with([' ', '.'])
            || segment
                .chars()
                .any(|c| c.is_control() || "<>\"|?*".contains(c))
            || ["CON", "PRN", "AUX", "NUL"].contains(&base.as_str())
            || (base.len() == 4
                && (base.starts_with("COM") || base.starts_with("LPT"))
                && base.as_bytes()[3].is_ascii_digit())
        {
            return Err(error(
                ErrorCode::FilePolicyViolation,
                "Unsafe filename is refused.",
            ));
        }
    }
    Ok(())
}
pub fn reject_links(path: &Path) -> Result<()> {
    for ancestor in path.ancestors() {
        if let Ok(meta) = fs::symlink_metadata(ancestor) {
            let linked = meta.file_type().is_symlink();
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 {
                    return Err(error(
                        ErrorCode::FilePolicyViolation,
                        "Reparse-point paths are refused.",
                    ));
                }
            }
            if linked {
                return Err(error(
                    ErrorCode::FilePolicyViolation,
                    "Symlink or reparse-point paths are refused.",
                ));
            }
        }
    }
    Ok(())
}
pub fn private_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if path.is_dir() { 0o700 } else { 0o600 };
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|_| storage_error())?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let user = std::env::var("USERNAME").map_err(|_| storage_error())?;
        if user.is_empty() || user.contains(['\n', '\r', ':', '"']) {
            return Err(storage_error());
        }
        let access = if path.is_dir() {
            format!("{user}:(OI)(CI)F")
        } else {
            format!("{user}:F")
        };
        // /grant:r only replaces this user's grants. Reset first to remove ALL
        // pre-existing explicit grants, then remove inherited grants before data writes.
        for arguments in [
            vec!["/reset"],
            vec!["/inheritance:r", "/grant:r", access.as_str()],
        ] {
            let mut child = std::process::Command::new("icacls.exe")
                .arg(path)
                .args(arguments)
                .creation_flags(0x08000000)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map_err(|_| storage_error())?;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            loop {
                match child.try_wait() {
                    Ok(Some(status)) if status.success() => break,
                    Ok(Some(_)) => return Err(storage_error()),
                    Err(_) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(storage_error());
                    }
                    Ok(None) => {}
                }
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(error(
                        ErrorCode::StorageUnavailable,
                        "Private Windows ACL deadline exceeded; no sensitive write allowed.",
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        if !path.exists() {
            return Err(error(
                ErrorCode::StorageUnavailable,
                "Cannot protect private Windows files. Check owner access and icacls availability.",
            ));
        }
    }
    Ok(())
}
