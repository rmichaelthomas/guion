//! Commands started in the background by `start_process`.
//!
//! A started command runs without a shell, with the grants `process_run` needs, in its own process
//! group. The program reads its output a line at a time, writes to its input, and waits for it to
//! exit. When the `together:` block that started it ends, or the program does, or `guion` is sent
//! `INT`, `TERM` or `HUP`, the whole group is stopped and reaped, so a sub-agent cannot leave
//! orphans behind. See `docs/design-structured-concurrency.md`.

use super::*;
use crate::process_ops::{capture, exit_code, failure, kill_group, prepare, text, track_group};
use std::io::{Read, Write};

/// One started command.
struct Started {
    child: std::process::Child,
    stdin: Option<std::process::ChildStdin>,
    /// Output lines as the command prints them; closed when its output ends.
    lines: mpsc::Receiver<Vec<u8>>,
    /// Set when the command wrote more than the per-process output limit.
    exceeded: Arc<AtomicBool>,
    stderr: Option<thread::JoinHandle<Vec<u8>>>,
    command: String,
    limit: usize,
    /// What `process_wait` returned, kept so a second call gives the same answer.
    finished: Option<Value>,
}

impl Started {
    /// Stop the command and everything it started, and collect it.
    fn stop(&mut self) {
        if self.finished.is_none() {
            kill_group(&mut self.child);
            let _ = self.child.wait();
            crate::process_ops::untrack_group(self.child.id());
            self.finished = Some(Value::Unit);
        }
        self.stdin.take();
        if let Some(reader) = self.stderr.take() {
            let _ = reader.join();
        }
    }
}

/// Every command an instance started; a handle is an index here.
#[derive(Default)]
pub(crate) struct ProcessTable {
    slots: Vec<Started>,
}

impl ProcessTable {
    pub(crate) fn len(&self) -> usize {
        self.slots.len()
    }

    /// Stop every command started at or after `first`.
    pub(crate) fn stop_from(&mut self, first: usize) {
        for slot in self.slots.iter_mut().skip(first) {
            slot.stop();
        }
    }
}

impl std::fmt::Debug for ProcessTable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "ProcessTable({} started)", self.slots.len())
    }
}

impl Drop for ProcessTable {
    fn drop(&mut self) {
        self.stop_from(0);
    }
}

/// Splits a command's output into lines on a thread of its own, so reading one never blocks the
/// program on the command's other streams.
fn read_lines<R: Read + Send + 'static>(
    mut stream: R,
    limit: usize,
    exceeded: Arc<AtomicBool>,
) -> mpsc::Receiver<Vec<u8>> {
    let (sender, lines) = mpsc::channel();
    thread::spawn(move || {
        let mut pending = Vec::new();
        let mut total = 0usize;
        let mut buffer = [0u8; 8192];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    total += count;
                    if total > limit {
                        exceeded.store(true, Ordering::SeqCst);
                        return;
                    }
                    pending.extend_from_slice(&buffer[..count]);
                    while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
                        let mut line: Vec<u8> = pending.drain(..=end).collect();
                        line.pop();
                        if line.last() == Some(&b'\r') {
                            line.pop();
                        }
                        if sender.send(line).is_err() {
                            return;
                        }
                    }
                }
            }
        }
        if !pending.is_empty() {
            let _ = sender.send(pending);
        }
    });
    lines
}

impl Instance {
    fn started(&mut self, handle: &Value, span: Span) -> Result<&mut Started, RuntimeError> {
        match handle {
            Value::Int(value) => value
                .to_usize()
                .and_then(|index| self.processes.slots.get_mut(index)),
            _ => None,
        }
        .ok_or_else(|| {
            RuntimeError::new(
                "R_PROCESS_INVALID",
                "that command was never started by this program",
                span,
            )
        })
    }

    /// `start_process`: the command's handle, or the failure `process_run` would give.
    pub(super) fn process_start(&mut self, command: &str, arguments: &[String]) -> Value {
        let mut builder = match prepare(&self.policy, command, arguments) {
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
        track_group(child.id());
        let limit = self.policy.max_process_output_bytes;
        let exceeded = Arc::new(AtomicBool::new(false));
        let lines = match child.stdout.take() {
            Some(stream) => read_lines(stream, limit, exceeded.clone()),
            None => mpsc::channel().1,
        };
        let stderr = child
            .stderr
            .take()
            .map(|stream| capture(stream, limit, Arc::new(AtomicBool::new(false))));
        let stdin = child.stdin.take();
        self.processes.slots.push(Started {
            child,
            stdin,
            lines,
            exceeded,
            stderr,
            command: command.to_owned(),
            limit,
            finished: None,
        });
        Value::ResultOk(Box::new(Value::Int(BigInt::from_usize(
            self.processes.slots.len() - 1,
        ))))
    }

    /// `process_line`: the next line the command printed, waiting for one; `None` once its output
    /// has ended.
    pub(super) fn process_line(
        &mut self,
        handle: &Value,
        budget: &Arc<Budget>,
        span: Span,
    ) -> Result<Value, RuntimeError> {
        loop {
            let started = self.started(handle, span)?;
            match started.lines.recv_timeout(Duration::from_millis(5)) {
                Ok(line) => {
                    return Ok(Value::OptionSome(Box::new(Value::Text(SharedText::from(
                        String::from_utf8_lossy(&line).as_ref(),
                    )))));
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    if started.exceeded.load(Ordering::SeqCst) {
                        kill_group(&mut started.child);
                    }
                    return Ok(Value::OptionNone);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => self.wait_cancelled(budget, span)?,
            }
        }
    }

    pub(super) fn process_write(
        &mut self,
        handle: &Value,
        input: &str,
        span: Span,
    ) -> Result<Value, RuntimeError> {
        let started = self.started(handle, span)?;
        let Some(pipe) = started.stdin.as_mut() else {
            return Ok(failure(
                "E_PROCESS_WRITE",
                format!("the input of `{}` is closed", started.command),
            ));
        };
        Ok(
            match pipe.write_all(input.as_bytes()).and_then(|()| pipe.flush()) {
                Ok(()) => Value::ResultOk(Box::new(Value::Unit)),
                Err(error) => failure(
                    "E_PROCESS_WRITE",
                    format!("cannot write to `{}`: {error}", started.command),
                ),
            },
        )
    }

    pub(super) fn process_close_input(
        &mut self,
        handle: &Value,
        span: Span,
    ) -> Result<Value, RuntimeError> {
        self.started(handle, span)?.stdin.take();
        Ok(Value::ResultOk(Box::new(Value::Unit)))
    }

    /// `process_wait`: wait for the command to exit. The result is `[exit code, output not read
    /// with process_line, standard error]`, the shape `process_run` gives.
    pub(super) fn process_wait(
        &mut self,
        handle: &Value,
        budget: &Arc<Budget>,
        span: Span,
    ) -> Result<Value, RuntimeError> {
        if let Some(done) = self.started(handle, span)?.finished.clone() {
            return Ok(done);
        }
        let status = loop {
            let started = self.started(handle, span)?;
            match started.child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {}
                Err(error) => {
                    return Ok(failure("E_PROCESS_WAIT", error.to_string()));
                }
            }
            if started.exceeded.load(Ordering::SeqCst) {
                let (command, limit) = (started.command.clone(), started.limit);
                started.stop();
                let done = failure(
                    "E_PROCESS_OUTPUT_LIMIT",
                    format!("`{command}` wrote more than {limit} bytes"),
                );
                self.started(handle, span)?.finished = Some(done.clone());
                return Ok(done);
            }
            self.wait_cancelled(budget, span)?;
            thread::sleep(Duration::from_millis(2));
        };
        let started = self.started(handle, span)?;
        crate::process_ops::untrack_group(started.child.id());
        // The command is gone, but anything it left running in its group goes too.
        kill_group(&mut started.child);
        let mut unread = Vec::new();
        while let Ok(line) = started.lines.recv_timeout(Duration::from_millis(50)) {
            unread.push(String::from_utf8_lossy(&line).into_owned());
        }
        let stderr = started
            .stderr
            .take()
            .map(|reader| reader.join().unwrap_or_default())
            .unwrap_or_default();
        started.stdin.take();
        let mut rest = unread.join("\n");
        if !unread.is_empty() {
            rest.push('\n');
        }
        let done = Value::ResultOk(Box::new(Value::List(Shared::new(vec![
            text(&exit_code(status).to_string()),
            text(&rest),
            text(&String::from_utf8_lossy(&stderr)),
        ]))));
        started.finished = Some(done.clone());
        Ok(done)
    }
}
