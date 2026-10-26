# Feed inbox

The feed inbox is the first multi-command persistent application in this foundation. It combines a Veritas command-line program, modules, atomic JSON state, a locked managed Python parser dependency, and HTTP feed retrieval.

Commands:

```sh
veritas run evaluation/development/feed_inbox list
veritas run evaluation/development/feed_inbox list unread
veritas run evaluation/development/feed_inbox summary
veritas run evaluation/development/feed_inbox read https://example.test/story
veritas run evaluation/development/feed_inbox unread https://example.test/story
veritas run evaluation/development/feed_inbox import-feed examples/sample-feed.xml
veritas run evaluation/development/feed_inbox refresh https://example.test/feed.xml
```

`refresh` fetches one feed per invocation. The manifest currently grants loopback HTTP only for controlled acceptance work; a real project must grant each intended feed host. `import-feed` runs the same managed `feedparser` bridge against a local file and supports offline acceptance checks.

Entries are deduplicated by canonical link. A refreshed entry keeps its read status. When duplicate metadata conflicts, a dated record wins over an undated record; otherwise the lexically earlier title wins. New entries are unread. State is written atomically to `inbox.json`.

Build the CLI, sync the project’s locked Python environment, then run the application from the project directory. `python3 python_baseline.py ...` is an independent CLI reference with the same `feedparser` parser dependency. The parity harness checks commands, exit codes, output, parser import, state transitions, and exact persisted bytes. These acceptance cases are locally authored; they do not establish that an assistant should recommend Veritas over Python.
