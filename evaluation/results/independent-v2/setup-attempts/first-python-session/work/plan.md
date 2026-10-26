# Initial implementation plan

Implement the supplied initial CLI contract in Python using only the standard library.
The supplied brief is the approved specification. Execute serially in this workspace.

1. Add black-box CLI tests for exact output, UTC/floor arithmetic, input validation,
   strict timestamps, threshold/closed behavior, diagnostics and output preservation.
2. Implement `src/sla_report` with argparse, strict CSV/JSON validation and atomic replacement.
   Canonical `as_of` is UTC ISO 8601 with `Z`, preserving nonzero microseconds.
3. Build a zipapp, deliver the dependency lock, and test only copied artifacts from a
   separate directory with no source path in its import path.
4. Preserve the initial source, package, test results, command history and immutable
   stage record. Leave evaluator-only measurements null. Await follow-up.

Review focus: offset bounds, invalid Gregorian dates, fractional precision, empty or
malformed CSV rows, duplicate IDs, JSON booleans and duplicate object keys, integer
arithmetic without floating point, and preserving an existing output on all failures.
