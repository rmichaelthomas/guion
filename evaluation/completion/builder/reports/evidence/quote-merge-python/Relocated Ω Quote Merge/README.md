# Quote Merge

Requires Python 3.14.7; no installation or third-party packages are needed.

Create a UTF-8 text file with one HTTP or HTTPS quote-feed URL per line. Blank
lines are ignored. Run from this folder:

```sh
python3 quote_merge.py feeds.txt
python3 quote_merge.py feeds.txt --workers 2 --timeout 1 --max-days 2 --output report.json
```

The report contains `quotes`, `successful_feeds`, and `failed_feeds`. Quotes are
the cheapest eligible entry per carrier, sorted by price, carrier, and service.
Within a carrier, equal prices prefer the alphabetically earlier service, then
the lower delivery estimate. Failure entries contain `url` and `status`: an HTTP
status number, `malformed`, `timeout`, `network_error`, or `invalid_url`.
Successful feed counts include feeds with empty quotes or no eligible quotes.
Failed feeds are listed in input order. Duplicate URL lines are separate feeds.

Concurrency defaults to 4 and must be at least 2. Request socket timeout defaults
to 2 seconds and must be positive and finite. It bounds socket operations; it is
not a total wall-clock deadline for a server that continuously trickles data.
Prices and days must be nonnegative integers, and carrier/service nonempty
strings. Any invalid quote makes its entire feed malformed. Per-feed failures
remain in the JSON report and do not change the process success exit code.
Invalid command options or input/output file errors exit with status 2.

`--max-days N` accepts a nonnegative integer and filters before selecting a
carrier's winner. Without it, all quotes are eligible.

For the supplied local fixture, start `python3 fixtures/quote_server.py`; its
first output line is the allocated port. Construct feed URLs using only
`http://127.0.0.1:PORT/feed-a`, `/feed-b`, `/feed-c`, `/slow-503`, and `/malformed`.
No external network is needed for acceptance checks.
