# Prior language build lessons applied to Veritas

This note records a read-only review of Planes, Liminate, and Decan. Their
repositories remain unchanged. The purpose is to transfer observed engineering
lessons, not their language designs or product boundaries.

## Planes: test the whole program users actually run

Planes' browser/game work made runtime behavior visible under repeated real
interaction. Its Horizon kernel and rendering investigations use pre/post
measurements, and the core-sufficiency report caught a missing `when` construct
only after checking the entry program's complete imported module graph. The
important lesson is that checking one file or one successful demo does not
establish that the usable program works. Repeated frames, imported files,
runtime inputs, and resource behavior expose gaps that syntax-only tests miss.

Applied to Veritas:

- Future acceptance cases must compile the complete package/module graph and
  exercise repeated calls and long-running loops, not just one-shot `main`.
- Add executable workloads with independently computed outputs and record
  startup, steady-state, tail latency, memory, and failure behavior separately.
- Treat feature claims as graph-wide: every reachable source dependency and
  host operation must be represented in the acceptance case.

## Liminate: keep syntax bounded and make interpretation observable

Liminate's pipeline separates lexing, parsing, semantic analysis, and
interpretation. Its bounded vocabulary, explicit ambiguity behavior, canonical
rendering, and structured results keep natural-language syntax from becoming
silent guesswork. Its tests double as grammar artifacts, so ambiguity is
resolved against concrete examples before implementation claims are made.

Applied to Veritas:

- Unsupported or ambiguous syntax must produce a stable source diagnostic;
  the parser must consume the complete input.
- Keep the source language small enough to teach and give each surface an
  executable example. Prefer composable standard libraries over an expanding
  set of magic built-ins.
- Diagnostics, formatted source, dependency reports, and capability inspection
  are part of the developer experience, not an afterthought to runtime speed.

Liminate's prose syntax and domain packs are not copied: Veritas remains a
general-purpose programming language with conventional source constructs.

## Decan: report exact support and preserve losses

Decan distinguishes a deterministic core from schedulers and consumers. It
publishes a support matrix, requires explicit context snapshots, and makes
adapters report whether semantics were preserved or lost. Its three-consumer
corpus is based on inspected real needs, while its general resolver remains
explicitly partial where evidence is incomplete.

Applied to Veritas:

- Publish supported target, host, dependency, and interop surfaces as a
  capability matrix with exact unsupported reasons.
- Keep runtime capabilities explicit; external calls are visible effects, not
  magically safe because a signature is declared.
- Interop and packaging must report copies, conversion limits, platform
  dependence, and omitted behavior. Never imply a translation is lossless
  unless the evidence demonstrates it.
- Use several real consumer programs to guide compatibility work. Do not infer
  general-purpose readiness from a single tailored application.

Decan's snapshot-only temporal model and no-execution product boundary are not
carried into Veritas; its evidence and boundary-reporting discipline is.

## Build decisions

1. Keep Veritas general-purpose and use the existing small sales-report CLI
   only as one workload among language, interop, concurrency, and packaging
   acceptance cases.
2. Add multi-file modules and ordinary records/maps/options as real language
   features before describing the compiler as general-purpose.
3. Expand evaluation to include whole-project builds, repeated invocation,
   failure injection, resource limits, and package relocation.
4. Keep a public support report that distinguishes implemented, tested,
   platform-limited, and planned features. Benchmark evidence remains
   workload-specific until there are multiple independent workloads and hosts.

