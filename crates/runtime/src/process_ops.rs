//! Subprocess and signal operations behind `Op::Process`.
//!
//! `run` starts exactly one granted program with an argument vector (never a shell), a cleared
//! environment (only `PATH` and granted variables), the project file root as working directory,
//! a wall-clock timeout, and a per-stream output cap. The child runs in its own process group so a
//! timeout or an output overflow kills it together with anything it spawned.
use super::{GrantHint, GrantKind, HostPolicy, Shared, SharedText, Value};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const MAX_ARGUMENTS: usize = 1024;
const MAX_ARGUMENT_BYTES: usize = 64 * 1024;

pub(crate) fn text(value: &str) -> Value {
    Value::Text(SharedText::from(value))
}

pub(crate) fn failure(code: &str, message: impl AsRef<str>) -> Value {
    Value::ResultErr(Box::new(Value::List(Shared::new(vec![
        text(code),
        text(message.as_ref()),
    ]))))
}

/// Reads up to `limit` bytes; sets `exceeded` and stops when the stream is larger.
pub(crate) fn capture<R: Read + Send + 'static>(
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
pub(crate) fn kill_group(child: &mut std::process::Child) {
    // SAFETY: signalling the child's own process group; the pid came from `Child::id`.
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
}

#[cfg(not(unix))]
pub(crate) fn kill_group(child: &mut std::process::Child) {
    windows_jobs::terminate(child.id());
    let _ = child.kill();
}

#[cfg(unix)]
pub(crate) fn exit_code(status: std::process::ExitStatus) -> i64 {
    use std::os::unix::process::ExitStatusExt;
    match status.code() {
        Some(code) => i64::from(code),
        None => 128 + i64::from(status.signal().unwrap_or(0)),
    }
}

#[cfg(not(unix))]
pub(crate) fn exit_code(status: std::process::ExitStatus) -> i64 {
    i64::from(status.code().unwrap_or(-1))
}

/// The granted command, ready to spawn: no shell, a cleared environment, the project root as the
/// working directory and its own process group. `Err` is the failure to hand back to the program.
pub(crate) fn prepare(
    policy: &HostPolicy,
    command: &str,
    arguments: &[String],
) -> Result<Command, Value> {
    let hint = GrantHint::command(command);
    if !policy.authorize(
        GrantKind::Command,
        command,
        policy.allowed_commands.contains(command),
        &hint,
    ) {
        let message = format!(
            "command `{command}` is not granted; add `[process] commands = [{command:?}]` to guion.toml or pass --allow-command {command}"
        );
        policy.record_denial("E_PROCESS_DENIED", &message, Some(&hint));
        return Err(failure("E_PROCESS_DENIED", message));
    }
    if arguments.len() > MAX_ARGUMENTS
        || arguments
            .iter()
            .any(|argument| argument.len() > MAX_ARGUMENT_BYTES || argument.contains('\0'))
    {
        return Err(failure(
            "E_PROCESS_ARGUMENT",
            "arguments must number at most 1024, be under 64 KiB each, and contain no NUL",
        ));
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
    Ok(builder)
}

pub(crate) fn run(policy: &HostPolicy, command: &str, arguments: &[String], stdin: &str) -> Value {
    let mut builder = match prepare(policy, command, arguments) {
        Ok(builder) => builder,
        Err(failure) => return failure,
    };
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
    let allowed_time = policy.cap(policy.process_timeout);
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
        if started.elapsed() >= allowed_time {
            failure_value = Some(failure(
                "E_PROCESS_TIMEOUT",
                format!(
                    "`{command}` ran longer than {} ms",
                    allowed_time.as_millis()
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

/// Set once a program polls for signals: from then on a signal is the program's to handle, and
/// ending its `together:` block (or the program) is what stops the commands it started.
static SIGNALS_POLLED: AtomicBool = AtomicBool::new(false);

/// The process groups of commands started in the background, so a signal that ends `guion`
/// stops them too instead of leaving them behind.
#[cfg(unix)]
static LIVE_GROUPS: [AtomicI32; 256] = [const { AtomicI32::new(0) }; 256];

#[cfg(unix)]
extern "C" fn stop_groups_and_exit(signal: libc::c_int) {
    for slot in &LIVE_GROUPS {
        let group = slot.load(Ordering::SeqCst);
        if group > 0 {
            // SAFETY: `kill` is async-signal-safe; the group is one this process started.
            unsafe {
                libc::kill(-group, libc::SIGKILL);
            }
        }
    }
    // SAFETY: `_exit` is async-signal-safe and skips handlers that are not.
    unsafe { libc::_exit(128 + signal) }
}

/// Remember a background command's group, and make `INT`, `TERM` and `HUP` stop every such group
/// before `guion` exits (unless the program polls for signals itself).
#[cfg(unix)]
pub(crate) fn track_group(child: &std::process::Child) {
    let group = child.id();
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(|| {
        if !SIGNALS_POLLED.load(Ordering::SeqCst) {
            for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
                // SAFETY: the handler only calls async-signal-safe functions.
                unsafe {
                    libc::signal(
                        signal,
                        stop_groups_and_exit as *const () as libc::sighandler_t,
                    );
                }
            }
        }
    });
    for slot in &LIVE_GROUPS {
        if slot
            .compare_exchange(0, group as i32, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            return;
        }
    }
}

#[cfg(unix)]
pub(crate) fn untrack_group(group: u32) {
    for slot in &LIVE_GROUPS {
        let _ = slot.compare_exchange(group as i32, 0, Ordering::SeqCst, Ordering::SeqCst);
    }
}

#[cfg(not(unix))]
pub(crate) fn track_group(child: &std::process::Child) {
    windows_jobs::adopt(child);
}

#[cfg(not(unix))]
pub(crate) fn untrack_group(group: u32) {
    windows_jobs::release(group);
}

/// On Windows a started command is put in a job object that kills everything in it when it is
/// terminated or when its handle closes, which also happens if `guion` itself dies, so a command
/// cannot leave processes behind. (A grandchild started in the instant before the command is
/// assigned to the job is not covered.)
#[cfg(not(unix))]
mod windows_jobs {
    use std::collections::HashMap;
    use std::os::windows::io::AsRawHandle;
    use std::sync::Mutex;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    static JOBS: Mutex<Option<HashMap<u32, isize>>> = Mutex::new(None);

    pub(super) fn adopt(child: &std::process::Child) {
        // SAFETY: plain Win32 calls on a handle this function creates and on the child's own
        // process handle, which `Child` keeps open for as long as it lives.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job == 0 {
                return;
            }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let configured = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                std::ptr::addr_of!(limits).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            let assigned = AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE);
            if configured == 0 || assigned == 0 {
                CloseHandle(job);
                return;
            }
            JOBS.lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get_or_insert_with(HashMap::new)
                .insert(child.id(), job);
        }
    }

    pub(super) fn terminate(pid: u32) {
        let jobs = JOBS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(job) = jobs.as_ref().and_then(|jobs| jobs.get(&pid)) {
            // SAFETY: the handle is one `adopt` made and `release` has not closed.
            unsafe {
                TerminateJobObject(*job, 1);
            }
        }
    }

    pub(super) fn release(pid: u32) {
        let job = JOBS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_mut()
            .and_then(|jobs| jobs.remove(&pid));
        if let Some(job) = job {
            // SAFETY: the handle came from `adopt` and is closed exactly once, here. Closing it
            // ends anything still running in the job.
            unsafe {
                CloseHandle(job);
            }
        }
    }
}

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
        SIGNALS_POLLED.store(true, Ordering::SeqCst);
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
