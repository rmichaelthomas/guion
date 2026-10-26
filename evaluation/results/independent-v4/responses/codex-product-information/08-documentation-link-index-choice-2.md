**Choose Python.**

This task’s main challenge is precise behavior: Unicode normalization and byte-based tie-breakers, strict front-matter validation, alias deduplication by page, and preserving the previous SQLite index after any failed build. Python’s mature standard library and explicitly permitted SQLite module make it the lower-risk choice for a maintainable, multi-module CLI.

Guion has relevant documented capabilities: modules, Unicode case-folding and whitespace collapse, JSON parsing, SQLite transactions, and typed error handling. It is a credible candidate. However, its relative-path-only file policy adds integration work for the CLI’s `PATH` arguments and separately located documentation, database, and output files. Its pre-release compatibility policy also adds maintenance risk.

Neither language automatically guarantees the required semantics. Transaction boundaries, exact delimiter handling, deterministic ordering, and backlinks matching the query term without alias expansion all need deliberate implementation.

I would change my choice if source-free distribution without Python or caller-controlled file capabilities became primary requirements, and the deployment could accommodate Guion’s path grants and pinned toolchain. No supplied evidence establishes a performance advantage for either language on this task.
