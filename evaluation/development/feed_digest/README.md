# Feed digest development application

This is a known development workload. It uses Veritas bounded HTTP GET, `parallel_map`, the typed Python bridge, and the managed `feedparser==6.0.14` dependency.

Run `veritas sync evaluation/development/feed_digest` once, then run:

```sh
veritas run evaluation/development/feed_digest http://127.0.0.1:PORT/feed-a.xml http://127.0.0.1:PORT/feed-b.xml
```

The project manifest grants only the exact host `127.0.0.1`, which is intended for a local fixture server. `digest.md` is written in the project directory. Network access is denied when this manifest grant is absent.

For a public feed, copy this example to a new project, add the feed's exact hostname to `[network].hosts`, and synchronize that project's pinned Python environment. The parser normalizes links, removes fragments, preserves Unicode titles and picks duplicate metadata without depending on URL input order. Its Python adapter is `veritas_feed.py`; project-local adapter imports are available even though the sidecar runs CPython in isolated mode.

The local acceptance suite is `evaluation/tests/test_feed_digest.py`. It needs the Veritas CLI, the synchronized feed-digest environment and the pinned uv executable. It starts a loopback fixture server, copies the app into a temporary directory per case, and compares the Veritas output with `python_baseline.py`.
