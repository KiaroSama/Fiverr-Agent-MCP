#![cfg(target_os = "linux")]
use fiverr_agent_mcp::{
    bootstrap,
    network::{Mode, Settings},
    storage::{Root, Store},
};
use serde_json::Value;
use std::{
    io::{Read, Write},
    os::unix::process::CommandExt,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

// A real PTY exercises stdin ownership and echo-off input, not a fake Console.
struct Menu {
    child: Child,
    session_pid: Option<u32>,
    completed: bool,
    output: Receiver<Vec<u8>>,
    reader: Option<std::thread::JoinHandle<()>>,
    text: String,
    cursor: usize,
    deadline: Instant,
}
impl Menu {
    fn start(root: &Root) -> Self {
        let quote = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
        let command = format!(
            "exec {} --root {} menu",
            quote(env!("CARGO_BIN_EXE_fiverr-agent-mcp")),
            quote(root.path().to_str().unwrap())
        );
        let mut child = Command::new("/usr/bin/script")
            .args(["-q", "-e", "--echo", "always", "-c", &command, "/dev/null"])
            .envs(bootstrap::environment(root))
            .env("TERM", "dumb")
            .env("NO_COLOR", "1")
            .process_group(0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let (tx, output) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut buffer = [0; 4096];
            while let Ok(n) = stdout.read(&mut buffer) {
                if n == 0 || tx.send(buffer[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            session_pid: None,
            completed: false,
            output,
            reader: Some(reader),
            text: String::new(),
            cursor: 0,
            deadline: Instant::now() + Duration::from_secs(15),
        }
    }
    fn expect(&mut self, expected: &str) {
        loop {
            if let Some(offset) = self.text[self.cursor..].find(expected) {
                self.cursor += offset + expected.len();
                return;
            }
            let remaining = self
                .deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_secs(3));
            let bytes = self
                .output
                .recv_timeout(remaining)
                .unwrap_or_else(|e| panic!("PTY missing {expected:?}: {e}; output: {}", self.text));
            // The fixture's terminal vocabulary/input is ASCII; no split UTF-8 decoding.
            assert!(bytes.is_ascii());
            self.text.push_str(std::str::from_utf8(&bytes).unwrap());
            assert!(self.text.len() < 256 * 1024, "Unbounded menu output");
        }
    }
    fn send(&mut self, prompt: &str, input: &str) {
        self.expect(prompt);
        if prompt.contains("(hidden") {
            self.wait_for_hidden_input();
        }
        writeln!(self.child.stdin.as_mut().unwrap(), "{input}").unwrap();
        self.child.stdin.as_mut().unwrap().flush().unwrap();
    }
    fn wait_for_hidden_input(&self) {
        // The prompt is flushed before rpassword disables ECHO. Observe termios,
        // not an arbitrary delay, before forwarding synthetic credentials.
        let device = format!("/proc/{}/fd/0", self.session_pid.unwrap());
        let deadline = self.deadline.min(Instant::now() + Duration::from_secs(2));
        loop {
            let mut check = Command::new("/bin/stty")
                .args(["-F", &device, "-a"])
                .env("LC_ALL", "C")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let status = loop {
                if let Some(status) = check.try_wait().unwrap() {
                    break status;
                }
                if Instant::now() >= deadline {
                    let _ = check.kill();
                    let _ = check.wait();
                    panic!("PTY echo inspection deadline exceeded");
                }
                std::thread::sleep(Duration::from_millis(2));
            };
            let mut output = String::new();
            check
                .stdout
                .take()
                .unwrap()
                .read_to_string(&mut output)
                .unwrap();
            assert!(status.success(), "Cannot inspect owned PTY echo state");
            if output
                .split_whitespace()
                .any(|flag| flag.trim_end_matches(';') == "-echo")
            {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "Hidden input did not disable echo"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    fn choose(&mut self, value: &str) {
        self.expect("Selection [1] {quit=exit}: ");
        if self.session_pid.is_none() {
            let children = std::fs::read_to_string(format!(
                "/proc/{}/task/{}/children",
                self.child.id(),
                self.child.id()
            ))
            .unwrap();
            let children: Vec<u32> = children
                .split_whitespace()
                .map(|id| id.parse().unwrap())
                .collect();
            assert_eq!(children.len(), 1, "Expected one owned PTY session leader");
            self.session_pid = Some(children[0]);
        }
        writeln!(self.child.stdin.as_mut().unwrap(), "{value}").unwrap();
        self.child.stdin.as_mut().unwrap().flush().unwrap();
    }
    fn finish(mut self) -> String {
        self.choose("exit");
        self.child.stdin.take();
        self.expect("\"menu_closed\": true");
        // Pipe EOF and child exit are separate signals; both must arrive before the deadline.
        loop {
            assert!(
                Instant::now() < self.deadline,
                "Menu output exceeded wall deadline"
            );
            let remaining = self.deadline.saturating_duration_since(Instant::now());
            match self.output.recv_timeout(remaining) {
                Ok(bytes) => {
                    self.text.push_str(std::str::from_utf8(&bytes).unwrap());
                    assert!(self.text.len() < 256 * 1024, "Unbounded exit output");
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => panic!("Menu output did not close"),
            }
        }
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "Menu did not exit successfully");
                break;
            }
            assert!(
                Instant::now() < self.deadline,
                "Menu exit deadline exceeded"
            );
            // The process handle has no blocking timed wait in std; bounded exit polling only.
            std::thread::sleep(Duration::from_millis(5));
        }
        self.completed = true;
        self.text.clone()
    }
}
impl Drop for Menu {
    fn drop(&mut self) {
        if self.completed {
            if let Some(reader) = self.reader.take() {
                let _ = reader.join();
            }
            return;
        }
        // script creates a separate session for its shell; reap both owned groups.
        // The parent remains unreaped here, preventing reuse of its identity during cleanup.
        let session = self.session_pid.or_else(|| {
            std::fs::read_to_string(format!(
                "/proc/{}/task/{}/children",
                self.child.id(),
                self.child.id()
            ))
            .ok()?
            .split_whitespace()
            .next()?
            .parse::<u32>()
            .ok()
        });
        for pid in session.into_iter().chain(std::iter::once(self.child.id())) {
            let _ = Command::new("/bin/kill")
                .args(["-KILL", "--", &format!("-{pid}")])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
fn state(store: &Store) -> Settings {
    Settings::load(store).unwrap()
}

#[test]
fn actual_menu_exercises_every_main_option_with_hidden_input_consent_and_failures() {
    let temp = tempfile::Builder::new()
        .prefix("practical menu space-")
        .tempdir()
        .unwrap();
    let root = Root::explicit(temp.path()).unwrap();
    let store = Store::open(root.clone()).unwrap();
    root.write_new(
        &root.path().join("Proxy list.txt"),
        b"http://127.0.0.1:1595:::Imported fixture\n",
    )
    .unwrap();
    let client = root.path().join("config/client.json");
    root.write_new(&client, br#"{"theme":"keep"}"#).unwrap();
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = occupied.local_addr().unwrap().port();
    let mut menu = Menu::start(&root);
    menu.choose("1");
    menu.expect("No saved proxies");
    menu.choose("2");
    menu.send(
        "Proxy list path [Proxy list.txt] {back=0, quit=exit}: ",
        &format!("\"{}\"", root.path().join("Proxy list.txt").display()),
    );
    menu.expect("Imported: 1");
    assert_eq!(state(&store).profiles.len(), 1);
    menu.choose("3");
    menu.send("Profile name [] {back=0, quit=exit}: ", "Hidden fixture");
    menu.send(
        "Proxy URL (hidden; encoded credentials) [] {back=0, quit=exit}: ",
        "socks5h://fixture-user:fixture-password@127.0.0.1:1596",
    );
    menu.expect("Proxy saved");
    assert_eq!(state(&store).profiles.len(), 2);
    menu.choose("1");
    menu.send("Proxy number [0] {back=0, quit=exit}: ", "2");
    menu.expect("Operation complete");
    assert!(state(&store).mode == Mode::Proxy);
    menu.choose("4");
    menu.send("[y/N]: ", "n");
    menu.expect("Owner declined");
    assert!(state(&store).mode == Mode::Proxy);
    menu.choose("4");
    menu.send("[y/N]: ", "y");
    menu.expect("Operation complete");
    assert!(state(&store).mode == Mode::Direct);
    menu.choose("5");
    menu.expect("Operation complete");
    assert!(state(&store).mode == Mode::Blocked);
    let cookie = root.safe_path("config", "session-input.txt").unwrap();
    root.write_new(&cookie, b"session=synthetic-menu-private")
        .unwrap();
    menu.choose("6");
    menu.send("Session action [0] {back=0, quit=exit}: ", "1");
    menu.send(
        "Private cookie file path [] {back=0, quit=exit}: ",
        "config/session-input.txt",
    );
    menu.send(
        "Expected Fiverr username [] {back=0, quit=exit}: ",
        "fixture_owner",
    );
    menu.expect("Session: credential_saved");
    menu.choose("6");
    menu.send("Session action [0] {back=0, quit=exit}: ", "2");
    menu.expect("ROUTE_BLOCKED");
    menu.choose("6");
    menu.send("Session action [0] {back=0, quit=exit}: ", "3");
    menu.send(
        "Extension ID from browser [] {back=0, quit=exit}: ",
        "abcdefghijklmnopabcdefghijklmnop",
    );
    menu.send(
        "Expected Fiverr username [] {back=0, quit=exit}: ",
        "fixture_owner",
    );
    menu.expect("Session: extension_paired");
    menu.choose("6");
    menu.send("Session action [0] {back=0, quit=exit}: ", "4");
    menu.expect("FILE_POLICY_VIOLATION"); // test binary is outside this temporary installation
    menu.choose("6");
    menu.send("Session action [0] {back=0, quit=exit}: ", "5");
    menu.expect("Session: extension_paired");
    menu.choose("6");
    menu.send("Session action [0] {back=0, quit=exit}: ", "6");
    menu.expect("Session: not_configured");
    assert!(
        !fiverr_agent_mcp::session::status(&store).unwrap()["configured"]
            .as_bool()
            .unwrap()
    );
    menu.choose("7");
    menu.send(
        "Existing client JSON configuration path [] {back=0, quit=exit}: ",
        client.to_str().unwrap(),
    );
    menu.send("[y/N]: ", "y");
    menu.expect("Operation complete");
    let configured: Value = serde_json::from_slice(&std::fs::read(&client).unwrap()).unwrap();
    assert_eq!(configured["theme"], "keep");
    assert!(configured["mcpServers"]["Fiverr-Agent-mcp"].is_object());
    menu.choose("8");
    menu.send(
        "Local proxy URL (hidden; Enter keeps current) [keep] {back=0, quit=exit}: ",
        "http://127.0.0.1:1599",
    );
    menu.expect("Local proxy settings saved");
    assert!(state(&store).mode == Mode::Blocked);
    menu.choose("9");
    menu.expect("Operation complete");
    assert!(state(&store).mode == Mode::Proxy);
    menu.choose("11");
    menu.send(
        "Local HTTP port [1597] {back=0, quit=exit}: ",
        &port.to_string(),
    );
    menu.expect(&format!("HTTP port: {port}"));
    assert_eq!(state(&store).http_port, port);
    menu.choose("10");
    menu.expect("PORT_IN_USE"); // controlled occupied listener, not an orphaned server
    menu.choose("12");
    menu.expect("Local runtime available");
    menu.choose("13");
    menu.expect("Route configuration: valid");
    menu.choose("14");
    menu.send("Selection [0] {back=0, quit=exit}: ", "2");
    menu.expect("Not connected");
    menu.choose("15");
    menu.send("Selection [0] {back=0, quit=exit}: ", "1");
    menu.expect("0 item(s)");
    menu.choose("2");
    menu.send(
        "Proxy list path [Proxy list.txt] {back=0, quit=exit}: ",
        "back",
    );
    menu.choose("11");
    menu.send(
        &format!("Local HTTP port [{port}] {{back=0, quit=exit}}: "),
        "invalid",
    );
    menu.expect("HTTP port must be");
    drop(occupied);
    menu.choose("10");
    menu.expect(&format!(
        "Local MCP HTTP listening at http://127.0.0.1:{port}/mcp"
    ));
    let mut health = std::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).unwrap();
    health
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    health
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write!(
        health,
        "GET /healthz HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    health.read_to_string(&mut response).unwrap();
    assert!(
        response.starts_with("HTTP/1.1 401"),
        "Token guard did not protect actual listener"
    );
    drop(health);
    // PTY-generated SIGINT exercises the real listener's graceful shutdown path.
    menu.child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"\x03")
        .unwrap();
    menu.child.stdin.as_mut().unwrap().flush().unwrap();
    let output = menu.finish();
    assert!(std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).is_ok());
    assert!(!output.contains("fixture-password"));
    assert!(!output.contains("fixture-user"));
    assert!(!output.contains("ABSOLUTELY NO WARRANTY"));
    assert!(!output.contains("Sign-in complete"));
}

#[test]
fn actual_menu_exercises_all_account_and_workspace_submenu_operations() {
    let temp = tempfile::tempdir().unwrap();
    let root = Root::explicit(temp.path()).unwrap();
    let store = Store::open(root.clone()).unwrap();
    let mut menu = Menu::start(&root);
    for (choice, expected) in [
        ("2", "Not connected"),
        ("4", "Browser configured: false"),
        ("5", "Manual handoff only"),
    ] {
        menu.choose("14");
        menu.send("Selection [0] {back=0, quit=exit}: ", choice);
        menu.expect(expected);
    }
    menu.choose("14");
    menu.send("Selection [0] {back=0, quit=exit}: ", "1");
    menu.send("Session action [0] {back=0, quit=exit}: ", "back");
    menu.choose("14");
    menu.send("Selection [0] {back=0, quit=exit}: ", "3");
    menu.send(
        "Browser executable path [] {back=0, quit=exit}: ",
        "missing-browser",
    );
    menu.expect("existing absolute path");
    menu.choose("14");
    menu.send("Selection [0] {back=0, quit=exit}: ", "3");
    let browser = root.path().join("fixture-browser");
    root.write_new(&browser, b"configuration-only fixture; never executed")
        .unwrap();
    menu.send(
        "Browser executable path [] {back=0, quit=exit}: ",
        browser.to_str().unwrap(),
    );
    menu.expect("Operation complete");
    assert_eq!(
        state(&store).browser_executable.as_deref(),
        browser.to_str()
    ); // configuration only; never execute fixture bytes
    for choice in ["2", "3", "4"] {
        menu.choose("15");
        menu.send("Selection [0] {back=0, quit=exit}: ", choice);
        menu.send(
            "Backup filename in exports [workspace.sqlite3] {back=0, quit=exit}: ",
            "fixture.sqlite3",
        );
        match choice {
            "2" => {
                menu.expect("Private output: fixture.sqlite3");
                assert!(root.path().join("exports/fixture.sqlite3").is_file());
            }
            "3" => menu.expect("Operation complete"),
            _ => {
                menu.send("[y/N]: ", "y");
                menu.expect("Operation complete");
                assert!(state(&store).mode == Mode::Blocked);
                assert!(state(&store).browser_executable.is_none());
            }
        }
    }
    for (choice, expected) in [
        ("5", "item(s)"),
        ("7", "0 item(s)"),
        ("8", "Operation complete"),
        ("9", "Automatic recursive cleanup is disabled"),
        ("10", "Local HTTP endpoint"),
        ("11", "Operation complete"),
        ("12", "Operation complete"),
    ] {
        menu.choose("15");
        menu.send("Selection [0] {back=0, quit=exit}: ", choice);
        menu.expect(expected);
    }
    menu.choose("15");
    menu.send("Selection [0] {back=0, quit=exit}: ", "6");
    menu.send(
        "Diagnostics filename [diagnostics.json] {back=0, quit=exit}: ",
        "fixture.json",
    );
    menu.expect("Private output: fixture.json");
    let diagnostics: Value =
        serde_json::from_slice(&std::fs::read(root.path().join("exports/fixture.json")).unwrap())
            .unwrap();
    assert_eq!(diagnostics["private_content_excluded"], true);
    menu.choose("15");
    menu.send("Selection [0] {back=0, quit=exit}: ", "1");
    menu.expect("0 item(s)");
    menu.finish();
}
