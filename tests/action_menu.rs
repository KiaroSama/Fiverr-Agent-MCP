use fiverr_agent_mcp::{
    cli::{
        console::{Console, Style},
        menu,
    },
    core::Result,
    service::actions,
    storage::{Root, Store},
};
use serde_json::json;
use std::collections::VecDeque;

struct Screen {
    input: VecDeque<String>,
    output: String,
}
impl Screen {
    fn new(input: &str) -> Self {
        Self {
            input: input.lines().map(str::to_owned).collect(),
            output: String::new(),
        }
    }
}
impl Console for Screen {
    fn write(&mut self, text: &str, _: Style) -> Result<()> {
        self.output.push_str(text);
        Ok(())
    }
    fn read(&mut self, _: bool) -> Result<Option<String>> {
        Ok(self.input.pop_front())
    }
}

#[tokio::test]
async fn action_menu_reuses_existing_owner_review_and_rejection() {
    let temp = tempfile::tempdir().unwrap();
    let root = Root::explicit(&temp.path().canonicalize().unwrap()).unwrap();
    let store = Store::open(root.clone()).unwrap();
    let proposal = actions::prepare(
        &store,
        "note",
        &json!({"target_id":"local-fixture","payload":{"body":"Exact owner proposal"}}),
    )
    .unwrap();
    let id = proposal["data"]["id"].as_str().unwrap();
    let mut screen = Screen::new(&format!("15\n1\n2\n{id}\n15\n1\n4\n{id}\n0\n"));
    menu::run(&root, &store, &mut screen).await.unwrap();
    assert!(
        screen.output.contains("Exact owner proposal"),
        "Review must show the proposal, not only its id"
    );
    assert_eq!(
        actions::get(&store, id).unwrap()["data"]["state"],
        "cancelled"
    );
    assert!(!screen.output.contains("remote_executed\": true"));
}

#[tokio::test]
async fn action_menu_never_approves_on_default_or_cancel() {
    let temp = tempfile::tempdir().unwrap();
    let root = Root::explicit(&temp.path().canonicalize().unwrap()).unwrap();
    let store = Store::open(root.clone()).unwrap();
    let proposal = actions::prepare(
        &store,
        "note",
        &json!({"target_id":"local-fixture","payload":{"body":"Keep prepared"}}),
    )
    .unwrap();
    let id = proposal["data"]["id"].as_str().unwrap();
    for input in [
        "15\n1\n".to_owned(),
        "15\n1\n\n0\n".to_owned(),
        "15\n1\nback\n0\n".to_owned(),
        format!("15\n1\n3\n{id}\n0\n"),
    ] {
        let mut screen = Screen::new(&input);
        menu::run(&root, &store, &mut screen).await.unwrap();
        assert!(
            screen.output.contains("3. Approve"),
            "Prepared actions must expose the owner submenu"
        );
        assert_eq!(
            actions::get(&store, id).unwrap()["data"]["state"],
            "prepared"
        );
    }
}
