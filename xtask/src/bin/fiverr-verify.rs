//! Standalone read-only Windows authenticity check; no workspace or developer runtime.
use serde_json::json;

#[cfg(windows)]
fn verify() -> xtask::Result<serde_json::Value> {
    let mut args = std::env::args_os().skip(1);
    let file = args.next().ok_or("usage: fiverr-verify <executable>")?;
    if args.next().is_some() {
        return Err("usage: fiverr-verify <executable>".into());
    }
    xtask::windows_verification::verify(
        std::path::Path::new(&file),
        &xtask::windows_verification::embedded_policy()?,
    )
}
#[cfg(not(windows))]
fn verify() -> xtask::Result<serde_json::Value> {
    Err("Pinned Authenticode verification requires Windows.".into())
}
fn main() {
    match verify() {
        Ok(value) => println!("{value}"),
        Err(error) => {
            // OS/path-bearing errors are mapped at the shared input boundary.
            println!(
                "{}",
                json!({"signature_valid":false,"publicly_trusted":false,"error":error.to_string()})
            );
            std::process::exit(1);
        }
    }
}
