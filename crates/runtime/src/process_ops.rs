//! Subprocess and signal operations behind `Op::Process`.
//!
//! `run` starts exactly one granted program with an argument vector (never a shell), a cleared
//! environment (only `PATH` and granted variables), the project file root as working directory,
//! a wall-clock timeout, and a per-stream output cap. The child runs in its own process group so a
//! timeout or an output overflow kills it together with anything it spawned.
use super::{HostPolicy, Shared, SharedText, Value};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const MAX_ARGUMENTS: usize = 1024;
const MAX_ARGUMENT_BYTES: usize = 64 * 1024;

fn text(value: &str) -> Value {
    Value::Text(SharedText::from(value))
}

fn failure(code: &str, message: impl AsRef<str>) -> Value {
    Value::ResultErr(Box::new(Value::List(Shared::new(vec![
        text(code),
        text(message.as_ref()),
    ]))))
}

/// Reads up to `limit` bytes; sets `exceeded` and stops when the stream is larger.
fn capture<R: Read + Send + 'static>(
    mut stream: R,
    limit: usize,
    exceeded: Arc<AtomicBool>,
) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut collected = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    if collected.len() + count > limit {
                        exceeded.store(true, Ordering::SeqCst);
                        break;
                    }
                    collected.extend_from_slice(&buffer[..count]);
                }
            }
        }
        collected
    })
}

#[cfg(unix)]
fn kill_group(child: &mut std::process::Child) {
    // SAFETY: signalling the child's own process group; the pid came from `Child::id`.
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
}

#[cfg(not(unix))]
fn kill_group(child: &mut std::process::Child) {
    let _ = child.kill();
}

#[cfg(unix)]
fn exit_code(status: std::process::ExitStatus) -> i64 {
    use std::os::unix::process::ExitStatusExt;
    match status.code() {
        Some(code) => i64::from(code),
        None => 128 + i64::from(status.signal().unwrap_or(0)),
    }
}

#[cfg(not(unix))]
fn exit_code(status: std::process::ExitStatus) -> i64 {
    i64::from(status.code().unwrap_or(-1))
}

pub(crate) fn run(policy: &HostPolicy, command: &str, arguments: &[String], stdin: &str) -> Value {
    if !policy.allowed_commands.contains(command) {
        return failure(
            "E_PROCESS_DENIED",
            format!("command `{command}` is not granted; add it to guion.toml [process].commands or pass --allow-command"),
        );
    }
    if arguments.len() > MAX_ARGUMENTS
        || arguments
            .iter()
            .any(|argument| argument.len() > MAX_ARGUMENT_BYTES || argument.contains('\0'))
    {
        return failure(
            "E_PROCESS_ARGUMENT",
            "arguments must number at most 1024, be under 64 KiB each, and contain no NUL",
        );
    }
    let mut builder = Command::new(command);
    builder
        .args(arguments)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = std::env::var_os("PATH") {
        builder.env("PATH", path);
    }
    for name in &policy.allowed_environment_variables {
        if let Some(value) = std::env::var_os(name) {
            builder.env(name, value);
        }
    }
    if let Some(root) = &policy.file_root {
        builder.current_dir(root);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        builder.process_group(0);
    }
    let mut child = match builder.spawn() {
        Ok(child) => child,
        Err(error) => {
            return failure(
                "E_PROCESS_SPAWN",
                format!("cannot start `{command}`: {error}"),
            )
        }
    };
    let limit = policy.max_process_output_bytes;
    let exceeded = Arc::new(AtomicBool::new(false));
    let out_reader = child
        .stdout
        .take()
        .map(|stream| capture(stream, limit, exceeded.clone()));
    let err_reader = child
        .stderr
        .take()
        .map(|stream| capture(stream, limit, exceeded.clone()));
    if let Some(mut pipe) = child.stdin.take() {
        let input = stdin.as_bytes().to_vec();
        thread::spawn(move || {
            let _ = pipe.write_all(&input);
        });
    }
    let started = Instant::now();
    let mut failure_value = None;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(error) => {
                failure_value = Some(failure("E_PROCESS_WAIT", error.to_string()));
                break None;
            }
        }
        if exceeded.load(Ordering::SeqCst) {
            failure_value = Some(failure(
                "E_PROCESS_OUTPUT_LIMIT",
                format!("`{command}` wrote more than {limit} bytes to one stream"),
            ));
            break None;
        }
        if started.elapsed() >= policy.process_timeout {
            failure_value = Some(failure(
                "E_PROCESS_TIMEOUT",
                format!(
                    "`{command}` ran longer than {} ms",
                    policy.process_timeout.as_millis()
                ),
            ));
            break None;
        }
        thread::sleep(Duration::from_millis(2));
    };
    if status.is_none() {
        kill_group(&mut child);
        let _ = child.wait();
    }
    let stdout = out_reader
        .map(|reader| reader.join().unwrap_or_default())
        .unwrap_or_default();
    let stderr = err_reader
        .map(|reader| reader.join().unwrap_or_default())
        .unwrap_or_default();
    if let Some(value) = failure_value {
        return value;
    }
    if exceeded.load(Ordering::SeqCst) {
        return failure(
            "E_PROCESS_OUTPUT_LIMIT",
            format!("`{command}` wrote more than {limit} bytes to one stream"),
        );
    }
    let code = exit_code(status.expect("a finished child has a status"));
    Value::ResultOk(Box::new(Value::List(Shared::new(vec![
        text(&code.to_string()),
        text(&String::from_utf8_lossy(&stdout)),
        text(&String::from_utf8_lossy(&stderr)),
    ]))))
}

/// The pending-signal slot: 0 when none, otherwise the signal number.
static PENDING_SIGNAL: AtomicI32 = AtomicI32::new(0);

#[cfg(unix)]
extern "C" fn record_signal(signal: libc::c_int) {
    PENDING_SIGNAL.store(signal, Ordering::SeqCst);
}

/// Installs handlers for `INT`, `TERM` and `HUP` the first time a program polls, then returns
/// and clears the pending signal name (`""` when none).
#[cfg(unix)]
pub(crate) fn signal_poll() -> &'static str {
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(|| {
        for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            // SAFETY: the handler only stores into an atomic, which is async-signal-safe.
            unsafe {
                libc::signal(signal, record_signal as *const () as libc::sighandler_t);
            }
        }
    });
    match PENDING_SIGNAL.swap(0, Ordering::SeqCst) {
        0 => "",
        signal if signal == libc::SIGINT => "INT",
        signal if signal == libc::SIGTERM => "TERM",
        signal if signal == libc::SIGHUP => "HUP",
        _ => "",
    }
}

#[cfg(not(unix))]
pub(crate) fn signal_poll() -> &'static str {
    ""
}
