use super::{Actions, Cache, HttpAuth, LocalProxy, Proxy, registration};
use crate::{
    core::{ErrorCode, Result, error, invalid},
    network::{Mode, Settings, profiles},
    server, service,
    storage::{Root, Store},
};
use serde_json::{Value, json};
use std::io::{IsTerminal, Write};
pub(super) fn read_json(root: &Root, path: &std::path::Path) -> Result<Value> {
    let data = root.read_bounded(path, 2 * 1024 * 1024)?;
    serde_json::from_slice(&data).map_err(|_| invalid("Input is not valid UTF-8 JSON."))
}
pub(super) fn write_export(root: &Root, filename: &str, value: &Value) -> Result<Value> {
    let path = root.safe_path("exports", filename)?;
    root.write_new(
        &path,
        &serde_json::to_vec_pretty(value).map_err(|_| invalid("Serialization failed."))?,
    )?;
    Ok(json!({"file":filename,"written":true,"overwritten":false}))
}
pub(super) fn owner_confirm(question: &str) -> Result<()> {
    if !std::io::stdin().is_terminal() {
        return Err(error(
            ErrorCode::ConfirmationRequired,
            "Owner control requires an interactive local terminal, not model stdin/boolean approval.",
        ));
    }
    eprint!("{question} [y/N]: ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|_| invalid("Cannot read owner choice."))?;
    if line.trim() != "y" && line.trim() != "Y" {
        return Err(error(
            ErrorCode::ConfirmationRequired,
            "Owner declined; nothing changed.",
        ));
    }
    Ok(())
}
pub(super) fn actions(store: &Store, command: Actions) -> Result<Value> {
    match command {
        Actions::List => service::actions::list(store),
        Actions::Review { id } => service::actions::get(store, &id),
        Actions::Approve { id } => {
            let preview = service::actions::get(store, &id)?;
            eprintln!(
                "{}",
                serde_json::to_string_pretty(&preview).map_err(|_| invalid("Preview failed."))?
            );
            owner_confirm("Approve exactly this account/target/payload for its stated effect?")?;
            service::actions::approve(store, &id, true)
        }
        Actions::Reject { id } => service::actions::cancel(store, &id),
    }
}
pub(super) fn proxy(store: &Store, command: Proxy) -> Result<Value> {
    match command {
        Proxy::List => Ok(json!(
            Settings::load(store)?
                .profiles
                .iter()
                .map(profiles::Profile::sanitized)
                .collect::<Vec<_>>()
        )),
        Proxy::Import { file } => {
            let bytes = store.root.read_bounded(&file, 256 * 1024)?;
            let text =
                std::str::from_utf8(&bytes).map_err(|_| invalid("Proxy import must be UTF-8."))?;
            let imported = profiles::parse_import(text)?;
            let count = imported.len();
            Settings::change(store, false, |s| {
                for p in &imported {
                    if s.profiles
                        .iter()
                        .any(|x| x.label.eq_ignore_ascii_case(&p.label))
                    {
                        return Err(invalid(
                            "Proxy label conflicts with existing profiles; entire import cancelled.",
                        ));
                    }
                }
                s.profiles.extend(imported);
                Ok(())
            })?;
            Ok(json!({"imported":count,"selected":false}))
        }
        Proxy::Add { label, file } => {
            let bytes = store.root.read_bounded(&file, 2048)?;
            let url = profiles::normalize(
                std::str::from_utf8(&bytes)
                    .map_err(|_| invalid("Proxy input must be UTF-8."))?
                    .trim(),
            )?
            .to_string();
            Settings::change(store, false, |s| {
                if s.profiles
                    .iter()
                    .any(|p| p.label.eq_ignore_ascii_case(&label))
                {
                    return Err(invalid("Proxy label already exists."));
                }
                s.profiles.push(profiles::Profile {
                    id: uuid::Uuid::new_v4().to_string(),
                    label,
                    url,
                });
                Ok(())
            })?;
            Ok(json!({"added":true,"selected":false}))
        }
        Proxy::Edit { id, file } => {
            let bytes = store.root.read_bounded(&file, 2048)?;
            let url = profiles::normalize(
                std::str::from_utf8(&bytes)
                    .map_err(|_| invalid("Proxy input must be UTF-8."))?
                    .trim(),
            )?
            .to_string();
            Settings::change(store, true, |s| {
                let p = s
                    .profiles
                    .iter_mut()
                    .find(|p| p.id == id)
                    .ok_or_else(|| invalid("Proxy ID not found."))?;
                p.url = url;
                Ok(())
            })?;
            Ok(json!({"updated":true}))
        }
        Proxy::Select { id } => {
            Settings::change(store, true, |s| {
                if !s.profiles.iter().any(|p| p.id == id) {
                    return Err(invalid("Proxy ID not found."));
                }
                s.selected_profile_id = Some(id);
                s.mode = Mode::Proxy;
                Ok(())
            })?;
            Ok(json!({"selected":true,"mode":"proxy"}))
        }
        Proxy::Delete { id } => {
            Settings::change(store, true, |s| {
                if !s.profiles.iter().any(|p| p.id == id) {
                    return Err(invalid("Proxy ID not found."));
                }
                s.profiles.retain(|p| p.id != id);
                if s.selected_profile_id.as_ref() == Some(&id) {
                    s.mode = Mode::Blocked;
                    s.selected_profile_id = None;
                }
                Ok(())
            })?;
            Ok(json!({"deleted":true,"active_profile_removal_blocks":true}))
        }
        Proxy::Test => Ok(
            json!({"configuration_valid":crate::network::RouteClient::new(store.clone()).is_ok(),"external_probe":false,"reachability_verified":false,"destination_dns":"SOCKS5 normalized to remote; HTTP CONNECT forwards authority","proxy_bootstrap_dns":"named proxy may resolve locally","resolver_geography":"not_guaranteed"}),
        ),
    }
}
pub(super) fn local_proxy(store: &Store, command: LocalProxy) -> Result<Value> {
    match command {
        LocalProxy::Settings { file: None } => {
            let settings = Settings::load(store)?;
            let selected = settings
                .profiles
                .iter()
                .find(|p| Some(&p.id) == settings.selected_profile_id.as_ref());
            let endpoint = profiles::Profile {
                id: "local".into(),
                label: "Local operator proxy".into(),
                url: settings.local_proxy.clone(),
            };
            Ok(
                json!({"configured_endpoint":endpoint.sanitized(),"active":settings.mode==Mode::Proxy && selected.is_some_and(|p|p.url==settings.local_proxy),"service_installed":false}),
            )
        }
        LocalProxy::Settings { file: Some(file) } => {
            let bytes = store.root.read_bounded(&file, 2048)?;
            let url = profiles::normalize(
                std::str::from_utf8(&bytes)
                    .map_err(|_| invalid("UTF-8 required."))?
                    .trim(),
            )?
            .to_string();
            Settings::change(store, false, |s| {
                s.local_proxy = url;
                Ok(())
            })?;
            Ok(json!({"updated":true,"activated":false}))
        }
        LocalProxy::Select => {
            Settings::change(store, true, |s| {
                let id = uuid::Uuid::new_v4().to_string();
                s.profiles.push(profiles::Profile {
                    id: id.clone(),
                    label: format!("Local proxy {}", s.profiles.len() + 1),
                    url: s.local_proxy.clone(),
                });
                s.selected_profile_id = Some(id);
                s.mode = Mode::Proxy;
                Ok(())
            })?;
            Ok(json!({"selected":true,"proxy_service_installed":false}))
        }
    }
}
pub(super) fn http_auth(store: &Store, command: HttpAuth) -> Result<Value> {
    match command {
        HttpAuth::Status => {
            Ok(json!({"mode":Settings::load(store)?.http_auth_mode,"token_disclosed":false}))
        }
        HttpAuth::Set { mode } => {
            if !["token", "loopback-trusted"].contains(&mode.as_str()) {
                return Err(invalid("Mode must be token or loopback-trusted."));
            }
            if mode == "loopback-trusted" {
                owner_confirm(
                    "Other local processes can access this connection without a token. Enable trusted mode?",
                )?;
            }
            Settings::change(store, false, |s| {
                s.http_auth_mode = mode;
                Ok(())
            })?;
            Ok(json!({"updated":true}))
        }
        HttpAuth::Rotate => {
            server::http::rotate(store)?;
            Ok(json!({"rotated":true}))
        }
        HttpAuth::Export { filename } => {
            owner_confirm(
                "Write a private local HTTP authorization file? Never publish or paste its content.",
            )?;
            write_export(
                &store.root,
                &filename,
                &json!({"headers":{"Authorization":format!("Bearer {}",server::http::token(store)?)}}),
            )
        }
    }
}
pub(super) fn inventory(root: &Root, area: &str) -> Result<Value> {
    let mut entries = vec![];
    for entry in std::fs::read_dir(root.path().join(area))
        .map_err(|_| crate::core::storage_error())?
        .take(100)
    {
        let entry = entry.map_err(|_| crate::core::storage_error())?;
        entries.push(json!({"name":entry.file_name().to_string_lossy(),"bytes":entry.metadata().map_err(|_|crate::core::storage_error())?.len()}));
    }
    Ok(json!({"entries":entries,"contents_disclosed":false}))
}
pub(super) fn cache(root: &Root, command: Cache) -> Result<Value> {
    match command {
        Cache::Inspect => inventory(root, ".cache/tmp"),
        Cache::Clean { apply } => {
            if apply {
                return Err(error(
                    ErrorCode::HumanActionRequired,
                    "Automatic recursive cleanup is disabled until owned-worker quiescence is verified. Inspect cache and remove only known disposable files.",
                ));
            }
            Ok(json!({"dry_run":true,"deleted":false,"inventory":inventory(root,".cache/tmp")?}))
        }
    }
}
