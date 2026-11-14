# Seam inventory

Internal file. It maps each boundary a program crosses to the outside world, against Python's crossing and against Guion's state. It is not shipped and is not public copy. The item column names the Batch 5 item that closes a row (F1 to F10 of the frameworks brief), or "later brief" where the work is out of this batch, or "none" where Guion already has the crossing.

Source: `/Users/rmichaelthomas/opencode-experiment/guion-try/BRIEF-guion-frameworks.md` §1, with the Browser row updated (see the note under the table).

| Seam | Python's crossing | Guion 0.2.0 | Batch 5 item |
|---|---|---|---|
| Read/write files | builtin, pathlib | `std:fs` — have it | none |
| Line streaming | builtin | `std:fs`, `std:jsonl` — have it | none |
| Structured data | `json`, `csv` | `std:json`, `std:csv` — have it | none |
| Persistence | `sqlite3` | `std:sqlite` — have it | F8 (session store), F10 (page store) |
| Text, regex, unicode | builtin, `re` | `std:text`, `std:regex`, `std:unicode` — have it | none |
| Subprocesses | `subprocess` | `std:process` — have it | F10 (cover resize) |
| Outbound HTTP | `requests`, `httpx` | `std:http` — have it | none |
| Inbound HTTP | Flask, Django, FastAPI | raw `std:http_server` — primitives only | F3 (response value), F4 (cookies), F5 (body parsing), F7 (router); F10 proves composition |
| Clock | `datetime` | `std:time` — have it, denies by default | none (F8 expiry uses it when the policy asks) |
| Entropy | `secrets`, `os.urandom` | `crypto_random_*` — have it (B12) | none |
| Config formats | `tomllib`, pyyaml | `std:toml`, `std:yaml` — have it (B10) | none |
| Compression | `gzip`, `tarfile` | `std:compress` gzip, zlib, tar — have it (B5) | none |
| Images | Pillow | none | later brief (F10 resizes through an external command, no codec) |
| Media/audio/video | ffmpeg bindings | none | later brief |
| Auth, sessions, CSRF | framework | none | F8 |
| HTML templating | Jinja2 | none | F9 |
| Static files, caching | framework | none | F6 |
| Concurrency | asyncio, gunicorn | worker pool — have it (B11 pattern) | F7 (`serve`), F10 (concurrency assertions) |
| Observability | `logging`, OpenTelemetry | `std:log` — have it (B6); traces none | later brief (traces) |
| Packaging | pip + PyPI | `package`/`install`/registry — have it, one package | F1 (framework hosting), F2 (effects across packages) |
| Browser | JavaScript (DOM, fetch, events) | **half-crossed** — the language runs in a browser through ABI 1.0 and the seam is half-crossed; the VM runs with zero capabilities | later brief (browser-side capabilities and DOM surface; ABI 1.0 documented by B13) |

## Note on the Browser row

The row says the language runs in a browser through ABI 1.0 and that the seam is half-crossed. The brief's §2 sentence "no wasm target exists" is not carried over: the public playground compiles and runs Guion in the browser today (`guion_compiler.wasm` and `guion_vm.wasm`, browser ABI 1.0, documented in `docs/browser.md` since B13). What is missing is the other direction: a browser-side program cannot reach the page, and host operations return `R_HOST_UNSUPPORTED` with no capabilities granted.
