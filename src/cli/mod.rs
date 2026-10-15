pub mod console;
mod management;
pub mod menu;
pub mod registration;
pub mod session;
use crate::{
    core::{ErrorCode, Result, error, invalid},
    network::{Mode, Settings, profiles},
    server, service,
    storage::{Root, Store},
};
use clap::{Parser, Subcommand};
use management::*;
use serde_json::{Value, json};
use std::path::PathBuf;

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
    Preview,
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
    Session {
        #[command(subcommand)]
        command: session::Session,
    },
    #[command(hide = true)]
    NativeSession {
        caller: String,
    },
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
        Command::Menu => menu::terminal(&root, &store).await,
        Command::Preview => {
            menu::preview(&store, &mut console::Terminal)?;
            Ok(Value::Null)
        }
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
                "Modify only Fiverr-Agent-mcp in the explicit client file, keeping a backup?",
            )?;
            registration::register(&root, &file, false)
        }
        Command::Unregister { file } => {
            owner_confirm("Remove only this installation's Fiverr-Agent-mcp entry?")?;
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
        Command::Session { command } => session::execute(&store, command).await,
        Command::NativeSession { caller } => crate::session::native::run(&store, &caller).await,
        Command::Auth {
            command: Auth::SignIn,
        } => {
            use std::io::IsTerminal;
            if !std::io::stdin().is_terminal() {
                return Err(error(
                    ErrorCode::HumanActionRequired,
                    "Sign-in requires an interactive owner terminal; use session import/verify or extension-pair explicitly.",
                ));
            }
            let Some(command) = session::menu(&root, &mut console::Terminal)? else {
                return Ok(Value::Null);
            };
            Box::pin(execute(root, store, command)).await
        }
        Command::Auth {
            command: Auth::Status,
        } => service::account::status(&store),
        Command::Auth {
            command: Auth::Logout,
        } => service::account::handoff("remote_logout", &json!({})),
        Command::Auth {
            command: Auth::RemoveLocalSession,
        } => crate::session::forget(&store),
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
        } => store.validate_backup(&filename),
        Command::Backup {
            command: Backup::Restore { filename },
        } => {
            let preview = store.validate_backup(&filename)?;
            eprintln!("{preview}");
            owner_confirm(
                "Restore this backup into an empty workspace with networking blocked and old approvals invalidated?",
            )?;
            store.restore_backup_checked(&filename, preview["sha256"].as_str())
        }
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
