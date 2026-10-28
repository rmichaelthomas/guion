# Guion feed digest

This current-format application fetches RSS/Atom feeds with bounded HTTP concurrency, parses them with the managed pure-Python `feedparser==6.0.14` package, deduplicates canonical links, and writes a deterministic `digest.md`.

The manifest grants HTTP access only to `127.0.0.1` for the local acceptance fixture. Edit the exact host list before using public feeds. The Python adapter is `guion_feed.py`; `python_baseline.py` is the independent acceptance reference.

Run from the Guion repository after building a `python-interop` CLI and synchronizing the managed dependency lock:

```sh
guion sync --update evaluation/development/guion_feed_digest
guion run evaluation/development/guion_feed_digest http://127.0.0.1:PORT/feed-a.xml http://127.0.0.1:PORT/feed-b.xml
```

The first sync creates the checked-in lock; later `guion sync` runs verify it.

The acceptance suite is `evaluation/tests/test_feed_digest.py`; the managed package verifier is `evaluation/completion/performance/verify_managed_feed_digest_package.py`.
