//! `guion repl`: an interactive prompt.
//!
//! Each entry is checked and run by the ordinary `guion check` and `guion run` commands in a
//! child process, so the permission flags mean exactly what they mean for `run`. `let`, `var`,
//! `set`, `fn`, `record`, `enum` and `import` entries are kept and rebuilt into every later
//! program, which means a kept `let` is evaluated again each time: keep entries that only
//! compute. An expression prints its value and type; any other statement runs once.

use std::{
    io::{BufRead, IsTerminal, Write},
    path::PathBuf,
    process::{Command, ExitCode},
};

const HELP: &str = "\
Type an expression to see its value and type, or a statement to run it.
let, var, set, fn, record, enum and import entries are kept for later entries
(they are evaluated again each time, so keep them free of side effects).
A line ending in `:` continues until an empty line.
:reset forgets everything kept, :help shows this, :quit leaves.";

struct Session {
    flags: Vec<String>,
    directory: PathBuf,
    declarations: Vec<String>,
    statements: Vec<String>,
    type_names: Vec<String>,
}

struct Outcome {
    success: bool,
    stdout: String,
    stderr: String,
}

impl Session {
    fn program(&self, extra: &[String]) -> String {
        let mut source = String::new();
        for declaration in &self.declarations {
            source.push_str(declaration);
            if !declaration.ends_with('\n') {
                source.push('\n');
            }
            source.push('\n');
        }
        source.push_str("fn main() -> Unit:\n");
        let mut wrote = false;
        for block in self.statements.iter().chain(extra) {
            for line in block.lines() {
                source.push_str("    ");
                source.push_str(line);
                source.push('\n');
                wrote = true;
            }
        }
        if !wrote {
            source.push_str("    print(\"\")\n");
        }
        source
    }

    fn execute(&self, command: &str, source: &str) -> Outcome {
        let file = self.directory.join("main.gu");
        if std::fs::write(&file, source).is_err() {
            return Outcome {
                success: false,
                stdout: String::new(),
                stderr: "cannot write the working program\n".into(),
            };
        }
        let Ok(executable) = std::env::current_exe() else {
            return Outcome {
                success: false,
                stdout: String::new(),
                stderr: "cannot find the guion program\n".into(),
            };
        };
        let mut child = Command::new(executable);
        child.arg(command).arg(&file);
        if command == "run" {
            child.args(&self.flags);
        }
        match child.output() {
            Ok(output) => Outcome {
                success: output.status.success(),
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            },
            Err(error) => Outcome {
                success: false,
                stdout: String::new(),
                stderr: format!("cannot start guion: {error}\n"),
            },
        }
    }

    fn check(&self, extra: &[String]) -> Outcome {
        self.execute("check", &self.program(extra))
    }

    fn run(&self, extra: &[String]) -> Outcome {
        self.execute("run", &self.program(extra))
    }

    fn candidate_types(&self) -> Vec<String> {
        let mut types: Vec<String> = ["Int", "Float64", "Text", "Bool", "Bytes"]
            .iter()
            .map(|name| (*name).to_owned())
            .collect();
        for name in &self.type_names {
            types.push(name.clone());
            types.push(format!("List[{name}]"));
        }
        for base in ["Int", "Float64", "Text", "Bool"] {
            types.push(format!("List[{base}]"));
            types.push(format!("Option[{base}]"));
        }
        types.push("Map[Text, Int]".into());
        types.push("Map[Text, Text]".into());
        types.push("List[List[Int]]".into());
        types.push("List[List[Text]]".into());
        types
    }
}

fn show(outcome: &Outcome) {
    print!("{}", outcome.stdout);
    eprint!("{}", outcome.stderr);
}

fn first_word(entry: &str) -> &str {
    entry
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .next()
        .unwrap_or("")
}

fn declared_type_name(entry: &str) -> Option<String> {
    let mut words = entry.split_whitespace();
    let mut word = words.next()?;
    if word == "pub" {
        word = words.next()?;
    }
    if word == "record" || word == "enum" {
        let name: String = words
            .next()?
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        return (!name.is_empty()).then_some(name);
    }
    None
}

pub fn run(flags: &[String]) -> Result<ExitCode, String> {
    let directory = std::env::temp_dir().join(format!("guion-repl-{}", std::process::id()));
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("cannot create the repl working directory: {error}"))?;
    let mut session = Session {
        flags: flags.to_vec(),
        directory: directory.clone(),
        declarations: Vec::new(),
        statements: Vec::new(),
        type_names: Vec::new(),
    };
    let interactive = std::io::stdin().is_terminal();
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    if interactive {
        println!("guion repl (:help for help, :quit to leave)");
    }
    loop {
        if interactive {
            print!("guion> ");
            let _ = std::io::stdout().flush();
        }
        let Some(Ok(first)) = lines.next() else {
            break;
        };
        let mut entry = first.trim_end().to_owned();
        if entry.trim().is_empty() {
            continue;
        }
        match entry.trim() {
            ":quit" | ":q" | "exit" => break,
            ":help" => {
                println!("{HELP}");
                continue;
            }
            ":reset" => {
                session.declarations.clear();
                session.statements.clear();
                session.type_names.clear();
                println!("forgot everything");
                continue;
            }
            _ => {}
        }
        if entry.trim_end().ends_with(':') {
            loop {
                if interactive {
                    print!("...... ");
                    let _ = std::io::stdout().flush();
                }
                let Some(Ok(next)) = lines.next() else {
                    break;
                };
                if next.trim().is_empty() {
                    break;
                }
                entry.push('\n');
                entry.push_str(next.trim_end());
            }
        }
        handle(&mut session, &entry);
    }
    let _ = std::fs::remove_dir_all(&directory);
    Ok(ExitCode::SUCCESS)
}

fn handle(session: &mut Session, entry: &str) {
    let word = first_word(entry.trim_start());
    match word {
        "fn" | "record" | "enum" | "pub" | "import" => {
            session.declarations.push(entry.to_owned());
            let outcome = session.check(&[]);
            if outcome.success {
                if let Some(name) = declared_type_name(entry) {
                    session.type_names.push(name);
                }
                println!("ok");
            } else {
                session.declarations.pop();
                show(&outcome);
            }
        }
        "let" | "var" | "set" => {
            session.statements.push(entry.to_owned());
            let outcome = session.check(&[]);
            if outcome.success {
                println!("ok");
            } else {
                session.statements.pop();
                show(&outcome);
            }
        }
        "if" | "while" | "for" | "match" | "break" | "continue" | "return" => {
            show(&session.run(&[entry.to_owned()]));
        }
        _ => evaluate_expression(session, entry),
    }
}

fn evaluate_expression(session: &Session, entry: &str) {
    let probe = format!("let __repl = {entry}");
    let outcome = session.check(std::slice::from_ref(&probe));
    if !outcome.success {
        // Not a value: it may be a call that returns nothing, such as `print("hi")`.
        if session.check(&[entry.to_owned()]).success {
            show(&session.run(&[entry.to_owned()]));
        } else {
            show(&outcome);
        }
        return;
    }
    for candidate in session.candidate_types() {
        let typed = format!("let __repl: {candidate} = {entry}");
        if session.check(std::slice::from_ref(&typed)).success {
            let result = session.run(&[typed, "print(__repl)".to_owned()]);
            if result.success {
                let value = result.stdout.trim_end_matches('\n');
                println!("{value} : {candidate}");
            } else {
                show(&result);
            }
            return;
        }
    }
    // No candidate type fits: a call that returns nothing runs as a statement; any other
    // type prints its value without a name.
    if session.check(&[entry.to_owned()]).success {
        show(&session.run(&[entry.to_owned()]));
    } else {
        show(&session.run(&[probe, "print(__repl)".to_owned()]));
    }
}
