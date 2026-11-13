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

The numbers are measurements on the machine that ran them, not guarantees.
