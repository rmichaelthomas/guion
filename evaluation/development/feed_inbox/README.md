# Feed inbox

The feed inbox is the first multi-command persistent application in this foundation. It combines a Guion command-line program, modules, atomic JSON state, a locked managed Python parser dependency, and HTTP feed retrieval.

Commands:

```sh
guion run evaluation/development/feed_inbox list
guion run evaluation/development/feed_inbox list unread
guion run evaluation/development/feed_inbox summary
guion run evaluation/development/feed_inbox read https://example.test/story
guion run evaluation/development/feed_inbox unread https://example.test/story
guion run evaluation/development/feed_inbox import-feed examples/sample-feed.xml
guion run evaluation/development/feed_inbox refresh https://example.test/feed.xml
```

`refresh` accepts any number of feeds and fetches up to four at a time. It uses `args_from(1)` to pass every URL after the command to `parallel_map`. The manifest currently grants loopback HTTP only for controlled acceptance work; a real project must grant each intended feed host. `import-feed` runs the same managed `feedparser` bridge against a local file and supports offline acceptance checks.

Entries are deduplicated by canonical link. A refreshed entry keeps its read status. When duplicate metadata conflicts, a dated record wins over an undated record; otherwise the lexically earlier title wins. New entries are unread. State is written atomically to `inbox.json`.

Build the CLI, sync the project’s locked Python environment, then run the application from the project directory. `python3 python_baseline.py ...` is an independent CLI reference with the same `feedparser` parser dependency. The parity harness checks commands, exit codes, output, parser import, state transitions, and exact persisted bytes. These acceptance cases are locally authored; they do not establish that an assistant should recommend Guion over Python.
