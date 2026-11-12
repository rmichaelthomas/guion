//! Helpers shared by the integration tests that start a `guion` server on a local port.
//!
//! Several tests in one binary run side by side on threads. Two things go wrong when they create
//! sockets and start processes at the same time, and both showed as a server that could not bind
//! its port ("Address already in use") or a connection that was reset on a loaded machine:
//!
//! * On macOS the standard library creates a socket and only then marks it close-on-exec, so a
//!   process started on another thread in between inherits it. A port probe, or a client socket,
//!   then stays open inside an unrelated `guion` process until that process ends, and the port
//!   cannot be bound while it does. Every socket creation and every process start below holds one
//!   lock, so none of them overlaps another.
//! * A port taken from `bind(0)` is in the ephemeral range, from which the kernel also picks the
//!   source port of each outgoing connection, so a client retrying a connection could be handed
//!   the port a server was about to bind. The ports here are below that range.

#![allow(dead_code)]

use std::io;
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Mutex;

/// The lowest port handed out. It sits below every operating system's ephemeral range
/// (49152 and up on macOS and Windows, 32768 and up on Linux).
const FIRST: u16 = 20_000;
const SPAN: u16 = 10_000;

static NEXT: AtomicU16 = AtomicU16::new(0);
static SOCKETS_AND_PROCESSES: Mutex<()> = Mutex::new(());

fn exclusive() -> std::sync::MutexGuard<'static, ()> {
    SOCKETS_AND_PROCESSES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A local port that nothing is listening on, spread by process id so test binaries running side
/// by side do not start from the same port, and checked to be free when it is chosen.
pub fn free_port() -> u16 {
    let _exclusive = exclusive();
    let start = (std::process::id() % u32::from(SPAN)) as u16;
    for _ in 0..SPAN {
        let step = NEXT.fetch_add(1, Ordering::Relaxed);
        let port = FIRST + (start.wrapping_add(step.wrapping_mul(7))) % SPAN;
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
    panic!("no free port between {FIRST} and {}", FIRST + SPAN);
}

/// Opens a connection to a local port.
pub fn connect_to(port: u16) -> io::Result<TcpStream> {
    let _exclusive = exclusive();
    TcpStream::connect(("127.0.0.1", port))
}

/// Starts a process without letting it inherit a socket another thread is creating.
pub fn spawn(command: &mut Command) -> io::Result<Child> {
    let _exclusive = exclusive();
    command.spawn()
}

/// Runs a process to completion, capturing its output, like `Command::output`.
pub fn output(command: &mut Command) -> io::Result<Output> {
    let child = spawn(
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )?;
    child.wait_with_output()
}
