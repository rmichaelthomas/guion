# Builder challenge brief: Queue Digest

## Goal

Build a small command-line tool in Guion that summarizes a local issue queue from a JSON Lines file. A maintainer should be able to run it on a queue export and get a deterministic digest without Python or network access.

## Input

The program accepts one path to a UTF-8 JSONL file. Each non-empty line contains one issue object with:

- `id`: a unique, non-empty text identifier;
- `team`: non-empty text;
- `state`: `open` or `closed`;
- `priority`: `high`, `normal`, or `low`;
- `labels`: a list of text labels.

## Output

Write one JSON object to standard output with:

- `total`: number of issues;
- `by_state`: issue counts for each state;
- `by_priority`: issue counts for each priority;
- `by_team`: issue counts grouped by team;
- `top_labels`: up to three `{label, count}` objects, ordered by count descending and then label ascending.

All known state and priority keys must appear, including when their counts are zero. Counts and grouping must include every valid issue exactly once. Output must be valid JSON and deterministic for the same input.

## Failure behavior

Malformed JSON and invalid issue records must produce a nonzero exit status and an actionable error that identifies the input line. The program must not print a partial summary. A duplicate `id` is invalid. An empty input file is valid and produces a zero-count summary.

## Constraints

Use Guion's default core profile. The program must not invoke Python, contact a network host, or require a grant beyond reading the supplied file under the project root. Keep the implementation small enough to complete and explain within two days.
