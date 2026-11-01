use fiverr_agent_mcp::{
    cli::console::{Console, Style},
    cli::menu,
    network::Settings,
    storage::{Root, Store},
};
use std::collections::VecDeque;

#[derive(Default)]
struct Screen {
    input: VecDeque<String>,
    output: String,
    styles: Vec<Style>,
    writes: Vec<(String, Style)>,
}
impl Screen {
    fn script(input: &str) -> Self {
        Self {
            input: input.lines().map(str::to_owned).collect(),
            ..Self::default()
        }
    }
}
impl Console for Screen {
    fn write(&mut self, text: &str, style: Style) -> fiverr_agent_mcp::core::Result<()> {
        self.output.push_str(text);
        self.styles.push(style);
        self.writes.push((text.to_owned(), style));
        Ok(())
    }
    fn read(&mut self, _secret: bool) -> fiverr_agent_mcp::core::Result<Option<String>> {
        Ok(self.input.pop_front())
    }
}
fn workspace() -> (tempfile::TempDir, Root, Store) {
    let temp = tempfile::tempdir().unwrap();
    let root = Root::explicit(&temp.path().canonicalize().unwrap()).unwrap();
    let store = Store::open(root.clone()).unwrap();
    (temp, root, store)
}
#[test]
fn preview_has_reference_layout_all_management_actions_and_no_secret_values() {
    let (_temp, _root, store) = workspace();
    Settings::change(&store, false, |s| {
        s.profiles
            .push(fiverr_agent_mcp::network::profiles::Profile {
                id: "fixture".into(),
                label: "Owner proxy".into(),
                url: "socks5h://fixture-user:fixture-password@127.0.0.1:1595".into(),
            });
        Ok(())
    })
    .unwrap();
    let mut screen = Screen::default();
    menu::preview(&store, &mut screen).unwrap();
    for text in [
        "Fiverr Agent MCP",
        "Fiverr Main menu:",
        "Select proxy [1]",
        "Account status (no login required)",
        "Register MCP",
        "Local proxy settings",
        "Start local MCP HTTP",
        "Workspace tools",
        "DIRECT",
        "No login required",
        "Proxy optional",
    ] {
        assert!(screen.output.contains(text), "missing {text}");
    }
    assert!(screen.styles.contains(&Style::Accent));
    assert!(!screen.output.contains("fixture-password"));
    assert!(!screen.output.contains("fixture-user"));
    assert!(!screen.output.contains("{\""));
    assert!(!screen.output.contains("ABSOLUTELY NO WARRANTY"));
    assert!(!screen.output.contains("GPL-3.0-only"));
    assert!(!screen.output.contains("Sign in to Fiverr"));
    assert!(!screen.output.contains("Session action"));
}
#[tokio::test]
async fn menu_eof_invalid_back_and_quit_preserve_settings() {
    for input in [
        "",
        "invalid\n",
        "2\nback\nquit\n",
        "3\nback\nexit\n",
        "1\n0\n",
    ] {
        let (_temp, root, store) = workspace();
        let before = serde_json::to_value(Settings::load(&store).unwrap()).unwrap();
        let mut screen = Screen::script(input);
        menu::run(&root, &store, &mut screen).await.unwrap();
        assert_eq!(
            before,
            serde_json::to_value(Settings::load(&store).unwrap()).unwrap()
        );
        assert!(screen.output.len() < 32 * 1024);
    }
}
#[tokio::test]
async fn reference_named_import_selection_local_proxy_and_port_are_real_transactions() {
    let (_temp, root, store) = workspace();
    root.write_new(
        &root.path().join("Proxy list.txt"),
        b"socks5://127.0.0.1:1595:::Local fixture\n",
    )
    .unwrap();
    let mut screen = Screen::script("2\n\n1\n1\n8\nhttp://127.0.0.1:1596\n9\n11\n1598\n5\nquit\n");
    menu::run(&root, &store, &mut screen).await.unwrap();
    let settings = Settings::load(&store).unwrap();
    assert_eq!(settings.http_port, 1598);
    assert_eq!(settings.local_proxy, "http://127.0.0.1:1596/");
    assert_eq!(serde_json::to_value(settings.mode).unwrap(), "blocked");
    assert!(screen.output.contains("Imported: 1"));
    assert!(!screen.output.contains("fixture-password"));
}
#[tokio::test]
async fn account_status_does_not_offer_login_or_change_saved_state() {
    let (_temp, root, store) = workspace();
    let file = root.safe_path("config", "session.txt").unwrap();
    root.write_new(&file, b"session=synthetic-preserved")
        .unwrap();
    fiverr_agent_mcp::session::import_cookie(&store, &file, "fixture_owner").unwrap();
    let settings = serde_json::to_value(Settings::load(&store).unwrap()).unwrap();
    let session = fiverr_agent_mcp::session::status(&store).unwrap();
    let mut screen = Screen::script("6\n14\n2\n13\n12\nexit\n");
    menu::run(&root, &store, &mut screen).await.unwrap();
    assert_eq!(
        serde_json::to_value(Settings::load(&store).unwrap()).unwrap(),
        settings
    );
    assert_eq!(fiverr_agent_mcp::session::status(&store).unwrap(), session);
    assert!(!screen.output.contains("Logout"));
    assert!(!screen.output.contains("synthetic-preserved"));
    assert!(!screen.output.contains("Import local cookie file"));
    assert!(!screen.output.contains("ROUTE_BLOCKED"));
    assert!(!screen.output.contains("Pair logged-in browser extension"));
    assert!(screen.output.contains("Not connected"));
    assert!(!screen.output.contains("Sign-in complete"));
    assert!(screen.output.contains("Route configuration"));
}
#[tokio::test]
async fn session_menu_uses_number_label_and_warning_styles_without_changing_actions() {
    let (_temp, root, store) = workspace();
    let mut screen = Screen::script("exit\n");
    assert!(fiverr_agent_mcp::cli::session::menu(&root, &mut screen).is_err());
    let start = screen
        .writes
        .iter()
        .position(|(text, _)| text == "\nFiverr session:\n")
        .expect("session heading must be separate from its actions");
    assert_eq!(screen.writes[start].1, Style::Accent);
    let actions = [
        "Import local cookie file (never paste cookies into chat)",
        "Verify imported cookie using selected route",
        "Pair logged-in browser extension",
        "Generate native host manifest",
        "Session status",
        "Forget local session (not remote logout)",
    ];
    for (index, action) in actions.iter().enumerate() {
        let offset = start + 1 + index * 3;
        assert_eq!(
            screen.writes[offset],
            (format!("  {}. ", index + 1), Style::Accent)
        );
        assert_eq!(
            screen.writes[offset + 1],
            (action.to_string(), Style::Plain)
        );
        assert_eq!(screen.writes[offset + 2], ("\n".to_owned(), Style::Plain));
    }
    assert_eq!(
        screen.writes[start + 19],
        ("\n  Saved cookies are not verified login; extension observations are not authentication.\n".to_owned(), Style::Warning)
    );
    assert_eq!(
        screen.writes[start + 20],
        ("  back=0 | quit/exit=close\n".to_owned(), Style::Muted)
    );
    assert!(screen.output.contains("Session action [0]"));
    assert!(!screen.output.contains('\u{1b}'));
    assert!(Settings::load(&store).unwrap().profiles.is_empty());
}
#[tokio::test]
async fn all_existing_workspace_and_account_forms_preserve_authority_and_report_errors() {
    let (_temp, root, store) = workspace();
    let input = concat!(
        "3\nOwner\nsocks5h://fixture-user:fixture-password@127.0.0.1:1595\n",
        "3\nOwner\nhttp://127.0.0.1:1596\n1\n99\n",
        "4\n7\nmissing-client.json\n",
        "8\n\n8\ninvalid\n11\nnot-a-port\n11\n0\n",
        "5\n14\n2\n14\n3\nmissing-browser\n14\n4\n14\n5\n14\n99\n",
        "15\n1\n15\n2\nfixture.sqlite3\n15\n3\nfixture.sqlite3\n",
        "15\n2\nfixture.sqlite3\n3\n\nhttp://127.0.0.1:1596\n",
        "15\n4\nfixture.sqlite3\n15\n5\n15\n6\nfixture.json\n",
        "15\n7\n15\n8\n15\n9\n15\n10\n15\n11\n15\n12\n15\n99\nexit\n"
    );
    let mut screen = Screen::script(input);
    menu::run(&root, &store, &mut screen).await.unwrap();
    assert_eq!(Settings::load(&store).unwrap().profiles.len(), 1);
    assert!(Settings::load(&store).unwrap().mode == fiverr_agent_mcp::network::Mode::Blocked);
    for expected in [
        "Proxy saved",
        "already exists",
        "listed proxy number",
        "CONFIRMATION_REQUIRED",
        "HTTP port",
        "Browser configured",
        "Private output",
        "authorization token",
        "workspace number",
        "account/browser number",
    ] {
        assert!(screen.output.contains(expected), "missing {expected}");
    }
    assert!(!screen.output.contains("fixture-password"));
    assert!(!screen.output.contains("fixture-user"));
    assert!(root.path().join("exports/fixture.json").is_file());
}
#[tokio::test]
async fn form_eof_and_exit_do_not_return_to_default_action() {
    for input in [
        "2\n",
        "2\nexit\n",
        "3\nName\n",
        "8\n",
        "14\n3\nback\nquit\n",
        "15\n2\nback\nquit\n",
        "15\n6\nback\nquit\n",
    ] {
        let (_temp, root, store) = workspace();
        let mut screen = Screen::script(input);
        menu::run(&root, &store, &mut screen).await.unwrap();
        assert!(Settings::load(&store).unwrap().profiles.is_empty());
        assert!(screen.output.len() < 8 * 1024);
    }
}
#[test]
fn menu_sanitizes_control_sequences_and_preview_does_not_read() {
    assert_eq!(
        fiverr_agent_mcp::cli::console::safe_text("Owner\u{1b}[2J\n"),
        "Owner[2J"
    );
    let (_temp, _root, store) = workspace();
    let mut screen = Screen::script("unread\n");
    menu::preview(&store, &mut screen).unwrap();
    assert_eq!(screen.input.len(), 1);
}
