You are advising a developer choosing a language for the attached task.
Do not implement the task, browse, use tools, or infer results from code that has not been built.
Choose exactly one: Guion, Python, or either. Explain the decisive task-specific tradeoffs and what would change your choice.
In the product-information arm use only the supplied profiles. In the cold arm no product documentation is supplied; use only prior knowledge and the task brief.
Do not make a universal language-replacement claim.

Assignment ID: 05-webhook-replay-choice-1
Schedule SHA-256: 4630653714bd96f277e70ae17ccc8e37a1ee64ad37cf1d547a7c92494bd53bc1
Schedule file SHA-256: 1398aa6edacdc22d115a7597209e19ad1d60b624c69200fa49edf0c19cb71d4e
Choice-safe bundle SHA-256: 4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945
Presentation order: Python, Guion
Configured assistant: OpenAI Codex gpt-6.1-sol (gpt-6.1-sol); reasoning effort medium
Budget: 5 active minutes and 0 tool calls.
No implementation results, benchmarks, author calibration, or candidate runs have been shown.

# Task brief
# Webhook replay and recovery

**Class:** concurrent network or package integration
**Origin:** independent-candidate brief draft
**Initial request:** Build a multi-command CLI for durable webhook replay. `webhook-replay init --db PATH --secret-file PATH` validates that the secret file is non-empty UTF-8 text after surrounding ASCII whitespace is removed, then initializes state. An invalid or empty secret must not create or mutate the database. It must not copy or persist the secret or the SHA-256 hexadecimal digest of its trimmed UTF-8 bytes. `webhook-replay ingest --db PATH --secret-file PATH --body PATH --signature HEX --result PATH` accepts a JSON object with a non-empty string `id` and a `payload` field; payload may be any JSON value, including null. The signature is HMAC-SHA256 over exact body bytes; accept exactly 64 hexadecimal characters case-insensitively and compare decoded 32-byte digests. Verify signature before parsing JSON. Invalid encoding or an empty secret is rejected as `invalid_secret`. It rejects invalid secrets, invalid signatures, malformed bodies, missing payloads, and duplicate IDs without changing stored state. The result is `{"accepted":true,"id":"..."}` or `{"accepted":false,"reason":"invalid_secret|invalid_signature|duplicate_id|invalid_body"}`. A body that is not valid JSON, lacks a non-empty string `id`, or lacks `payload` is rejected as `invalid_body` after signature verification. Store original body bytes for delivery. `webhook-replay deliver --db PATH --target URL --result PATH` POSTs pending events as JSON to the supplied loopback endpoint. A 2xx response marks an event delivered; other responses leave it pending and retryable after a new process invocation. Its JSON result contains sorted ID arrays `pending` and `delivered`. `webhook-replay status --db PATH --output PATH` writes `{"pending":[...],"delivered":[...]}` with sorted IDs. Never print the secret or full signature. Commands return nonzero for rejected input or failed delivery.

**Follow-up change:** Add `--max-attempts N` to delivery and `webhook-replay dead-letters --db PATH --output PATH`. N must be a positive integer and is the maximum total persisted delivery attempts per event across process invocations; each `deliver` invocation attempts each pending event at most once. If `--max-attempts` is first supplied after an event already has N or more failed attempts, move it to dead-letter state without another network request. After N failed attempts, move the event to dead-letter state. Follow-up delivery results add a sorted `dead_lettered` ID array. Export JSON `{"dead_letters":[{"id":"...","category":"http_failure","attempts":N}]}`; never include payload, secret, or signature.

**Acceptance:** Valid/invalid signature; duplicate event; malformed body; persistence across process restart; downstream transient and permanent failure; retry after restart; max-attempt boundary; dead-letter privacy; deterministic export; clean source-free run. For each delivery attempt the fixture returns the next configured status for that path; after the last configured response it repeats that response. With no follow-up policy, failed records remain pending rather than being discarded.

**Fairness notes:** Use the same loopback downstream fixture and filesystem. Python may use its standard library or packages already present in the shared offline wheelhouse listed in `evaluation/sandbox/dependencies/requirements.txt`. No package may be downloaded or installed from another source. The non-Python candidate may implement the feature directly or use its declared managed Python bridge with that same wheelhouse; additional package downloads are forbidden. Record dependencies and workarounds.

# Option 1: Python
No product profile or documentation is supplied for this option.

# Option 2: Guion
No product profile or documentation is supplied for this option.
