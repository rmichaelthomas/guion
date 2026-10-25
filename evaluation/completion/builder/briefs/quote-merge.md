# Assignment: Quote Merge

**Target class:** concurrent network integration

Build a command-line tool that reads a text file of quote-feed URLs, fetches them concurrently, and writes a deterministic JSON report. Each successful endpoint returns `{"quotes":[{"carrier":"…","service":"…","price_cents":1234,"days":2}]}`. The report lists the cheapest eligible quote per carrier, ordered by price then carrier and service, the number of successful feeds, and failed feeds with their URL and HTTP/error status. A failed or malformed feed must not discard valid feeds. Use bounded concurrency and a finite timeout.

The supplied fixture server serves three feeds with known quotes and one delayed HTTP 503 response. The delay is long enough to distinguish concurrent requests from serial fetches without making timing the correctness criterion. Grant network access only to the fixture host.

**Follow-up change:** add `--max-days N`, excluding quotes whose delivery estimate exceeds N while retaining feed-level success/failure counts. Existing output without the option must remain unchanged.

**Acceptance:** verify the best quote, stable ordering, partial success on the 503, malformed-feed isolation, timeout/error reporting, a concurrency limit of two or more, the follow-up filter, and relocation to a Unicode-and-spaces path. Record observed elapsed time but prioritize correctness. No third-party Python packages.
