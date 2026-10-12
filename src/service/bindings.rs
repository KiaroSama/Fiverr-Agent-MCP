use crate::{
    core::{ErrorCode, Result, error, hash, invalid},
    storage::Store,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileBinding {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
    pub mime: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordBinding {
    pub kind: String,
    pub id: String,
    pub version: u64,
}
pub fn files(store: &Store, args: &Value) -> Result<Vec<FileBinding>> {
    let Some(files) = args.get("files") else {
        return Ok(vec![]);
    };
    let files = files
        .as_array()
        .ok_or_else(|| invalid("files must contain explicit relative workspace file names."))?;
    if files.len() > 10 {
        return Err(invalid("At most 10 file bindings are allowed."));
    }
    let mut bindings = vec![];
    for file in files {
        let name = file
            .as_str()
            .ok_or_else(|| invalid("File name must be text."))?;
        let path = store.root.safe_path(".state/workspace", name)?;
        let bytes = store.root.read_bounded(&path, 10 * 1024 * 1024)?;
        let mime = match path
            .extension()
            .and_then(|s| s.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("txt" | "md" | "csv" | "json") => {
                std::str::from_utf8(&bytes)
                    .map_err(|_| invalid("Text attachment must be UTF-8."))?;
                "text/plain"
            }
            Some("pdf") if bytes.starts_with(b"%PDF-") => "application/pdf",
            Some("png") if bytes.starts_with(b"\x89PNG\r\n\x1a\n") => "image/png",
            Some("jpg" | "jpeg") if bytes.starts_with(b"\xff\xd8\xff") => "image/jpeg",
            _ => {
                return Err(error(
                    ErrorCode::FilePolicyViolation,
                    "Attachment type is not allowlisted or does not match its signature. Archives, executables and active web formats are refused.",
                ));
            }
        };
        bindings.push(FileBinding {
            path: name.into(),
            bytes: bytes.len() as u64,
            sha256: hash(&bytes),
            mime: mime.into(),
        });
    }
    Ok(bindings)
}
pub fn unchanged(store: &Store, files: &[FileBinding]) -> Result<()> {
    for file in files {
        let path = store.root.safe_path(".state/workspace", &file.path)?;
        let bytes = store.root.read_bounded(&path, 10 * 1024 * 1024)?;
        if bytes.len() as u64 != file.bytes || hash(&bytes) != file.sha256 {
            return Err(error(
                ErrorCode::ActionStateChanged,
                "Prepared file changed; nothing dispatched. Prepare and review again.",
            ));
        }
    }
    Ok(())
}
pub fn record(store: &Store, account: &str, args: &Value) -> Result<Option<RecordBinding>> {
    let Some(value) = args.get("record_binding") else {
        return Ok(None);
    };
    let binding: RecordBinding = serde_json::from_value(value.clone())
        .map_err(|_| invalid("Record binding needs kind, id and integer version."))?;
    let current = store.get(account, &binding.kind, &binding.id)?;
    if current.version != binding.version {
        return Err(error(
            ErrorCode::ActionStateChanged,
            "Bound draft version changed.",
        ));
    }
    Ok(Some(binding))
}
