//! The one plain-language question before a run, and the same question mid-run.
//!
//! The question is a convenience for a person and never the security boundary. A person counts as
//! present only when stdin, stdout and stderr are terminals, the terminal itself can be opened and
//! the answer is read from it (not from stdin, so piped input like `yes |` cannot answer), `CI`
//! is not set, and neither `GUION_NO_PROMPT` nor `--no-prompt` is in effect.

use crate::grants::Need;
use guion_model::GrantHint;
use guion_runtime::{GrantAnswer, GrantAsker, GrantRequest};
use std::io::{BufRead, BufReader, IsTerminal, Write};
use std::path::PathBuf;
use std::sync::Mutex;

pub struct Tty {
    reader: BufReader<std::fs::File>,
    writer: Box<dyn Write + Send>,
}

/// Open the person's terminal, or `None` when nobody can be asked.
pub fn present(no_prompt_flag: bool) -> Option<Tty> {
    if no_prompt_flag
        || std::env::var_os("GUION_NO_PROMPT").is_some()
        || std::env::var_os("CI").is_some()
    {
        return None;
    }
    if !(std::io::stdin().is_terminal()
        && std::io::stdout().is_terminal()
        && std::io::stderr().is_terminal())
    {
        return None;
    }
    open_terminal()
}

#[cfg(unix)]
fn open_terminal() -> Option<Tty> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .ok()?;
    let writer = file.try_clone().ok()?;
    Some(Tty {
        reader: BufReader::new(file),
        writer: Box::new(writer),
    })
}

#[cfg(windows)]
fn open_terminal() -> Option<Tty> {
    // CONIN$ reaches the console's own input even when stdin is redirected; with no console
    // attached it cannot be opened, and then nobody is present.
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("CONIN$")
        .ok()?;
    Some(Tty {
        reader: BufReader::new(file),
        writer: Box::new(std::io::stderr()),
    })
}

#[cfg(not(any(unix, windows)))]
fn open_terminal() -> Option<Tty> {
    None
}

impl Tty {
    fn say(&mut self, text: &str) {
        let _ = self.writer.write_all(text.as_bytes());
        let _ = self.writer.flush();
    }

    fn line(&mut self) -> Option<String> {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(line),
        }
    }
}

fn parse_answer(line: Option<String>, can_save: bool) -> GrantAnswer {
    let Some(line) = line else {
        return GrantAnswer::No;
    };
    match line.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => GrantAnswer::ThisRun,
        "a" | "always" if can_save => GrantAnswer::Always,
        _ => GrantAnswer::No,
    }
}

fn options(can_save: bool) -> &'static str {
    if can_save {
        "Allow? [y] this run  [a] always (saves to guion.toml)  [N] no "
    } else {
        "Allow? [y] this run  [N] no "
    }
}

/// Ask once, before any Guion code runs, about every missing grant whose target is known.
pub fn ask_before_run(
    tty: &mut Tty,
    program: &str,
    missing: &[Need],
    can_save: bool,
) -> GrantAnswer {
    let mut text = format!("{program} wants to:\n");
    for need in missing {
        text.push_str(&format!("  · {}\n", need.phrase()));
    }
    text.push_str(options(can_save));
    tty.say(&text);
    let answer = parse_answer(tty.line(), can_save);
    if answer == GrantAnswer::No {
        tty.say("running without them\n");
    }
    answer
}

/// Asks about one target while the program runs. Questions are serialized by the runtime's broker;
/// the lock here only guards the terminal handle.
pub struct TtyAsker {
    tty: Mutex<Tty>,
    program: String,
    manifest: Option<PathBuf>,
}

impl TtyAsker {
    pub fn new(tty: Tty, program: &str, manifest: Option<PathBuf>) -> Self {
        Self {
            tty: Mutex::new(tty),
            program: program.to_owned(),
            manifest,
        }
    }
}

/// Save one grant to the manifest, telling the person what happened.
pub fn save_grant(tty: &mut Tty, manifest: &std::path::Path, grant: &GrantHint) -> bool {
    match crate::manifest_edit::add_grant(manifest, grant) {
        Ok(_) => {
            let name = manifest
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "guion.toml".to_owned());
            tty.say(&format!("saved to {name}\n"));
            true
        }
        Err(error) => {
            tty.say(&format!(
                "could not save: {error}; allowing this run only\n"
            ));
            false
        }
    }
}

impl GrantAsker for TtyAsker {
    fn ask(&self, request: &GrantRequest) -> GrantAnswer {
        let mut tty = match self.tty.lock() {
            Ok(tty) => tty,
            Err(poisoned) => poisoned.into_inner(),
        };
        let can_save = self.manifest.is_some() && request.grant.table.is_some();
        tty.say(&format!(
            "{} wants to {}\n{}",
            self.program,
            request.kind.phrase(&request.target),
            options(can_save)
        ));
        match parse_answer(tty.line(), can_save) {
            GrantAnswer::Always => {
                let manifest = self.manifest.clone().expect("can_save implies a manifest");
                if save_grant(&mut tty, &manifest, &request.grant) {
                    GrantAnswer::Always
                } else {
                    GrantAnswer::ThisRun
                }
            }
            answer => answer,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_and_anything_unrecognised_mean_no() {
        assert_eq!(parse_answer(Some("\n".into()), true), GrantAnswer::No);
        assert_eq!(parse_answer(Some("maybe\n".into()), true), GrantAnswer::No);
        assert_eq!(parse_answer(None, true), GrantAnswer::No);
        assert_eq!(parse_answer(Some("y\n".into()), true), GrantAnswer::ThisRun);
        assert_eq!(parse_answer(Some("A\n".into()), true), GrantAnswer::Always);
    }

    #[test]
    fn always_is_not_offered_or_accepted_without_a_project_file() {
        assert_eq!(parse_answer(Some("a\n".into()), false), GrantAnswer::No);
        assert!(!options(false).contains("[a]"));
    }
}
