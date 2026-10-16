use super::{
    console::{Console, Style},
    *,
};

#[derive(Subcommand)]
pub enum Session {
    Import { file: PathBuf, username: String },
    Verify,
    Status,
    Forget,
    ExtensionPair { id: String, username: String },
    ExtensionManifest,
}
pub async fn execute(store: &Store, command: Session) -> Result<Value> {
    match command {
        Session::Import { file, username } => {
            let file = if file.is_absolute() {
                file
            } else {
                store.root.path().join(file)
            };
            crate::session::import_cookie(store, &file, &username)
        }
        Session::Verify => crate::session::verify(store).await,
        Session::Status => crate::session::status(store),
        Session::Forget => crate::session::forget(store),
        Session::ExtensionPair { id, username } => {
            crate::session::native::pair(store, &id, &username)
        }
        Session::ExtensionManifest => crate::session::native::manifest(store),
    }
}
pub fn menu(root: &Root, screen: &mut impl Console) -> Result<Option<Command>> {
    screen.write("\nFiverr session:\n", Style::Accent)?;
    for (index, label) in [
        "Import local cookie file (never paste cookies into chat)",
        "Verify imported cookie using selected route",
        "Pair logged-in browser extension",
        "Generate native host manifest",
        "Session status",
        "Forget local session (not remote logout)",
    ]
    .iter()
    .enumerate()
    {
        screen.write(&format!("  {}. ", index + 1), Style::Accent)?;
        screen.write(label, Style::Plain)?;
        screen.write("\n", Style::Plain)?;
    }
    screen.write(
        "\n  Saved cookies are not verified login; extension observations are not authentication.\n",
        Style::Warning,
    )?;
    screen.write("  back=0 | quit/exit=close\n", Style::Muted)?;
    let Some(choice) = super::menu::ask(screen, "Session action", "0", false)? else {
        return Ok(None);
    };
    let command = match choice.as_str() {
        "1" => {
            let Some(file) = super::menu::ask(screen, "Private cookie file path", "", false)?
            else {
                return Ok(None);
            };
            let Some(username) = super::menu::ask(screen, "Expected Fiverr username", "", false)?
            else {
                return Ok(None);
            };
            Session::Import {
                file: super::menu::path(root, &file),
                username,
            }
        }
        "2" => Session::Verify,
        "3" => {
            let Some(id) = super::menu::ask(screen, "Extension ID from browser", "", false)? else {
                return Ok(None);
            };
            let Some(username) = super::menu::ask(screen, "Expected Fiverr username", "", false)?
            else {
                return Ok(None);
            };
            Session::ExtensionPair { id, username }
        }
        "4" => Session::ExtensionManifest,
        "5" => Session::Status,
        "6" => Session::Forget,
        _ => return Err(invalid("Choose a session action from 1 to 6.")),
    };
    Ok(Some(Command::Session { command }))
}
