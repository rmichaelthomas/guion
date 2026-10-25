# Quote Merge

Quote Merge reads one feed URL per line, fetches the feeds concurrently with a four-worker ceiling and a two-second request deadline, and writes a deterministic `quote-report.json` in the project directory. The project manifest grants HTTP only to `127.0.0.1`.

Run from this directory:

```sh
veritas run src/main.vr feeds.txt
veritas run src/main.vr feeds.txt --max-days 3
```

The report contains the cheapest eligible quote per carrier, sorted by price, carrier, and service; a count of valid feeds; and failed feeds with URL, HTTP status, and error. The optional `--max-days N` filter changes quote eligibility while keeping feed-level counts unchanged.
