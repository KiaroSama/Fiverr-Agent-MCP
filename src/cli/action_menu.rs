use super::{
    Actions, Command,
    console::{Console, Style},
    menu::ask,
};
use crate::core::{Result, invalid};

pub(super) fn select(screen: &mut impl Console) -> Result<Option<Command>> {
    screen.write(
        "\nPrepared actions:\n  1. List\n  2. Review\n  3. Approve (owner terminal required)\n  4. Reject\n  0. Back\n",
        Style::Accent,
    )?;
    let Some(choice) = ask(screen, "Action selection", "0", false)? else {
        return Ok(None);
    };
    if choice == "0" {
        return Ok(None);
    }
    if !matches!(choice.as_str(), "1" | "2" | "3" | "4") {
        return Err(invalid("Enter an action number from 1 to 4, or back."));
    }
    let command = if choice == "1" {
        Actions::List
    } else {
        let Some(id) = ask(screen, "Prepared action ID", "", false)? else {
            return Ok(None);
        };
        if id.is_empty() {
            return Ok(None);
        }
        match choice.as_str() {
            "2" => Actions::Review { id },
            "3" => Actions::Approve { id },
            _ => Actions::Reject { id },
        }
    };
    Ok(Some(Command::Actions { command }))
}
