# Design: structured concurrency for waiting work

Status: built, in artifact format 76. See "Notes from the build" at the end for where the build differs from the text above.

## The idea in one paragraph

Guion already does "do this to every item, in parallel" well: `parallel_map`
joins every worker, stops siblings on a fault, shares budgets and grants, and
replays identically from a tape. Agent and automation programs need more than
that. They fan out many network calls that spend their time waiting rather than
computing, give up on slow work, run a command in the background while doing
something else, and react to whichever of several things happens first. This
design adds those abilities without async/await, without tasks that outlive
their scope, and without anything that weakens grants, budgets or replay.

## What exists today

- `parallel_map(list, f, worker_limit) -> Result[List[U], TaskError]` runs `f`
  on every item with at most `worker_limit` workers. Every worker is joined
  before it returns. A fault in one item stops the rest. Workers share the
  allocation counter and the parent's grants.
- `try_map_list(items, f)` makes each item its own fault boundary and returns
  `List[Result[U, TaskError]]`, but runs items one at a time.
- Cancellation (`invoke_with_cancellation`) stops scheduling and joins workers.
  The VM checks between instructions. A blocking HTTP call is noticed only when
  it returns or reaches its own timeout.
- Tapes give each `parallel_map` item its own lane (`"lane":"0.2"`), so a
  replay gives the same output with any worker limit.
- Guion values are immutable and closures capture by value, so workers can't
  share mutable state. There's nothing to lock.
- `process_run(command, arguments, stdin)` waits for the command to finish.

## What's missing

1. **Fan-out is capped at the core count.** The worker limit can't exceed
   `max_parallelism`, which defaults to the number of cores
   (`crates/runtime/src/lib.rs`, `R_PARALLEL_LIMIT`). On an 8-core laptop,
   `parallel_map(urls, fetch, 50)` fails. A worker waiting on the network
   doesn't use a core, so this is the wrong limit for that work.
2. **No deadlines.** There's no way to say "give up on any item after 5
   seconds" or "the whole fan-out gets 30 seconds."
3. **Nothing runs in the background.** A program can't start a command, read
   its output as it arrives, and do other work at the same time.
4. **No "whichever comes first."** A program can't wait on several sources and
   act on the first one ready.

## Rules every part follows

These are what make the design safe, and every part below is checked against
them.

1. **No async/await.** Ordinary functions stay ordinary. Marking functions as
   async splits a language in two and makes the person learn the runtime's
   model. Here, concurrency is something you *call*, not a kind of function.
2. **Nothing outlives its scope.** Every task, background command and channel
   belongs to a block, and none of them exists after the block ends. There is
   no fire-and-forget.
3. **Tasks get exactly the parent's grants.** Never more. A permission question
   raised inside a task is asked one at a time, as in
   `design-scoped-profiles.md`.
4. **Budgets are shared.** Memory, instruction and output limits count across
   all tasks, as with `parallel_map` today.
5. **Every choice made by timing goes on the tape.** Which task finished first,
   which item ran out of time, which value arrived on a channel first. A replay
   makes the same choices, so it's still deterministic even though the live run
   isn't.
6. **No shared mutable state.** Tasks communicate by returning values and by
   channels. Immutability already rules out data races. This design keeps it
   that way and adds no locks.

## Part 1: fan-out isn't capped by cores

- The `parallel_map` worker limit gets its own default ceiling of **64**,
  independent of core count. `[runtime] max_parallelism` still sets it, and can
  raise it up to **1024**.
- A worker is an OS thread. Sixty-four threads waiting on the network cost
  little. For work that keeps the processor busy, a limit above the core count
  just takes turns on the cores, so it gives correct results without speeding
  anything up.
- A new builtin, `core_count() -> Int`, lets computing work say
  `parallel_map(items, crunch, core_count())` instead of guessing.
- The HTTP client's internal thread pool (currently clamped to 8) is sized from
  the worker ceiling, so 50 workers aren't funneled through 8 connections.
- `R_PARALLEL_LIMIT` names the fix:

  ```
  error[R_PARALLEL_LIMIT]: worker limit 200 is above this project's limit of 64
    raise it in guion.toml:   [runtime] max_parallelism = 200
  ```

The tape and replay are unaffected, because lanes already make replay
independent of the worker limit.

## Part 2: deadlines

A new builtin, `try_parallel_map(list, f, limits) -> List[Result[U, TaskError]]`,
combines `parallel_map`'s workers with `try_map_list`'s per-item results. Its
options are a record:

```
let limits = Workers(at_once=50, each_ms=5000, all_ms=30000)
let pages = try_parallel_map(urls, fetch, limits)
```

- `at_once` is the worker limit. `each_ms` and `all_ms` are optional. `0`
  means no deadline.
- An item that runs past `each_ms` stops and becomes
  `Err(TaskError)` with `task_code` `R_DEADLINE`. Other items carry on.
- When `all_ms` passes, items still running stop, items not started never
  start, and both become `Err` with `R_DEADLINE`. Finished items keep their
  results.
- An item's own fault is its own `Err`, as in `try_map_list`. It doesn't stop
  siblings. `parallel_map` keeps its current all-or-nothing behavior for code
  that wants it.

**Making deadlines real.** Checking between instructions isn't enough when an
item is blocked inside an HTTP call. Inside an item with a deadline, every
blocking host call (HTTP, process, socket, Python) uses the smaller of its own
timeout and the time left before the deadline. A slow server therefore can't
hold an item past its deadline.

**Time without a clock grant.** Deadlines use a monotonic timer inside the
runtime. The program never reads it, so no `[clock]` grant is needed.

**Tape.** An item that runs out of time ends its lane with a `deadline` entry.
A replay stops that item at the same point with the same `R_DEADLINE`, however
fast the replay runs.

## Part 3: a block for background work

A new block, `together:`, runs work in the background and ends it all when the
block ends:

```
together:
    let build = start(run_build, "release")
    let lint = start(run_lint, "")
    let built = wait(build)
    let linted = wait(lint)
```

- `start(f, argument) -> Task[T]` starts `f(argument)` in the background. It's
  only allowed inside a `together:` block (`E_START_OUTSIDE_TOGETHER`). `f`
  follows the same rules as `parallel_map` functions: a named function, or a
  function value that captures nothing.
- `wait(task) -> Result[T, TaskError]` waits for the task and gives its result.
  A fault in the task is an `Err`. It doesn't stop siblings.
- `wait_within(task, ms) -> Result[T, TaskError]` gives up after `ms` with
  `R_DEADLINE`, and stops the task.
- **When the block ends, anything not yet waited for is stopped.** You get the
  results you waited for. Everything else is cancelled, and its background
  commands are killed. This makes "watch something until I'm done" natural,
  because the watcher stops when the block ends. Waiting for everything at the
  end, as some languages do, would make such a block hang forever.
- `Task`, `Channel` and `Process` belong to the block that made them. They
  can't be returned from it, stored in anything that outlives it, or sent
  across a JSON, file, HTTP or `print` boundary. This is checked when the code
  is checked (`E_SCOPE_ESCAPE`), the same way function types are kept away from
  serialization today (`E_FN_NOT_SERIALIZABLE`).
- `together:` blocks can be nested. An inner block ends before the outer one,
  so the rule that nothing outlives its scope holds at every level.

**Output.** A `print` from a task is written as a whole line and never mixed
with another task's line. The order of lines from different tasks is not
promised. The intended pattern is for tasks to return values and the block to
print them, and `guion lint` will suggest that when it sees `print` inside a
started function.

## Part 4: channels and "whichever comes first"

Inside a `together:` block:

- `channel(capacity) -> Channel[T]` makes a channel with a fixed capacity
  (1 to 65,536). The element type comes from the expected type, as with empty
  lists.
- `send(channel, value)` waits while the channel is full. Sending on a closed
  channel is `Err` (`R_CHANNEL_CLOSED`).
- `receive(channel) -> Option[T]` waits for the next value. It returns `None`
  once the channel is closed and empty.
- `close(channel)` closes it. Channels are closed automatically when their
  block ends.
- `first_ready(tasks) -> Ready[T]` waits until any of the listed tasks finishes
  and returns `Ready(index, result)`. `first_value(channels) -> Ready[T]` does
  the same for channels. Both take an optional deadline in their `_within`
  forms.

Fixed capacity means a fast producer can't fill memory faster than a slow
consumer drains it. That's backpressure by default, with nothing to configure.

**Tape.** Which task won `first_ready`, and which sender's value `receive` got,
are written to the parent's lane as `choice` entries. A replay makes the same
choices: it waits for the chosen lane's recorded event before continuing, so
replay output matches the recording exactly.

## Part 5: commands in the background

Inside a `together:` block:

- `start_process(command, arguments) -> Process` starts a command without
  waiting. It needs the same `[process] commands` grant as `process_run`.
- `process_line(process) -> Option[Text]` reads the next line of its output, or
  `None` when the output ends.
- `process_write(process, text)` writes to its input.
  `process_close_input(process)` closes the input.
- `process_wait(process) -> Result[ProcessResult, ProcessError]` waits for it
  to exit, with the existing result and error types.
- Each started command gets its own process group. When the block ends, or
  `guion` itself is interrupted with Ctrl-C, the whole group is stopped and
  reaped, so a sub-agent can't leave orphan processes behind.
- Output reads are bounded by the existing `max_process_output_bytes`, counted
  per process.

This is how a Guion program runs and supervises sub-agents: start them, read
their output as it arrives, give each one a deadline, and know they're all gone
when the block ends.

## What this design doesn't cover

- **Running across machines.** Everything here is one process. Distributed
  work is a separate question.
- **The browser playground.** It has no threads. `together:` and
  `try_parallel_map` report a clear "not available in the playground" error
  there, recorded as a known issue.
- **Machine code.** Functions that use `together:` run on the VM, as closures
  do today. `parallel_map` is unchanged.
- **The self-hosted compiler.** It's frozen and isn't updated for this.

## How `guion inspect` reports it

- `together:`, `start`, `try_parallel_map` and channels report
  `concurrency.parallel`, as `parallel_map` does.
- `start_process` reports the process effect with the command name when it's a
  literal.
- The effects of a started function count toward the function that starts it,
  as with `parallel_map` workers today.

## Build order

1. **Worker ceiling and fix-it error** (Part 1). Small, and it fixes the
   50-request case on its own.
2. **`try_parallel_map` with deadlines** (Part 2), including host-call timeouts
   capped by the deadline, and `deadline` tape entries.
3. **`together:`, `start`, `wait`, `wait_within`** (Part 3), with
   scope-escape checking and task lanes on the tape.
4. **Channels, `first_ready`, `first_value`** (Part 4), with `choice` tape
   entries.
5. **`start_process` and process groups** (Part 5).

Each step gets tests:
- 50 concurrent requests against a local server on any core count;
- an item past `each_ms` is `R_DEADLINE`, and a hung HTTP call inside it still
  returns on time;
- `all_ms` keeps finished results;
- a task not waited for is stopped when the block ends, and its process group
  is gone;
- returning a `Task` from a block fails to check;
- replay of a recorded `first_ready` race gives identical output across 100
  runs;
- a full channel blocks its sender, and memory stays flat;
- Ctrl-C during a block leaves no child processes;
- every task sees the parent's grants and no others.

## Decisions

1. **No async/await.** Concurrency is something you call, not a kind of
   function. The cost is that waiting work uses OS threads rather than a
   lightweight scheduler. At the scale a single agent program fans out to
   (tens to low hundreds of operations at once), threads are cheap, and the
   ceiling in Part 1 keeps that bounded. If a real workload ever needs
   thousands at once, the runtime can move to a lightweight scheduler
   underneath without changing a line of Guion code. That's the reason for
   keeping the model out of function signatures.
2. **Unwaited tasks are stopped at block end, not waited for.** That makes
   "watch until done" work without extra code, and it means a forgotten task
   can't hang a program.
3. **No detached tasks.** Long-lived services are written as one `together:`
   block around the program's main loop, which is the same thing with a
   visible scope.
4. **Task faults don't stop siblings** in `together:` and `try_parallel_map`.
   The person decides what a failure means. `parallel_map` keeps
   all-or-nothing for code that wants it.
5. **Deadlines need no clock grant.** The program never sees the time, only
   whether it ran out, and that outcome is recorded on the tape.

## Notes from the build

All five steps are built, in the order above, and one artifact format (76) carries them. Where
the build differs from the design:

- **`Workers` takes its numbers by name.** Record constructors now accept `field=value` in any
  order (`=`, as every other keyword argument in Guion, not `:`), so it is
  `Workers(at_once=50, each_ms=5000, all_ms=30000)`. Positional still works.
- **`Ready[T]` comes from `std:process`.** `first_ready` and `first_value` give `Ready(index,
  value)` as the design says, so they need `import "std:process"` (the error says so). For a task
  `value` is the `wait` result; for a channel it is the `receive` result, taken in the same step
  under the channel's lock. The `_within` forms give `Option[Ready[T]]`.
- **`start_process` gives `Result[Process, StdError]`, not a bare `Process`.** A command that is not
  granted or cannot start is an ordinary outcome, with the same error codes and fix-it text as
  `process_run`. `process_wait` gives `Result[ProcessResult, StdError]` (StdError is the existing
  error type).
- **A deadline on the tape is a cut-short lane, not a new `deadline` entry.** A recording marks the
  lane of an item that ran out of time as incomplete (the marker sibling-cancelled items already
  use), and a replay turns that item into `R_DEADLINE` whatever its speed. An item that ran out
  of time contributes no output, in the recording and in the replay, so both print the same lines.
  A `wait_within`, `first_ready`, `first_value`, `send` and `receive` are recorded as the operations
  they are; a replay returns what was recorded and, for a `wait_within`, still waits for or stops
  the live task to match.
- **Items with a deadline run on the interpreter, not machine code,** because machine code does
  not look at a deadline. A blocking HTTP, command or socket call inside such an item waits no
  longer than the time left; a call into Python is bounded by its own timeout and by the next
  check after it returns.
- **A task's output is shown when the task is waited for,** in the order of the waits, so a
  recording and its replay print the same lines. A task that is stopped shows none.
- **Budgets.** A `together:` block takes three quarters of the instruction budget that is left, as
  `parallel_map` does, and gives back what its tasks did not use when it ends. Tasks share the
  memory budget.
- **`return`, `break`, `continue` and `?` cannot leave a `together:` block** (`E_TOGETHER_EXIT`).
  The design said nothing outlives its scope; making leaving early a compile error keeps that
  true without a cleanup path on every exit. An error that unwinds past a block still stops its
  tasks, when the instance is dropped.
- **`Channel[T]` is a parameter type, never a result type,** so a started function can be handed
  a channel. `Task` and `Process` cannot be written in a signature at all. A channel is made with
  an annotation (`let c: Channel[Int] = channel(8)`).
- **Process groups and signals are Unix; Windows uses a job object.** On Unix each command is its
  own process group and `INT`, `TERM` and `HUP` stop every group before `guion` exits. On Windows
  each command is put in a job object that kills everything in it when the block ends, the
  program ends, or `guion` dies for any reason. A grandchild started in the instant before the
  command is assigned to the job is not covered. One test runs on a GitHub-hosted Windows runner
  (`.github/workflows/windows-background.yml`): `cmd` starts `ping`, the program reads two lines
  of its output, the block ends, and no `ping` is left. It passes. Ctrl-C on Windows and the rest
  of the suite are not run there. A program that polls for signals with
  `process_signal_poll` handles `INT`, `TERM` and `HUP` itself; the commands are then stopped
  when its block or the program ends.
- **The browser playground** reports that `together:` and `try_parallel_map` need threads.
- **The self-hosted compiler is frozen at format 74** and does not know any of this; only its
  builtin catalog lists the new native names, because a conformance test reads it.

