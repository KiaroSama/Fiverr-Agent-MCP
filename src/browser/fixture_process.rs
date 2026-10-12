use crate::{
    core::{ErrorCode, Result, error, invalid},
    storage::Root,
};
use chromiumoxide::{Browser, Handler, handler::HandlerConfig};
use std::{path::Path, process::Stdio, time::Duration};

pub async fn launch(
    root: &Root,
    executable: &Path,
    profile: &Path,
) -> Result<(Browser, Handler, tokio::process::Child)> {
    let stderr_path = root.safe_path("logs", "fixture-browser.stderr")?;
    root.write_new(&stderr_path, b"")?;
    let stderr = std::fs::OpenOptions::new()
        .write(true)
        .open(&stderr_path)
        .map_err(|_| crate::core::storage_error())?;
    let mut command = tokio::process::Command::new(executable);
    command
        .args([
            "--headless=new",
            "--remote-debugging-port=0",
            "--remote-debugging-address=127.0.0.1",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-background-networking",
            "--disable-extensions",
            "--disable-component-update",
            "--enable-automation",
            "--password-store=basic",
            "--use-mock-keychain",
            "--disable-dev-shm-usage",
            "--enable-logging=stderr",
            "about:blank",
        ])
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg(format!(
            "--disk-cache-dir={}",
            root.path().join(".cache/browser").display()
        ))
        .env("TEMP", root.path().join(".cache/tmp"))
        .env("TMP", root.path().join(".cache/tmp"))
        .env("TMPDIR", root.path().join(".cache"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr))
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command.spawn().map_err(|_| {
        error(
            ErrorCode::FeatureUnavailable,
            "Fixture browser process failed to start.",
        )
    })?;
    // Chrome writes this readiness contract even when GUI-build stderr is silent.
    let ready = async {
        let mut ticks = tokio::time::interval(Duration::from_millis(50));
        loop {
            if child
                .try_wait()
                .map_err(|_| crate::core::storage_error())?
                .is_some()
            {
                return Err(error(
                    ErrorCode::FeatureUnavailable,
                    "Fixture browser exited before readiness.",
                ));
            }
            let path = profile.join("DevToolsActivePort");
            if path.exists() {
                let text = root.read_bounded(&path, 4096)?;
                let text = std::str::from_utf8(&text)
                    .map_err(|_| invalid("Invalid browser readiness encoding."))?;
                let lines: Vec<_> = text.lines().collect();
                if lines.len() == 2 && lines[1].starts_with("/devtools/browser/") {
                    let port: u16 = lines[0]
                        .parse()
                        .map_err(|_| invalid("Invalid browser readiness port."))?;
                    if port == 0
                        || !lines[1]
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"/-".contains(&b))
                    {
                        return Err(invalid("Invalid browser readiness endpoint."));
                    }
                    let config = HandlerConfig {
                        ignore_https_errors: false,
                        ignore_invalid_messages: false,
                        request_timeout: Duration::from_secs(5),
                        ..Default::default()
                    };
                    return Browser::connect_with_config(
                        format!("ws://127.0.0.1:{port}{}", lines[1]),
                        config,
                    )
                    .await
                    .map_err(|_| {
                        error(
                            ErrorCode::FeatureUnavailable,
                            "Owned fixture browser connection failed.",
                        )
                    });
                }
            }
            let bytes = root.read_bounded(&stderr_path, 16384)?;
            if let Ok(text) = std::str::from_utf8(&bytes)
                && let Some(endpoint) = text
                    .lines()
                    .find_map(|line| line.strip_prefix("DevTools listening on "))
            {
                let endpoint = url::Url::parse(endpoint.trim())
                    .map_err(|_| invalid("Invalid owned browser endpoint."))?;
                if endpoint.scheme() != "ws"
                    || endpoint.host_str() != Some("127.0.0.1")
                    || endpoint.port().is_none()
                    || !endpoint.username().is_empty()
                    || endpoint.query().is_some()
                    || endpoint.fragment().is_some()
                    || !endpoint.path().starts_with("/devtools/browser/")
                {
                    return Err(invalid("Owned browser endpoint must be loopback."));
                }
                let config = HandlerConfig {
                    ignore_https_errors: false,
                    ignore_invalid_messages: false,
                    request_timeout: Duration::from_secs(5),
                    ..Default::default()
                };
                return Browser::connect_with_config(endpoint.to_string(), config)
                    .await
                    .map_err(|_| {
                        error(
                            ErrorCode::FeatureUnavailable,
                            "Owned fixture browser connection failed.",
                        )
                    });
            }
            ticks.tick().await;
        }
    };
    match tokio::time::timeout(Duration::from_secs(10), ready).await {
        Ok(Ok((browser, handler))) => Ok((browser, handler, child)),
        result => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            if let Ok(bytes) = root.read_bounded(&stderr_path, 16384) {
                eprintln!(
                    "Owned loopback browser readiness failure: {}",
                    String::from_utf8_lossy(&bytes)
                );
            }
            match result {
                Ok(Err(error)) => Err(error),
                _ => Err(error(
                    ErrorCode::FeatureUnavailable,
                    "Owned fixture browser readiness deadline exceeded.",
                )),
            }
        }
    }
}
