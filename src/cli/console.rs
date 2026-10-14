use crate::core::{Result, invalid};
use std::io::{BufRead, IsTerminal, Read, Write};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Plain,
    Accent,
    Success,
    Warning,
    Error,
    Muted,
}
pub trait Console {
    fn write(&mut self, text: &str, style: Style) -> Result<()>;
    fn read(&mut self, secret: bool) -> Result<Option<String>>;
}
pub struct Terminal;
impl Console for Terminal {
    fn write(&mut self, text: &str, style: Style) -> Result<()> {
        let color = std::io::stdout().is_terminal()
            && std::env::var_os("NO_COLOR").is_none()
            && std::env::var("TERM").ok().as_deref() != Some("dumb")
            && std::env::var("CI").ok().as_deref() != Some("true");
        let code = match style {
            Style::Accent => "1;38;2;155;225;255",
            Style::Success => "92",
            Style::Warning => "93",
            Style::Error => "91",
            Style::Muted => "90",
            Style::Plain => "0",
        };
        let mut output = std::io::stdout().lock();
        if color {
            write!(output, "\x1b[{code}m{text}\x1b[0m")
        } else {
            write!(output, "{text}")
        }
        .and_then(|_| output.flush())
        .map_err(|_| invalid("Cannot write menu output."))
    }
    fn read(&mut self, secret: bool) -> Result<Option<String>> {
        if secret {
            if !std::io::stdin().is_terminal() {
                return Err(invalid(
                    "Hidden input requires an owner terminal; use the file-based CLI for automation.",
                ));
            }
            return rpassword::read_password()
                .map(Some)
                .map_err(|_| invalid("Cannot read hidden input."));
        }
        let mut line = String::new();
        let count = std::io::stdin()
            .lock()
            .take(4097)
            .read_line(&mut line)
            .map_err(|_| invalid("Cannot read menu input."))?;
        if line.len() > 4096 {
            return Err(invalid("Menu input exceeds 4096 bytes."));
        }
        Ok((count != 0).then(|| line.trim_end_matches(['\r', '\n']).to_owned()))
    }
}
pub fn safe_text(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(256).collect()
}
