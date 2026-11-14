# Comprehension harness

Measures whether an outsider, given only what ships in `share/guion/`, can find and use
Guion's capabilities: how many tasks they close, and how many false statements about the
language they make on the way. Not a performance benchmark. Current version: **H-1.1**
(`HARNESS.md`). The earlier version is kept as `HARNESS-H-1.0.md`.

    python3 run.py setup t1 1          # isolated root with the distribution and one starter
    python3 run.py score t1 ROOT       # re-run the subject's program; never trust its own check
    python3 run.py check TRANSCRIPT --root ROOT
    python3 run.py receipt t1 1 3 --root ROOT --subject "..." --success "yes — ..." --wrong 'W1:6:"quote"'

Receipts are append-only in `receipts/`. Ground truth lives in `reference/` and is re-run
against the artifact before every campaign. Changing any task, starter, prompt or the taxonomy
makes a new version.
