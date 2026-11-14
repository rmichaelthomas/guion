# Outside JSONL summarizer

A small log summarizer written outside this project, kept as a regression
guardrail. It reads `access.jsonl` from the working directory and prints
request counts per status class, the error rate and the most requested path.

- `src/main.gu` is the Guion program; `summarize.py` is the Python twin.
- `gen.py ROWS PATH` writes a deterministic access log. Data is generated
  into a temporary directory by `run.py` and never committed.
- `run.py` times Guion, Python and a hello-world program (median of N runs) and
  exits non-zero if the Guion and Python outputs differ byte for byte.

```
python3 evaluation/outside/jsonl-summary/run.py --binary target/release/guion \
    --rows 5000 200000 --reps 7
```

`run.py --json PATH` also writes the medians as JSON. `baseline-0.2.0.json` holds
the 0.2.0 medians (copied from the Batch 4 results file so the check does not
depend on ignored files). `cargo xtask outside-check` (after `cargo build --locked --release -p guion-cli`;
it does not build, because hello world measures slow for a while after a build)
runs `run.py` and fails if the Guion and Python outputs differ or if the hello
world, 5,000-row or 200,000-row Guion median is more than 5% slower than the
baseline. The 5% is the noise allowance: repeated runs of one build on the
development machine moved by about that much (hello world, at a few
milliseconds, can move more; a single over-allowance result is worth one re-run
before it is treated as a regression).

The numbers are measurements on the machine that ran them, not guarantees.
