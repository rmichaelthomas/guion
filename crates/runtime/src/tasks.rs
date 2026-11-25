//! `together:` blocks: background tasks that end with their block.
//!
//! A task is a function running on its own thread with its own `Instance`, the parent's grants and
//! a slice of the parent's budgets. Nothing outlives its block: leaving the block stops every task
//! that was not waited for, and an error that unwinds past the block does the same when the
//! instance is dropped. See `docs/design-structured-concurrency.md`.

use super::*;

/// What a finished task hands back.
struct TaskReport {
    result: Result<Value, RuntimeError>,
    output: String,
    errors: String,
    thread_id: String,
}

/// How a task ended, once the program has asked for it.
#[derive(Clone)]
enum Outcome {
    Value(Value),
    Fault(TaskErrorValue),
}

struct TaskSlot {
    /// Stops this task: its VM notices between instructions.
    cancelled: Arc<AtomicBool>,
    reports: mpsc::Receiver<TaskReport>,
    handle: Option<thread::JoinHandle<()>>,
    lane: String,
    /// Its position among the tasks of its block, which is what `TaskError.index` reports.
    ordinal: usize,
    /// A report that `first_ready` saw arrive and `wait` has not yet taken.
    arrived: Option<TaskReport>,
    /// Set once the program has its result (or has given up on it).
    outcome: Option<Outcome>,
    /// The task ended by itself, with a result the program collected, so its recorded lane is whole.
    complete: bool,
}

struct Scope {
    first_task: usize,
    /// The instruction budget and cancellation shared by this block's tasks.
    budget: Arc<Budget>,
    /// The parent's budget, to give back what the tasks did not use.
    parent: Arc<Budget>,
    call_number: u32,
    /// Channels made in this block, closed when it ends.
    channels: Vec<usize>,
    /// Commands started before this block began: the ones from here on are the block's.
    first_process: usize,
}

/// The `together:` blocks an instance is inside, innermost last, and the tasks they started.
#[derive(Default)]
pub(crate) struct Together {
    scopes: Vec<Scope>,
    tasks: Vec<TaskSlot>,
}

impl std::fmt::Debug for Together {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "Together({} blocks, {} tasks)",
            self.scopes.len(),
            self.tasks.len()
        )
    }
}

impl Together {
    fn stop_from(&mut self, first: usize) {
        for slot in &self.tasks[first..] {
            slot.cancelled.store(true, Ordering::Release);
        }
        for slot in &mut self.tasks[first..] {
            if let Some(handle) = slot.handle.take() {
                let _ = handle.join();
            }
        }
    }
}

impl Drop for Together {
    fn drop(&mut self) {
        for scope in &self.scopes {
            if let Some(flag) = &scope.budget.local_cancellation {
                flag.store(true, Ordering::Release);
            }
        }
        self.stop_from(0);
    }
}

/// Everything a task's own `Instance` is built from; all of it is shared or copied, so it can move
/// to the task's thread.
struct TaskParts {
    tape: Option<Arc<tape::Tape>>,
    artifact: Arc<Artifact>,
    native_constants: Arc<Vec<Value>>,
    registers: Arc<RegisterPrograms>,
    native_code: Option<Arc<NativeCode>>,
    native_enabled: bool,
    native_calls: Arc<AtomicUsize>,
    native_deoptimizations: Arc<AtomicUsize>,
    native_deoptimization_sites: Arc<std::sync::Mutex<BTreeMap<(usize, usize), usize>>>,
    native_compilations: Arc<AtomicUsize>,
    policy: HostPolicy,
    http: Option<Arc<HostHttpClient>>,
    sockets: net_ops::SocketTable,
    channels: Arc<ChannelTable>,
}

impl TaskParts {
    fn build(self, lane: String, cancelled: Arc<AtomicBool>) -> Instance {
        let policy = self.policy;
        let mut database_roots = Vec::with_capacity(
            1 + policy.additional_file_roots.len() + policy.file_root_aliases.len(),
        );
        if let Some(root) = &policy.file_root {
            database_roots.push(root.clone());
        }
        database_roots.extend(policy.additional_file_roots.iter().cloned());
        database_roots.extend(policy.file_root_aliases.values().cloned());
        Instance {
            tape: self.tape,
            tape_lane: lane,
            parallel_calls: 0,
            artifact: self.artifact,
            native_constants: self.native_constants,
            registers: self.registers,
            native: None,
            native_code: self.native_code,
            native_enabled: self.native_enabled,
            native_whole_program_entry: false,
            native_compile_attempted: false,
            runtime_profile_enabled: false,
            runtime_profile_sampled: false,
            runtime_profile_metrics: None,
            native_calls: self.native_calls,
            native_deoptimizations: self.native_deoptimizations,
            native_deoptimization_sites: self.native_deoptimization_sites,
            native_entry_copy_dominated: Vec::new(),
            native_compilations: self.native_compilations,
            readers: stdlib_data::FileReaderRegistry::new(usize::MAX, policy.max_file_bytes),
            sockets: self.sockets,
            databases: stdlib_services::DatabaseRegistry::with_roots(
                database_roots,
                policy.max_file_bytes,
                policy.max_collection_items,
                stdlib_services::SQLITE_BUSY_DEADLINE_MS,
            ),
            toml_documents: stdlib_services::TomlRegistry::new(policy.max_text_bytes),
            http: self.http,
            policy,
            output: String::new(),
            program_stderr: String::new(),
            streamed_bytes: 0,
            replay_skip_stdout: Default::default(),
            replay_skip_stderr: Default::default(),
            closed: false,
            worker_threads: HashSet::new(),
            cancelled: Some(cancelled),
            #[cfg(feature = "python-interop")]
            python_bridge: None,
            host_stderr: String::new(),
            effect_report: Vec::new(),
            omitted_effects: 0,
            python_native_image_observation: "not_used".into(),
            python_native_images: Vec::new(),
            together: Together::default(),
            channels: self.channels,
            processes: background::ProcessTable::default(),
        }
    }
}

fn task_error(index: usize, error: RuntimeError, span: Span) -> TaskErrorValue {
    let mut trace = error.trace;
    trace.push(span);
    TaskErrorValue {
        index: BigInt::from_usize(index),
        code: error.code,
        message: error.message,
        trace,
    }
}

impl Instance {
    fn together_lane(&self, call_number: u32, ordinal: usize) -> String {
        if self.tape_lane.is_empty() {
            format!("{call_number}.{ordinal}")
        } else {
            format!("{}/{call_number}.{ordinal}", self.tape_lane)
        }
    }

    /// `together:` begins: the block's tasks get three quarters of what is left of the
    /// instruction budget, and share the allocation budget.
    pub(super) fn together_enter(&mut self, budget: &Arc<Budget>) {
        // Tasks read the artifact's constants, so their reference counts are atomic from here on.
        self.artifact.share_across_threads();
        self.registers.share_across_threads();
        let cancel = Arc::new(AtomicBool::new(false));
        let scope_budget = Arc::new(Budget {
            remaining: guion_vm_core::CachePadded::new(AtomicU64::new(budget.reserve_parallel())),
            instruction_limit: budget.instruction_limit,
            allocation: budget.allocation.clone(),
            allocation_floor: budget.parallel_allocation_floor(),
            cancellation: budget.cancellation.clone(),
            local_cancellation: Some(cancel),
            runtime_profile: budget.runtime_profile.clone(),
        });
        budget.allocation.begin_sharing();
        let call_number = self.parallel_calls;
        self.parallel_calls += 1;
        let first_task = self.together.tasks.len();
        self.together.scopes.push(Scope {
            first_task,
            budget: scope_budget,
            parent: budget.clone(),
            call_number,
            channels: Vec::new(),
            first_process: self.processes.len(),
        });
    }

    /// `start(function, argument)`: begin the task and return its handle.
    pub(super) fn task_start(
        &mut self,
        function: usize,
        argument: Value,
        depth: usize,
        span: Span,
    ) -> Result<Value, RuntimeError> {
        let Some(scope) = self.together.scopes.last() else {
            return Err(RuntimeError::new(
                "R_START_OUTSIDE_TOGETHER",
                "start was reached outside a together: block",
                span,
            ));
        };
        let (first_task, call_number, task_budget) =
            (scope.first_task, scope.call_number, scope.budget.clone());
        let ordinal = self.together.tasks.len() - first_task;
        let handle_index = self.together.tasks.len();
        if ordinal >= self.policy.max_parallelism {
            return Err(RuntimeError::new(
                "R_PARALLEL_LIMIT",
                format!(
                    "a together: block started more than {} tasks\n  raise it in guion.toml:   [runtime] max_parallelism = {}",
                    self.policy.max_parallelism,
                    ordinal + 1
                ),
                span,
            ));
        }
        argument.share_across_threads();
        let lane = self.together_lane(call_number, ordinal);
        let cancelled = Arc::new(AtomicBool::new(false));
        let parts = TaskParts {
            tape: self.tape.clone(),
            artifact: Arc::clone(&self.artifact),
            native_constants: Arc::clone(&self.native_constants),
            registers: Arc::clone(&self.registers),
            native_code: self.native_code.clone(),
            native_enabled: self.native_enabled,
            native_calls: self.native_calls.clone(),
            native_deoptimizations: self.native_deoptimizations.clone(),
            native_deoptimization_sites: self.native_deoptimization_sites.clone(),
            native_compilations: self.native_compilations.clone(),
            // A task keeps its output for the parent to merge when it is waited for.
            policy: self.policy.clone().with_streaming_output(false),
            http: self.http.clone(),
            sockets: self.sockets.share(),
            channels: Arc::clone(&self.channels),
        };
        let (sender, reports) = mpsc::channel();
        let thread_lane = lane.clone();
        let thread_cancelled = cancelled.clone();
        let handle = thread::spawn(move || {
            let mut task = parts.build(thread_lane, thread_cancelled);
            let thread_id = format!("{:?}", thread::current().id());
            let result = task.invoke_inner(function, vec![argument], depth, task_budget);
            if let Ok(value) = &result {
                value.share_across_threads();
            }
            let _ = sender.send(TaskReport {
                result,
                output: std::mem::take(&mut task.output),
                errors: std::mem::take(&mut task.program_stderr),
                thread_id,
            });
        });
        self.together.tasks.push(TaskSlot {
            cancelled,
            reports,
            handle: Some(handle),
            lane,
            ordinal,
            arrived: None,
            outcome: None,
            complete: false,
        });
        Ok(Value::Task(handle_index))
    }

    fn task_slot(&mut self, value: &Value, span: Span) -> Result<usize, RuntimeError> {
        match value {
            Value::Task(index) if *index < self.together.tasks.len() => Ok(*index),
            _ => Err(RuntimeError::new(
                "R_TASK_INVALID",
                "that task does not belong to a together: block that is still running",
                span,
            )),
        }
    }

    /// Wait for a task, up to `limit` when there is one. A task that is not done in time is
    /// stopped and its result is `R_DEADLINE`.
    pub(super) fn task_wait(
        &mut self,
        value: &Value,
        limit: Option<Duration>,
        budget: &Arc<Budget>,
        span: Span,
    ) -> Result<Value, RuntimeError> {
        let index = self.task_slot(value, span)?;
        if self.together.tasks[index].outcome.is_none() {
            let started = Instant::now();
            let report = loop {
                if budget.is_cancelled() || self.externally_cancelled() {
                    return Err(RuntimeError::new(
                        "R_CANCELLED",
                        "the program was cancelled while waiting for a task",
                        span,
                    ));
                }
                let slice = match limit {
                    Some(limit) => {
                        let left = limit.saturating_sub(started.elapsed());
                        if left.is_zero() {
                            break None;
                        }
                        left.min(Duration::from_millis(5))
                    }
                    None => Duration::from_millis(5),
                };
                if let Some(report) = self.together.tasks[index].arrived.take() {
                    break Some(report);
                }
                match self.together.tasks[index].reports.recv_timeout(slice) {
                    Ok(report) => break Some(report),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        return Err(RuntimeError::new(
                            "R_WORKER_PANIC",
                            "a task terminated unexpectedly",
                            span,
                        ));
                    }
                }
            };
            let slot = &mut self.together.tasks[index];
            let ordinal = slot.ordinal;
            match report {
                Some(report) => {
                    if let Some(handle) = slot.handle.take() {
                        let _ = handle.join();
                    }
                    self.worker_threads.insert(report.thread_id);
                    if self.stdout_bytes().saturating_add(report.output.len())
                        > self.policy.output_limit_bytes
                    {
                        return Err(RuntimeError::new(
                            "R_OUTPUT_LIMIT",
                            "task output exceeds the configured limit",
                            span,
                        ));
                    }
                    self.append_stdout(&report.output)?;
                    self.append_stderr(&report.errors, span)?;
                    let slot = &mut self.together.tasks[index];
                    slot.outcome = Some(match report.result {
                        Ok(value) => Outcome::Value(value),
                        Err(error) => Outcome::Fault(task_error(ordinal, error, span)),
                    });
                    slot.complete = !matches!(&slot.outcome, Some(Outcome::Fault(fault)) if fault.code == "R_CANCELLED");
                }
                None => {
                    // Out of time: the task stops, and its output is never shown.
                    slot.cancelled.store(true, Ordering::Release);
                    slot.outcome = Some(Outcome::Fault(TaskErrorValue {
                        index: BigInt::from_usize(ordinal),
                        code: "R_DEADLINE".into(),
                        message: "the task was not done in time".into(),
                        trace: vec![span],
                    }));
                    slot.complete = false;
                }
            }
        }
        Ok(match self.together.tasks[index].outcome.clone() {
            Some(Outcome::Value(value)) => Value::ResultOk(Box::new(value)),
            Some(Outcome::Fault(fault)) => {
                Value::ResultErr(Box::new(Value::TaskError(Box::new(fault))))
            }
            None => unreachable!("the wait above always settles the task"),
        })
    }

    /// While replaying a recorded `wait_within`: the recording already says how it went, so make
    /// the live task match it. A task that finished in time is waited for, so its output is shown
    /// as it was; one that ran out of time is stopped.
    pub(super) fn task_settle_replayed_wait(
        &mut self,
        value: &Value,
        timed_out: bool,
        budget: &Arc<Budget>,
        span: Span,
    ) -> Result<(), RuntimeError> {
        if timed_out {
            let index = self.task_slot(value, span)?;
            let slot = &mut self.together.tasks[index];
            slot.cancelled.store(true, Ordering::Release);
            slot.outcome = Some(Outcome::Fault(TaskErrorValue {
                index: BigInt::from_usize(slot.ordinal),
                code: "R_DEADLINE".into(),
                message: "the task was not done in time".into(),
                trace: vec![span],
            }));
            return Ok(());
        }
        self.task_wait(value, None, budget, span).map(|_| ())
    }

    /// `together:` ends: every task not waited for stops, and what the block did not use of the
    /// instruction budget goes back to the program.
    pub(super) fn together_exit(&mut self, span: Span) -> Result<(), RuntimeError> {
        let Some(scope) = self.together.scopes.pop() else {
            return Err(RuntimeError::new(
                "R_TASK_INVALID",
                "a together: block ended that was never begun",
                span,
            ));
        };
        if let Some(flag) = &scope.budget.local_cancellation {
            flag.store(true, Ordering::Release);
        }
        self.together.stop_from(scope.first_task);
        self.processes.stop_from(scope.first_process);
        for index in &scope.channels {
            if let Some(channel) = self.channels.get(*index) {
                channel.close();
            }
        }
        if let Some(tape) = &self.tape {
            // A task the program did not see through has a shorter lane than a full run; a replay
            // of it may stop where the recording did.
            for slot in &self.together.tasks[scope.first_task..] {
                if !slot.complete {
                    tape.mark_lane_incomplete(&slot.lane);
                }
            }
        }
        self.together.tasks.truncate(scope.first_task);
        scope.parent.remaining.fetch_add(
            scope.budget.remaining.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        scope.parent.allocation.end_sharing();
        if scope.budget.cancellation.is_cancelled() {
            return Err(RuntimeError::new(
                "R_CANCELLED",
                "invocation was cancelled while tasks were running",
                span,
            ));
        }
        Ok(())
    }
}

/// One bounded queue shared by the tasks of a block. A full queue makes `send` wait, so a fast
/// producer cannot get ahead of a slow consumer by more than the capacity.
struct Channel {
    state: std::sync::Mutex<ChannelState>,
    changed: std::sync::Condvar,
}

struct ChannelState {
    queue: std::collections::VecDeque<Value>,
    capacity: usize,
    closed: bool,
}

impl Channel {
    fn lock(&self) -> std::sync::MutexGuard<'_, ChannelState> {
        match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    pub(crate) fn close(&self) {
        self.lock().closed = true;
        self.changed.notify_all();
    }
}

/// Every channel of an instance and the tasks it started; a channel is its index here.
#[derive(Default)]
pub(crate) struct ChannelTable {
    channels: std::sync::Mutex<Vec<Arc<Channel>>>,
}

impl std::fmt::Debug for ChannelTable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ChannelTable")
    }
}

impl ChannelTable {
    fn get(&self, index: usize) -> Option<Arc<Channel>> {
        match self.channels.lock() {
            Ok(channels) => channels.get(index).cloned(),
            Err(poisoned) => poisoned.into_inner().get(index).cloned(),
        }
    }

    fn add(&self, channel: Channel) -> usize {
        let mut channels = match self.channels.lock() {
            Ok(channels) => channels,
            Err(poisoned) => poisoned.into_inner(),
        };
        channels.push(Arc::new(channel));
        channels.len() - 1
    }
}

const MAX_CHANNEL_CAPACITY: usize = 65_536;

impl Instance {
    fn channel_of(&self, value: &Value, span: Span) -> Result<Arc<Channel>, RuntimeError> {
        match value {
            Value::Channel(index) => self.channels.get(*index),
            _ => None,
        }
        .ok_or_else(|| {
            RuntimeError::new(
                "R_TASK_INVALID",
                "that channel does not belong to a together: block that is still running",
                span,
            )
        })
    }

    pub(super) fn wait_cancelled(
        &self,
        budget: &Arc<Budget>,
        span: Span,
    ) -> Result<(), RuntimeError> {
        if budget.is_cancelled() || self.externally_cancelled() {
            return Err(RuntimeError::new(
                "R_CANCELLED",
                "the program was cancelled while waiting on a channel",
                span,
            ));
        }
        Ok(())
    }

    pub(super) fn channel_new(
        &mut self,
        capacity: &Value,
        span: Span,
    ) -> Result<Value, RuntimeError> {
        let capacity = match capacity {
            Value::Int(value) => value.to_usize(),
            _ => None,
        }
        .filter(|capacity| (1..=MAX_CHANNEL_CAPACITY).contains(capacity))
        .ok_or_else(|| {
            RuntimeError::new(
                "R_CHANNEL_CAPACITY",
                format!("a channel holds 1 to {MAX_CHANNEL_CAPACITY} values"),
                span,
            )
        })?;
        let Some(scope) = self.together.scopes.last_mut() else {
            return Err(RuntimeError::new(
                "R_TASK_INVALID",
                "a channel was made outside a together: block",
                span,
            ));
        };
        let index = self.channels.add(Channel {
            state: std::sync::Mutex::new(ChannelState {
                queue: std::collections::VecDeque::new(),
                capacity,
                closed: false,
            }),
            changed: std::sync::Condvar::new(),
        });
        scope.channels.push(index);
        Ok(Value::Channel(index))
    }

    /// `send`: wait while the channel is full; `Err(R_CHANNEL_CLOSED)` when it is closed.
    pub(super) fn channel_send(
        &mut self,
        channel: &Value,
        value: Value,
        budget: &Arc<Budget>,
        span: Span,
    ) -> Result<Value, RuntimeError> {
        let channel = self.channel_of(channel, span)?;
        value.share_across_threads();
        let mut state = channel.lock();
        loop {
            if state.closed {
                return Ok(Value::ResultErr(Box::new(Value::TaskError(Box::new(
                    TaskErrorValue {
                        index: BigInt::from_usize(0),
                        code: "R_CHANNEL_CLOSED".into(),
                        message: "the channel was closed".into(),
                        trace: vec![span],
                    },
                )))));
            }
            if state.queue.len() < state.capacity {
                state.queue.push_back(value);
                drop(state);
                channel.changed.notify_all();
                return Ok(Value::ResultOk(Box::new(Value::Unit)));
            }
            self.wait_cancelled(budget, span)?;
            state = match channel
                .changed
                .wait_timeout(state, Duration::from_millis(5))
            {
                Ok((state, _)) => state,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
    }

    /// `receive`: the next value, waiting for one; `None` once the channel is closed and empty.
    pub(super) fn channel_receive(
        &mut self,
        channel: &Value,
        budget: &Arc<Budget>,
        span: Span,
    ) -> Result<Value, RuntimeError> {
        let channel = self.channel_of(channel, span)?;
        let mut state = channel.lock();
        loop {
            if let Some(value) = state.queue.pop_front() {
                drop(state);
                channel.changed.notify_all();
                return Ok(Value::OptionSome(Box::new(value)));
            }
            if state.closed {
                return Ok(Value::OptionNone);
            }
            self.wait_cancelled(budget, span)?;
            state = match channel
                .changed
                .wait_timeout(state, Duration::from_millis(5))
            {
                Ok((state, _)) => state,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
    }

    pub(super) fn channel_close(
        &mut self,
        channel: &Value,
        span: Span,
    ) -> Result<(), RuntimeError> {
        self.channel_of(channel, span)?.close();
        Ok(())
    }

    /// `first_ready` and `first_value`: `Some(Ready(position, result))` for the first listed task
    /// that is finished, or channel that has a value or is closed, waiting up to `limit`; `None`
    /// when time runs out. The result is the task's `wait` result, or what `receive` took from the
    /// channel (`None` for one closed and empty), taken in the same step so no other consumer can
    /// slip in between. When several are ready the earliest in the list wins.
    pub(super) fn first_of(
        &mut self,
        items: &Value,
        tasks: bool,
        name: guion_model::RecordName,
        limit: Option<Duration>,
        budget: &Arc<Budget>,
        span: Span,
    ) -> Result<Value, RuntimeError> {
        let items: Vec<Value> = match items {
            Value::List(values) => values.as_ref().clone(),
            _ => Vec::new(),
        };
        if items.is_empty() {
            return Err(RuntimeError::new(
                "R_TASK_INVALID",
                "there is nothing to wait for in an empty list",
                span,
            ));
        }
        let started = Instant::now();
        loop {
            for (position, item) in items.iter().enumerate() {
                let payload = if tasks {
                    let index = self.task_slot(item, span)?;
                    if self.task_poll(index) {
                        Some(self.task_wait(item, None, budget, span)?)
                    } else {
                        None
                    }
                } else {
                    let channel = self.channel_of(item, span)?;
                    let mut state = channel.lock();
                    if let Some(value) = state.queue.pop_front() {
                        drop(state);
                        channel.changed.notify_all();
                        Some(Value::OptionSome(Box::new(value)))
                    } else if state.closed {
                        Some(Value::OptionNone)
                    } else {
                        None
                    }
                };
                if let Some(payload) = payload {
                    return Ok(Value::OptionSome(Box::new(Value::Record(
                        name,
                        Shared::new(vec![Value::Int(BigInt::from_usize(position)), payload]),
                    ))));
                }
            }
            self.wait_cancelled(budget, span)?;
            if limit.is_some_and(|limit| started.elapsed() >= limit) {
                return Ok(Value::OptionNone);
            }
            thread::sleep(Duration::from_millis(2));
        }
    }

    /// Whether a task has finished, without taking its result.
    fn task_poll(&mut self, index: usize) -> bool {
        let slot = &mut self.together.tasks[index];
        if slot.outcome.is_some() || slot.arrived.is_some() {
            return true;
        }
        match slot.reports.try_recv() {
            Ok(report) => {
                slot.arrived = Some(report);
                true
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => true,
        }
    }
}
