mod management;
pub mod registration;
use crate::{
    core::{ErrorCode, Result, error, invalid},
    network::{Mode, Settings, profiles},
    server, service,
    storage::{Root, Store},
};
use clap::{Parser, Subcommand};
use management::*;
use serde_json::{Value, json};
use std::{io::IsTerminal, path::PathBuf};

#[derive(Parser)]
#[command(
    name = "fiverr-agent-mcp",
    version,
    about = "Unofficial native Fiverr workspace; account automation requires verified permission and route assurance."
)]
pub struct Arguments {
    #[arg(long, global = true)]
    pub root: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Option<Command>,
}
#[derive(Subcommand)]
pub enum Command {
    Version,
    Menu,
    Status,
    Capabilities,
    Doctor,
    Bridge,
    Http {
        #[arg(long)]
        port: Option<u16>,
    },
    HttpPort {
        port: u16,
    },
    HttpConfig,
    HttpAuth {
        #[command(subcommand)]
        command: HttpAuth,
    },
    DesktopConfig {
        #[arg(long)]
        entry_only: bool,
    },
    Register {
        file: PathBuf,
    },
    Unregister {
        file: PathBuf,
    },
    Proxy {
        #[command(subcommand)]
        command: Proxy,
    },
    LocalProxy {
        #[command(subcommand)]
        command: LocalProxy,
    },
    Direct,
    Block,
    Auth {
        #[command(subcommand)]
        command: Auth,
    },
    Browser {
        #[command(subcommand)]
        command: Browser,
    },
    Config {
        #[command(subcommand)]
        command: Config,
    },
    Cache {
        #[command(subcommand)]
        command: Cache,
    },
    Logs {
        #[command(subcommand)]
        command: Logs,
    },
    Diagnostics {
        #[command(subcommand)]
        command: Diagnostics,
    },
    Plugins {
        #[command(subcommand)]
        command: Plugins,
    },
    Actions {
        #[command(subcommand)]
        command: Actions,
    },
    Backup {
        #[command(subcommand)]
        command: Backup,
    },
    Call {
        name: String,
        #[arg(long)]
        input: Option<PathBuf>,
    },
}
#[derive(Subcommand)]
pub enum HttpAuth {
    Status,
    Set { mode: String },
    Rotate,
    Export { filename: String },
}
#[derive(Subcommand)]
pub enum Proxy {
    List,
    Import {
        file: PathBuf,
    },
    Add {
        label: String,
        #[arg(long)]
        file: PathBuf,
    },
    Edit {
        id: String,
        #[arg(long)]
        file: PathBuf,
    },
    Select {
        id: String,
    },
    Delete {
        id: String,
    },
    Test,
}
#[derive(Subcommand)]
pub enum LocalProxy {
    Settings {
        #[arg(long)]
        file: Option<PathBuf>,
    },
    Select,
}
#[derive(Subcommand)]
pub enum Auth {
    SignIn,
    Status,
    Logout,
    RemoveLocalSession,
}
#[derive(Subcommand)]
pub enum Browser {
    Status,
    Configure { executable: PathBuf },
}
#[derive(Subcommand)]
pub enum Config {
    Validate,
    Export { filename: String },
}
#[derive(Subcommand)]
pub enum Cache {
    Inspect,
    Clean {
        #[arg(long)]
        apply: bool,
    },
}
#[derive(Subcommand)]
pub enum Logs {
    Inspect,
}
#[derive(Subcommand)]
pub enum Diagnostics {
    Export { filename: String },
}
#[derive(Subcommand)]
pub enum Plugins {
    Validate,
    Manifest,
}
#[derive(Subcommand)]
pub enum Actions {
    List,
    Review { id: String },
    Approve { id: String },
    Reject { id: String },
}
#[derive(Subcommand)]
pub enum Backup {
    Create { filename: String },
    Validate { filename: String },
    Restore { filename: String },
}

pub async fn execute(root: Root, store: Store, command: Command) -> Result<Value> {
    match command {
        Command::Version => {
            Ok(json!({"name":"fiverr-agent-mcp","version":env!("CARGO_PKG_VERSION")}))
        }
        Command::Bridge => {
            server::bridge(store).await?;
            Ok(Value::Null)
        }
        Command::Http { port } => {
            server::http::run(store, port).await?;
            Ok(Value::Null)
        }
        Command::Menu => menu(&store),
        Command::Status | Command::Doctor => service::dispatch(&store, "fiverr_status", json!({})),
        Command::Capabilities => server::catalog::capabilities(),
        Command::HttpPort { port } => {
            if port == 0 {
                return Err(invalid("Port 0 is not a production listener port."));
            }
            Settings::change(&store, false, |s| {
                s.http_port = port;
                Ok(())
            })?;
            Ok(json!({"port":port,"restart_required":true}))
        }
        Command::HttpConfig => {
            let s = Settings::load(&store)?;
            Ok(
                json!({"url":format!("http://127.0.0.1:{}/mcp",s.http_port),"auth_mode":s.http_auth_mode,"token_export":"http-auth export <filename>","environment_precedence":"CLI > FIVERR_AGENT_HTTP_PORT > saved > 1597"}),
            )
        }
        Command::HttpAuth { command } => http_auth(&store, command),
        Command::DesktopConfig { entry_only } => Ok(registration::desktop(&root, entry_only)),
        Command::Register { file } => {
            owner_confirm(
                "Modify only fiverr-local in the explicit client file, keeping a backup?",
            )?;
            registration::register(&root, &file, false)
        }
        Command::Unregister { file } => {
            owner_confirm("Remove only this installation's fiverr-local entry?")?;
            registration::register(&root, &file, true)
        }
        Command::Proxy { command } => proxy(&store, command),
        Command::LocalProxy { command } => local_proxy(&store, command),
        Command::Direct => {
            owner_confirm(
                "Enable direct outbound networking explicitly? Native route does not configure external browsers.",
            )?;
            Settings::change(&store, true, |s| {
                s.mode = Mode::Direct;
                s.selected_profile_id = None;
                Ok(())
            })?;
            Ok(json!({"mode":"direct","generation_changed":true}))
        }
        Command::Block => {
            Settings::change(&store, true, |s| {
                s.mode = Mode::Blocked;
                Ok(())
            })?;
            Ok(
                json!({"mode":"blocked","generation_changed":true,"active_work":"cooperative generation cancellation within 50ms; no remote write adapter dispatched"}),
            )
        }
        Command::Auth {
            command: Auth::SignIn,
        } => crate::browser::sign_in(&store).await,
        Command::Auth {
            command: Auth::Status,
        } => service::account::status(&store),
        Command::Auth {
            command: Auth::Logout,
        } => service::account::handoff("remote_logout", &json!({})),
        Command::Auth {
            command: Auth::RemoveLocalSession,
        } => Err(error(
            ErrorCode::HumanActionRequired,
            "No app-managed authenticated session exists. Do not delete browser profiles/drafts as logout.",
        )),
        Command::Browser {
            command: Browser::Status,
        } => crate::browser::status(&store),
        Command::Browser {
            command: Browser::Configure { executable },
        } => {
            crate::browser::configure(&store, &executable)?;
            Ok(
                json!({"browser_configured":true,"downloaded":false,"route_assurance":"not_verified"}),
            )
        }
        Command::Config {
            command: Config::Validate,
        } => {
            let settings = Settings::load(&store)?;
            settings.effective_port(None, None)?;
            for p in &settings.profiles {
                profiles::normalize(&p.url)?;
            }
            Ok(json!({"valid":true}))
        }
        Command::Config {
            command: Config::Export { filename },
        } => write_export(&root, &filename, &Settings::load(&store)?.sanitized()),
        Command::Cache { command } => cache(&root, command),
        Command::Logs {
            command: Logs::Inspect,
        } => inventory(&root, "logs"),
        Command::Diagnostics {
            command: Diagnostics::Export { filename },
        } => write_export(
            &root,
            &filename,
            &json!({"status":Settings::load(&store)?.sanitized(),"version":env!("CARGO_PKG_VERSION"),"private_content_excluded":true}),
        ),
        Command::Plugins {
            command: Plugins::Manifest,
        } => serde_json::from_str(include_str!("../../plugin.json"))
            .map_err(|_| invalid("Plugin manifest invalid.")),
        Command::Plugins {
            command: Plugins::Validate,
        } => {
            let value: Value = serde_json::from_str(include_str!("../../plugin.json"))
                .map_err(|_| invalid("Plugin manifest invalid."))?;
            if value["name"] != "fiverr-agent-mcp" {
                return Err(invalid("Plugin identity mismatch."));
            }
            Ok(json!({"manifest_valid":true,"native_client_loading":"not_verified"}))
        }
        Command::Actions { command } => actions(&store, command),
        Command::Backup {
            command: Backup::Create { filename },
        } => {
            store.backup(&filename)?;
            Ok(json!({"file":filename,"private":true,"contains_workspace_and_settings":true}))
        }
        Command::Backup {
            command: Backup::Validate { filename },
        } => {
            let path = root.safe_path("exports", &filename)?;
            let bytes = root.read_bounded(&path, 64 * 1024 * 1024)?;
            if !bytes.starts_with(b"SQLite format 3\0") {
                return Err(invalid("Backup is not a SQLite database."));
            }
            Ok(json!({"header_valid":true,"restore_verified":false}))
        }
        Command::Backup {
            command: Backup::Restore { .. },
        } => Err(error(
            ErrorCode::HumanActionRequired,
            "In-place rollback/restore is not implemented. Preserve the backup; use a clean compatible installation, never overwrite live state blindly.",
        )),
        Command::Call { name, input } => {
            let args = if let Some(file) = input {
                read_json(&root, &file)?
            } else {
                json!({})
            };
            service::dispatch(&store, &name, args)
        }
    }
}
fn menu(store: &Store) -> Result<Value> {
    if !std::io::stdin().is_terminal() {
        return Err(invalid(
            "Menu requires an interactive terminal; use scriptable commands for automation.",
        ));
    }
    loop {
        eprintln!(
            "\nFiverr Agent MCP — Copyright (C) 2026 KiaroSama\nGPL-3.0-only; ABSOLUTELY NO WARRANTY. See LICENSE and THIRD_PARTY_NOTICES.md.\n1 Status\n2 Proxy profiles\n3 Browser/account status\n4 Prepared actions\n5 HTTP configuration\n0 Exit"
        );
        let mut choice = String::new();
        std::io::stdin()
            .read_line(&mut choice)
            .map_err(|_| invalid("Menu input failed."))?;
        let result = match choice.trim() {
            "0" => break,
            "1" => service::dispatch(store, "fiverr_status", json!({})),
            "2" => proxy(store, Proxy::List),
            "3" => service::account::status(store),
            "4" => service::actions::list(store),
            "5" => Ok(json!({"port":Settings::load(store)?.http_port})),
            _ => Err(invalid("Choose 0-5.")),
        };
        match result {
            Ok(v) => eprintln!("{v}"),
            Err(e) => eprintln!("{e}"),
        }
    }
    Ok(json!({"menu_closed":true}))
}
