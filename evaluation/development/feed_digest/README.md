# Legacy feed digest fixture

This directory preserves the pre-rename Veritas project as a migration and
compatibility fixture. It uses `main.vr`, `veritas.toml`, `.veritas/`, and the
original `veritas_feed.py` adapter intentionally.

The current Guion application uses current project names and is maintained in
[`../guion_feed_digest`](../guion_feed_digest/README.md). The acceptance suite
can still run this fixture by setting `GUION_FEED_PROJECT` to this directory;
new examples and managed-package verification should use the Guion project.
