# Design: scoped profiles and asking the human

Status: built. All five steps of the build order are implemented, each with tests. Notes on
where the build settled a detail are at the end.

## The idea in one paragraph

A person writing a quick script should not need to learn permission flags. An
agent running a script should not be able to widen its own access. Guion
already denies everything by default and lets the project folder be read and
written. This design keeps that and adds three things: a named default profile
with a private temp folder, one plain-language question before a run when a
human is present, and denials that say exactly how to grant what was missing.
The question is a convenience for people. It is never the security boundary.

## What exists today

- `HostPolicy` (`crates/runtime/src/lib.rs`) denies clock, HTTP, commands,
  sockets, environment variables and files by default.
- `guion run` and `guion test` grant read and write on the project root
  (`crates/cli/src/main.rs`, `with_file_access(project_root, true, true)`).
- Grants live in `guion.toml`, spread over tables that already exist:
  `[network] hosts`, `[environment] variables`, `[process] commands`,
  `[sockets] connect` / `bind`, `[clock] enabled`.
- Per-run flags add to them: `--allow-http-host`, `--allow-clock`,
  `--allow-command`, `--allow-connect`, `--allow-bind`, `--file-root`.
- A replayed tape gets no access at all, whatever the flags say.
- `guion inspect` reports the effects a program can reach by category
  (`EffectKind`: file read/write, HTTP, environment, clock, process, socket,
  Python). `library_literal_hosts` already pulls host names out of URL
  literals.
- Denials name a fix in some places (`E_NET_DENIED` mentions
  `guion.toml [sockets]` and the flag) but not consistently.
- There is no `--allow-all`. This design keeps it that way.

## Goals

1. A first-time scripter runs `guion run hello.gn` and it works, or they get
   one clear question.
2. An agent, or anything else with no person present, gets exactly the
   project's declared grants and nothing more. It cannot make a running
   program grant itself access.
3. Every widening of access is visible: a line in `guion.toml`, a flag on the
   command line, or an answer a person typed.
4. Error messages tell the reader the exact line or flag that fixes them, in a
   form a person can paste or an agent can propose.

## Non-goals

- A grant-everything switch.
- Treating "a terminal is attached" as proof that a person is present. Agents
  can run commands inside a terminal and can script answers to it. The
  guarantee is *no silent escalation*, not *no agent can ever answer*.
- Changing how packages get capabilities. `[approved-capabilities.NAME]` and
  `guion add --approve-capabilities` stay as they are; the run-time question
  never approves package capabilities.

## Part 1: the `scratch` profile

`scratch` is the name for what `guion run` already does, plus temp storage:

| Access | `scratch` |
|---|---|
| Read and write the project folder | yes (unchanged) |
| Read and write a private temp folder | yes (new) |
| Clock, HTTP, commands, sockets, environment | only what `guion.toml` or flags grant |

**Temp folder.** Each run gets a fresh folder under the OS temp directory,
reached from Guion code through the virtual prefix `temp:` (the same mechanism
as the existing `pkg:` and `std` aliases, `with_file_root_alias`). Paths under
`temp:` never fall through to the project folder. The folder is removed when
the run ends, including after a failure. `--keep-temp` keeps it and prints its
path.

**`sealed`.** `--sealed` runs with no file access at all, the same as a
replay. It's for running code you haven't read yet. It's the only other
profile, and it adds no new concepts.

`guion test` keeps the project folder grant, gets its own `temp:` folder per
test, and never asks questions (see Part 4).

## Part 2: what a script needs vs. what it has

A new report, `guion inspect --grants [--json] <path>`, compares what the
program can reach with what is granted:

```
needs                          granted?
  fetch api.github.com         no   → [network] hosts = ["api.github.com"]
  run git                      yes  (guion.toml [process])
  read the clock               no   → [clock] enabled = true
  connect to a host chosen     no   → known only while running
    while running
```

- Targets come from literals where the compiler can see them (hosts from URL
  literals, command names from string constants passed to `process_run`,
  environment variable names). Where the target is computed, the report says
  so instead of guessing.
- The JSON form gives each missing grant as a `guion.toml` table, key and
  value plus the equivalent flag. An agent reads this to propose a change for
  a person to review.

This report drives Part 3 and is useful on its own.

## Part 3: one question before the run

When `guion run` finds missing grants with known targets and a person is
present, it asks once, before any Guion code runs:

```
hello.gn wants to:
  · fetch from api.github.com
  · read the clock
Allow? [y] this run  [a] always (saves to guion.toml)  [N] no
```

- **y** grants exactly those targets for this run.
- **a** writes them into the existing `guion.toml` tables, keeping comments
  and formatting, then runs. The change shows up in `git diff`.
- **N** (the default, and what Enter does) runs without them. The program
  fails at the first denied effect with the usual error.

The question uses plain words ("fetch from", "run", "read the clock"), never
flag names or effect kinds.

**When a person counts as present.** All of these must be true:

1. stdin and stderr are terminals,
2. `/dev/tty` (Windows: `CONIN$`) can be opened, and the answer is read from
   there rather than from stdin, so piped input like `yes |` can't answer,
3. `CI` is not set,
4. `GUION_NO_PROMPT` is not set and `--no-prompt` was not passed.

Agent harnesses should set `GUION_NO_PROMPT=1`. Even if one doesn't, the
worst case is that an answer typed into a terminal grants access for one run,
or writes a visible line to `guion.toml`. Neither is silent.

**Without a `guion.toml`** (running a single file), only `y` and `N` are
offered. `a` would have to create a project file the person didn't ask for.

**Never asked** during `--replay`, `--replay-changed`, `--sealed`, `guion test`,
`--json` runs, or when stdout is not a terminal.

## Part 4: asking during the run

Some targets are only known while the program runs, for example a host name
read from a file. When such an effect is denied and a person is present (same
rules as Part 3), the run pauses and asks about that exact target:

```
hello.gn wants to connect to data.example.org:443
Allow? [y] this run  [a] always  [N] no
```

- Questions are serialized, so `parallel_map` workers, and tasks in a
  `together:` block (`design-structured-concurrency.md`), can't interleave
  them.
- Time spent waiting for an answer doesn't count against the HTTP, process or
  socket timeouts.
- `a` writes the exact target, never a wildcard. The question never offers
  `host:*`, "all hosts" or "all commands".
- Effects before the pause have already happened. That's why Part 3 asks
  first whenever it can, and this is only the fallback.

## Part 5: denials that say how to fix them

When there's no one to ask, or the answer is no, every denial names both
fixes:

```
error[E_NET_DENIED]: fetching api.github.com is not granted
  add to guion.toml:   [network] hosts = ["api.github.com"]
  or for one run:      guion run --allow-http-host api.github.com hello.gn
```

- The same shape applies to clock, commands, sockets, environment variables
  and files outside the allowed folders.
- With `--json`, the error carries `grant.table`, `grant.key`, `grant.value`
  and `grant.flag`, so an agent can propose the change without parsing text.
- Error codes don't change.

## Tapes

A recording records which grants came from a question, so a reader can tell
"granted in guion.toml" from "granted by a person at the prompt". Replays
behave as today: no access and no questions.

## Build order

1. Fix-it denials (Part 5). Small, useful on its own, and Parts 3 and 4 reuse
   the grant descriptions.
2. `inspect --grants` (Part 2).
3. Up-front question and `guion.toml` writing (Part 3).
4. `temp:` folder and `--sealed` (Part 1).
5. Mid-run question (Part 4).

Each step gets tests: the same script run with and without a grant; piped
`yes` doesn't answer the question; `CI=1` and `GUION_NO_PROMPT=1` never ask;
`a` produces a minimal `guion.toml` diff that keeps comments; replay never
asks; `temp:` is removed after a failed run; `temp:` paths can't reach the
project folder.

## Decisions on the questions this design raised

1. **Editing `guion.toml` without wrecking it.** Use `toml_edit`. It's already
   in `Cargo.lock` (pulled in by `toml`), so it adds nothing new to the tree.
   Writes add values to the existing table and key, creating the table only if
   it's missing, and never reorder or reformat anything else.

2. **No `guion.local.toml`.** "Always" writes to `guion.toml`. A grants file
   that isn't committed is exactly where an access change could hide: a
   reviewer would never see it, and an agent could add to it unnoticed. Grants
   are names (hosts, commands, variable names), never secret values, so there's
   nothing private to keep out of the repository. People who want a grant for
   themselves only can keep using the per-run flags.

3. **`guion repl` asks, using the same rules as `run`.** A REPL nearly always
   has a person at it. The question appears when an evaluated line hits a
   missing grant (the mid-run case, Part 4). "This run" means the rest of the
   REPL session, and "always" is only offered when the REPL was started inside
   a project. The REPL already reads lines from stdin, so the answer still comes
   from `/dev/tty` to keep the two inputs separate.

4. **Windows reads the answer from `CONIN$`.** Opening `CONIN$` for read and
   write reaches the console's own input even when stdin is redirected, which
   matches `/dev/tty`. If it can't be opened (no console attached, for example
   a detached process or a service), no person is present and nothing is asked.
   This gets a test on the Windows build when it's implemented, rather than an
   assumption.

## Running under Bunks

Bunks (`/Users/rmichaelthomas/bunks`) runs heavy jobs, including `guion test`,
inside a bounded Linux container and limits CPU, memory, processes and disk.
Guion decides what a program may touch; Bunks decides how much of the machine a
job may use. Guion builds these profiles first, in the build order above, and
Bunks consumes them (Bunks decision D9, v0.8 checkpoint §20). Three things must
stay true:

1. **No one is asked inside Bunks.** Bunks always sets `GUION_NO_PROMPT=1`,
   and its jobs have no terminal. A question appearing under Bunks is a defect.
   The Part 3 presence rules already guarantee this; keep it covered by a test.
2. **Denials are a stable machine contract.** The Part 5 `--json` error code and
   `grant.table`, `grant.key`, `grant.value` and `grant.flag` fields are what
   Bunks' permission memory stores, so an agent does not retry the same denied
   effect. Treat changes to those field names as a breaking change, and include
   them in the `guion run --json` report as well as in error output.
3. **`temp:` lives where Bunks puts it.** The Part 1 temp folder must be
   created under the directory named by `TMPDIR` when it is set. Bunks points
   `TMPDIR` at its own size-capped scratch, so `temp:` usage counts against the
   job's disk limit and is removed with the job. Rust's `std::env::temp_dir`
   honors `TMPDIR` on Unix; confirm the implementation uses it and add a test.

## Notes from the build

- `temp:` accepts `temp:name` and `temp:/name`; both reach the private folder, and `temp:` itself
  cannot be removed or renamed.
- A grant that has no `guion.toml` entry (an extra file folder) has `table` and `key` null in the
  `--json` grant, and only the `--file-root` flag. Path escapes with `..` name no folder, so
  they carry no structured grant.
- A "no" to the question before the run means nothing more is asked that run, including about
  targets only known while running.
- `guion repl` runs each entry as its own run, so "this run" is passed to later entries as a run
  flag. The repl is not inside a project, so `[a]` is not offered there. Environment-variable
  grants have no run flag, so they are asked about again on the next entry.
- The `guion repl` permission flags were placed after the file name, where `run` read them as
  program arguments; they now come before it.
- Not checked on Windows: the `CONIN$` answer path is written but has not been compiled or run
  on a Windows build. It needs that test, as planned.
