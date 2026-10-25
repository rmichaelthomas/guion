**Choose python.**

This task centers on filesystem traversal, precise Unicode normalization, JSON parsing, and transactional SQLite updates. Python’s standard library provides direct facilities for those requirements, including the explicitly permitted SQLite module. That makes it the stronger fit for a maintainable, multi-module CLI with strict edge cases.

The decisive requirements are:

- **Preserving the previous index after failure.** SQLite transactions support committing a complete rebuild only after document and metadata validation succeeds.
- **Exact text and ordering behavior.** Unicode case-folding, whitespace handling, strict UTF-8 decoding, and UTF-8 byte tie-breakers can be expressed directly.
- **Narrow front-matter parsing.** Exact delimiter checks plus JSON parsing support the specified aliases format without introducing a broader YAML interpretation.

Veritas offers useful typed modules, explicit effects, resource limits, and managed packaging. However, the supplied documentation does not establish native SQLite, directory traversal, or full Unicode normalization facilities sufficient for this task. Its Python bridge provides a plausible route, but delegating these central operations to Python adds adapter, locking, packaging, and maintenance work. Veritas’s documented experimental compatibility also matters for a maintainable application.

I would reconsider if Veritas supplied verified native support for these operations, or if integration into an existing Veritas application made its policy controls and packaging especially valuable.

This is a task-specific capability recommendation. No implementation, correctness result, packaging success, or performance advantage has been demonstrated.
