use super::{
    console::{Console, Style, safe_text},
    *,
};
use std::io::IsTerminal;

const ITEMS: &[&str] = &[
    "Select proxy [1]",
    "Import proxy list from file",
    "Add one proxy",
    "Direct connection (explicit)",
    "Block connections",
    "Sign in to Fiverr (cookie / browser extension)",
    "Register MCP in your client",
    "Local proxy settings (default port 1595)",
    "Select local proxy from settings",
    "Start local MCP HTTP server",
    "Set local HTTP port",
    "Local diagnostics",
    "Check route configuration",
    "Browser and account settings",
    "Workspace tools",
];
pub fn preview(store: &Store, screen: &mut impl Console) -> Result<()> {
    let settings = Settings::load(store)?;
    screen.write(
        "\n                           Fiverr Agent MCP\n",
        Style::Accent,
    )?;
    screen.write(&format!("{}\n", "=".repeat(80)), Style::Accent)?;
    let mode = match settings.mode {
        Mode::Blocked => "BLOCKED",
        Mode::Direct => "DIRECT",
        Mode::Proxy => "PROXY",
    };
    let selected = settings
        .profiles
        .iter()
        .find(|p| Some(&p.id) == settings.selected_profile_id.as_ref());
    screen.write(
        &format!(
            "  Connection: {mode} | Proxy: {} | Saved proxies: {}\n",
            selected
                .map(|p| safe_text(&p.label))
                .unwrap_or_else(|| "none".into()),
            settings.profiles.len()
        ),
        Style::Plain,
    )?;
    let session = crate::session::status(store)?;
    let state = session["state"].as_str().unwrap_or("not_configured");
    screen.write(
        &format!(
            "  Account: {} | Session: {} | Automation permission: not verified\n",
            if session["account_observed"] == true {
                "Observed"
            } else {
                "Not connected"
            },
            safe_text(state)
        ),
        Style::Warning,
    )?;
    screen.write("\nFiverr Main menu:\n", Style::Accent)?;
    for (i, item) in ITEMS.iter().enumerate() {
        screen.write(&format!("  {}. ", i + 1), Style::Accent)?;
        screen.write(item, if i == 0 { Style::Success } else { Style::Plain })?;
        screen.write("\n", Style::Plain)?;
    }
    screen.write(
        "\n  Enter=select proxy | back=0 | quit/exit=close\n",
        Style::Muted,
    )
}
pub(super) fn ask(
    screen: &mut impl Console,
    label: &str,
    default: &str,
    secret: bool,
) -> Result<Option<String>> {
    screen.write(
        &format!("\n{label} [{default}] {{back=0, quit=exit}}: "),
        Style::Plain,
    )?;
    let input = screen
        .read(secret)?
        .ok_or_else(|| invalid("Menu exit requested."))?;
    let input = input.trim();
    if matches!(input.to_ascii_lowercase().as_str(), "quit" | "exit") {
        return Err(invalid("Menu exit requested."));
    }
    if matches!(input.to_ascii_lowercase().as_str(), "0" | "b" | "back") {
        return Ok(None);
    }
    Ok(Some(if input.is_empty() {
        default.to_owned()
    } else {
        input.to_owned()
    }))
}
pub(super) fn path(root: &Root, input: &str) -> std::path::PathBuf {
    let input = input.trim();
    let input = if input.len() >= 2
        && ((input.starts_with('"') && input.ends_with('"'))
            || (input.starts_with('\'') && input.ends_with('\'')))
    {
        &input[1..input.len() - 1]
    } else {
        input
    };
    let value = std::path::PathBuf::from(input);
    if value.is_absolute() {
        value
    } else {
        root.path().join(value)
    }
}
async fn invoke(root: &Root, store: &Store, command: Command) -> Result<Value> {
    Box::pin(super::execute(root.clone(), store.clone(), command)).await
}
fn result(screen: &mut impl Console, value: &Value) -> Result<()> {
    let data = value.get("data").unwrap_or(value);
    if value.is_null() {
        return Ok(());
    }
    if let Some(count) = data.get("imported").and_then(Value::as_u64) {
        return screen.write(
            &format!("Imported: {count}; route unchanged.\n"),
            Style::Success,
        );
    }
    if let Some(port) = data.get("port").and_then(Value::as_u64) {
        return screen.write(
            &format!("HTTP port: {port}. Restart the local listener to apply.\n"),
            Style::Success,
        );
    }
    if let Some(valid) = data.get("configuration_valid").and_then(Value::as_bool) {
        return screen.write(
            &format!(
                "Route configuration: {}. External IP/DNS reachability was not probed.\n",
                if valid { "valid" } else { "blocked or invalid" }
            ),
            Style::Warning,
        );
    }
    if let Some(state) = data.get("state").and_then(Value::as_str) {
        return screen.write(
            &format!(
                "Session: {}. Account observed: {}. Automation permission: not verified.\n",
                safe_text(state),
                data["account_observed"] == true
            ),
            Style::Warning,
        );
    }
    if data.get("live_authenticated").is_some() || data.get("live_verified").is_some() {
        let observed = data
            .get("session")
            .is_some_and(|s| s["account_observed"] == true);
        return screen.write(
            &format!(
                "Local runtime available. Account: {}; automation permission is not verified.\n",
                if observed {
                    "Observed"
                } else {
                    "Not connected"
                }
            ),
            Style::Warning,
        );
    }
    if data.get("mode") == Some(&json!("manual_handoff")) {
        return screen.write(
            "Manual handoff only; no remote action or logout was completed.\n",
            Style::Warning,
        );
    }
    if let Some(file) = data.get("file").and_then(Value::as_str) {
        return screen.write(
            &format!(
                "Private output: {} (under installation exports).\n",
                safe_text(file)
            ),
            Style::Success,
        );
    }
    if let Some(url) = data.get("url").and_then(Value::as_str) {
        screen.write(
            &format!("Local HTTP endpoint: {}\n", safe_text(url)),
            Style::Accent,
        )?;
        return screen.write(
            "A private authorization token is required; do not paste it into chat.\n",
            Style::Warning,
        );
    }
    if let Some(configured) = data.get("configured").and_then(Value::as_bool) {
        return screen.write(&format!("Browser configured: {configured}. Account identity and browser routing are not verified.\n"), Style::Warning);
    }
    if let Some(items) = data
        .as_array()
        .or_else(|| data.get("items").and_then(Value::as_array))
        .or_else(|| data.get("entries").and_then(Value::as_array))
    {
        screen.write(&format!("{} item(s).\n", items.len()), Style::Plain)?;
        for item in items.iter().take(100) {
            let label = item
                .get("label")
                .or_else(|| item.get("name"))
                .or_else(|| item.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("item");
            screen.write(&format!("  {}\n", safe_text(label)), Style::Plain)?;
        }
        return Ok(());
    }
    screen.write(
        "Operation complete. Reconnect MCP after a route change.\n",
        Style::Success,
    )
}
fn select(store: &Store, screen: &mut impl Console) -> Result<Option<Command>> {
    let settings = Settings::load(store)?;
    if settings.profiles.is_empty() {
        screen.write(
            "No saved proxies. Import a list with option 2.\n",
            Style::Warning,
        )?;
        return Ok(None);
    }
    for (i, p) in settings.profiles.iter().enumerate() {
        let public = p.sanitized();
        screen.write(
            &format!(
                "  {}. {} - {}://{}:{}{}\n",
                i + 1,
                safe_text(&p.label),
                public["scheme"].as_str().unwrap_or("?"),
                public["host"].as_str().unwrap_or("?"),
                public["port"],
                if settings.selected_profile_id.as_ref() == Some(&p.id) {
                    " [active]"
                } else {
                    ""
                }
            ),
            Style::Plain,
        )?;
    }
    let Some(pick) = ask(screen, "Proxy number", "0", false)? else {
        return Ok(None);
    };
    let p = pick
        .parse::<usize>()
        .ok()
        .and_then(|n| n.checked_sub(1))
        .and_then(|n| settings.profiles.get(n))
        .ok_or_else(|| invalid("Enter a listed proxy number."))?;
    Ok(Some(Command::Proxy {
        command: Proxy::Select { id: p.id.clone() },
    }))
}
async fn browser(root: &Root, screen: &mut impl Console) -> Result<Option<Command>> {
    screen.write("\nAccount and browser:\n  1. Sign in to Fiverr (cookie / browser extension)\n  2. Account status\n  3. Configure browser executable\n  4. Browser status\n  5. Logout (manual handoff)\n", Style::Accent)?;
    let Some(pick) = ask(screen, "Selection", "0", false)? else {
        return Ok(None);
    };
    Ok(Some(match pick.as_str() {
        "1" => return session::menu(root, screen),
        "2" => Command::Auth {
            command: Auth::Status,
        },
        "3" => {
            let Some(p) = ask(screen, "Browser executable path", "", false)? else {
                return Ok(None);
            };
            Command::Browser {
                command: Browser::Configure {
                    executable: path(root, &p),
                },
            }
        }
        "4" => Command::Browser {
            command: Browser::Status,
        },
        "5" => Command::Auth {
            command: Auth::Logout,
        },
        _ => return Err(invalid("Enter an account/browser number from 1 to 5.")),
    }))
}
async fn workspace(screen: &mut impl Console) -> Result<Option<Command>> {
    screen.write("\nWorkspace tools:\n  1. Prepared actions\n  2. Create backup\n  3. Validate backup\n  4. Restore backup (empty workspace only)\n  5. Inspect logs\n  6. Export diagnostics\n  7. Inspect cache\n  8. Preview cache cleanup\n  9. Apply cache cleanup\n  10. HTTP connection settings\n  11. Rotate HTTP access token\n  12. Validate plugin\n", Style::Accent)?;
    let Some(pick) = ask(screen, "Selection", "0", false)? else {
        return Ok(None);
    };
    Ok(Some(match pick.as_str() {
        "1" => Command::Actions {
            command: Actions::List,
        },
        "2" | "3" | "4" => {
            let Some(filename) = ask(
                screen,
                "Backup filename in exports",
                "workspace.sqlite3",
                false,
            )?
            else {
                return Ok(None);
            };
            Command::Backup {
                command: match pick.as_str() {
                    "2" => Backup::Create { filename },
                    "3" => Backup::Validate { filename },
                    _ => Backup::Restore { filename },
                },
            }
        }
        "5" => Command::Logs {
            command: Logs::Inspect,
        },
        "6" => {
            let Some(filename) = ask(screen, "Diagnostics filename", "diagnostics.json", false)?
            else {
                return Ok(None);
            };
            Command::Diagnostics {
                command: Diagnostics::Export { filename },
            }
        }
        "7" => Command::Cache {
            command: Cache::Inspect,
        },
        "8" | "9" => Command::Cache {
            command: Cache::Clean { apply: pick == "9" },
        },
        "10" => Command::HttpConfig,
        "11" => Command::HttpAuth {
            command: HttpAuth::Rotate,
        },
        "12" => Command::Plugins {
            command: Plugins::Validate,
        },
        _ => return Err(invalid("Enter a workspace number from 1 to 12.")),
    }))
}
pub async fn run(root: &Root, store: &Store, screen: &mut impl Console) -> Result<Value> {
    loop {
        preview(store, screen)?;
        screen.write("\nSelection [1] {quit=exit}: ", Style::Plain)?;
        let Some(choice) = screen.read(false)? else {
            break;
        };
        let choice = choice.trim().to_ascii_lowercase();
        if matches!(choice.as_str(), "0" | "quit" | "exit") {
            break;
        }
        let choice = if choice.is_empty() {
            "1"
        } else {
            choice.as_str()
        };
        let outcome: Result<Option<Command>> =
            async {
                Ok(match choice {
                    "1" => select(store, screen)?,
                    "2" => ask(screen, "Proxy list path", "Proxy list.txt", false)?.map(|p| {
                        Command::Proxy {
                            command: Proxy::Import {
                                file: path(root, &p),
                            },
                        }
                    }),
                    "3" => {
                        let Some(label) = ask(screen, "Profile name", "", false)? else {
                            return Ok(None);
                        };
                        let Some(url) =
                            ask(screen, "Proxy URL (hidden; encoded credentials)", "", true)?
                        else {
                            return Ok(None);
                        };
                        management::add_proxy(store, label, &url)?;
                        screen.write("Proxy saved; route unchanged.\n", Style::Success)?;
                        None
                    }
                    "4" => Some(Command::Direct),
                    "5" => Some(Command::Block),
                    "6" => session::menu(root, screen)?,
                    "7" => ask(screen, "Existing client JSON configuration path", "", false)?.map(
                        |p| Command::Register {
                            file: path(root, &p),
                        },
                    ),
                    "8" => {
                        let Some(url) = ask(
                            screen,
                            "Local proxy URL (hidden; Enter keeps current)",
                            "keep",
                            true,
                        )?
                        else {
                            return Ok(None);
                        };
                        if url != "keep" {
                            management::set_local_proxy(store, &url)?;
                        }
                        screen.write(
                            "Local proxy settings saved; use option 9 to select.\n",
                            Style::Success,
                        )?;
                        None
                    }
                    "9" => Some(Command::LocalProxy {
                        command: LocalProxy::Select,
                    }),
                    "10" => Some(Command::Http { port: None }),
                    "11" => {
                        let current = Settings::load(store)?.http_port.to_string();
                        let Some(port) = ask(screen, "Local HTTP port", &current, false)? else {
                            return Ok(None);
                        };
                        Some(Command::HttpPort {
                            port: port
                                .parse()
                                .map_err(|_| invalid("HTTP port must be 1-65535."))?,
                        })
                    }
                    "12" => Some(Command::Doctor),
                    "13" => Some(Command::Proxy {
                        command: Proxy::Test,
                    }),
                    "14" => browser(root, screen).await?,
                    "15" => workspace(screen).await?,
                    _ => return Err(invalid("Choose 1-15, or quit/exit.")),
                })
            }
            .await;
        let outcome = match outcome {
            Ok(Some(command)) => invoke(root, store, command).await.map(Some),
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        };
        match outcome {
            Ok(Some(value)) => result(screen, &value)?,
            Ok(None) => {}
            Err(error) if error.message == "Menu exit requested." => break,
            Err(error) => screen.write(
                &format!("{}: {}\n", error.code, safe_text(&error.message)),
                Style::Error,
            )?,
        }
    }
    Ok(json!({"menu_closed":true}))
}
pub async fn terminal(root: &Root, store: &Store) -> Result<Value> {
    if !std::io::stdin().is_terminal() {
        return Err(invalid(
            "Menu requires an interactive owner terminal; use preview or scriptable commands.",
        ));
    }
    run(root, store, &mut super::console::Terminal).await
}
