use fiverr_agent_mcp::{
    core::ErrorCode,
    service,
    storage::{Root, Store},
};
use serde_json::{Value, json};

fn workspace() -> (tempfile::TempDir, Store) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let store = Store::open(Root::explicit(&root).unwrap()).unwrap();
    (directory, store)
}
fn save(store: &Store, body: &str) -> Value {
    service::dispatch(
        store,
        "fiverr_record_save",
        json!({"kind":"note","id":"sample","payload":{"body":body}}),
    )
    .unwrap()
}
fn export(store: &Store, format: &str, filename: &str) -> String {
    service::dispatch(
        store,
        "fiverr_export_prepare",
        json!({"kind":"note","format":format,"filename":filename}),
    )
    .unwrap();
    let path = store.root.safe_path("exports", filename).unwrap();
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn export_format_cannot_be_disguised_as_active_content() {
    let (_directory, store) = workspace();
    save(
        &store,
        "<script>document.title=String.fromCharCode(65)</script>",
    );
    for (format, filename) in [
        ("json", "data.html"),
        ("json", "data.json.html"),
        ("csv", "data.svg"),
        ("markdown", "data.html"),
        ("html", "data.md"),
        ("json", "data"),
    ] {
        let result = service::dispatch(
            &store,
            "fiverr_export_prepare",
            json!({"kind":"note","format":format,"filename":filename}),
        );
        assert_eq!(result.unwrap_err().code, ErrorCode::InvalidInput);
        assert!(!store.root.safe_path("exports", filename).unwrap().exists());
    }
}

#[test]
fn markdown_export_keeps_untrusted_markup_in_literal_json_fences() {
    let (_directory, store) = workspace();
    let body = "![image](https://example.invalid/image) [link](https://example.invalid/link)\n````````\n<script>void(0)</script> & فارسی";
    let saved = save(&store, body);
    let text = export(&store, "markdown", "notes.md");
    let payload = serde_json::to_string_pretty(&saved["data"]["payload"]).unwrap();
    let fence = "`".repeat(9);
    assert_eq!(
        text,
        format!("# Imported note records\n\n## sample\n\n{fence}json\n{payload}\n{fence}\n\n")
    );
    assert!(!text.contains("&quot;"));
}

#[test]
fn valid_export_formats_preserve_data_and_do_not_overwrite() {
    let (_directory, store) = workspace();
    let body = "<script>void(0)</script> & فارسی";
    save(&store, body);
    let text = export(&store, "json", "notes.JSON");
    let records: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(records[0]["payload"]["body"], body);
    let html = export(&store, "html", "notes.htm");
    assert!(html.contains("Content-Security-Policy"));
    assert!(html.contains("&lt;script&gt;"));
    assert!(!html.contains("<script>"));
    assert!(export(&store, "csv", "notes.csv").starts_with("id,kind,version,payload\n"));
    assert!(export(&store, "markdown", "notes.markdown").contains("```json\n"));
    assert!(
        service::dispatch(
            &store,
            "fiverr_export_prepare",
            json!({"kind":"note","format":"json","filename":"notes.JSON"})
        )
        .is_err()
    );
    let path = store.root.safe_path("exports", "notes.JSON").unwrap();
    assert_eq!(std::fs::read_to_string(path).unwrap(), text);
}
