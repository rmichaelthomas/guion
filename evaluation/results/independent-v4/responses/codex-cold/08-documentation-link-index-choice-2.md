**Python.**

This task fits Python’s standard library closely: SQLite transactions, strict UTF-8 decoding, JSON parsing, Unicode case-folding, and filesystem traversal. These directly support the hardest requirements: preserving the previous index on failed builds, validating aliases with JSON escaping, deduplicating matches by page, and producing deterministic Unicode-aware output with UTF-8 byte tie-breakers.

The follow-up also favors explicit, maintainable parsing: recognize the exact front-matter boundaries before interpreting aliases, then keep destination resolution separate from backlinks’ exact normalized-term matching. Python supports organizing those rules across small modules without extra dependencies.

No Guion profile is supplied, and my prior knowledge is insufficient to establish equivalent facilities or maintenance costs. That uncertainty is decisive for this particular library-style CLI; it is not evidence that Guion cannot implement it.

I would reconsider if Guion’s Unicode, SQLite transaction, filesystem, JSON, and multi-module facilities were established and the maintaining team had stronger Guion expertise. This choice reflects implementation fit and confidence, not measured performance or demonstrated acceptance results.
