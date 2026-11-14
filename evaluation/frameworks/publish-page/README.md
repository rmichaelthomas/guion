# publish-page: the F10 slice

One narrow path that touches every web seam once: publish one page. It depends on the `web` package by path (`[dependencies.web]` in `guion.toml`) and contains no copy of its code.

## What it does

| Behaviour | Where |
|---|---|
| `POST /api/pages`: a block model as JSON, checked by the schema (a paragraph or an image; any other block type is refused with 422) | `src/model.gu`, `src/routes.gu` |
| `GET /p/{slug}`: reads the page from SQLite (`sqlite_begin_read`) and renders it through a checked template, with a publish form that carries the CSRF token | `src/store.gu`, `src/templates/view.gut` |
| `POST /p/{slug}/publish`: CSRF-gated, writes the static bundle `dist/SLUG/index.html` | `src/routes.gu`, `src/templates/page.gut` |
| `GET /static/{path}`: files from `public/` with `ETag`, `Range` and confinement (the `static_files` middleware of `web`) | `src/routes.gu` |
| `POST /api/pages/{id}/cover`: one multipart file part, stored under `uploads/`, resized into `public/` by an external command, the upload removed | `src/cover.gu` |
| A session cookie on the first request | `session_middleware` in `src/routes.gu` |

There are four routes in the router, plus the static files layer and the session layer: the six behaviours the brief lists. A seventh route or a second page type is a stop: record the finding and fix the framework instead.

## Run it

```
guion update            # records the absolute path of packages/web in guion.toml and writes guion.lock
guion run src/main.gu 8080 4
```

`guion update` rewrites the relative `path` of the dependency to an absolute one on your machine: do not commit that change. The two arguments are the port and the number of workers. `guion.toml` grants the clock (session expiry), one local port and one command (`sips`; on Debian name `convert` under both `[resize]` and `[process]`). The application makes `pages.db`, `sessions.db`, `public/`, `uploads/` and `dist/` in this directory.

The first request gets a session cookie and an `X-CSRF-Token` header. A write sends the token back as the `x-csrf-token` header (JSON and multipart) or the `_csrf` form field (the publish form).

## Checks

- `guion test tests.gu`: the application driven by `Request` values, no socket.
- `python3 run_checks.py --guion PATH`: the size cap (source under `src/`, tests excluded: printed, near 600 lines, fails over 700), the capability block (at most 40 lines and 8 grants), the three fixture pages rendered by Guion and by `twin/render.py` and compared byte for byte, and the effects `guion doc --json` reports per function.
- `crates/cli/tests/f10_publish_page.rs`: the server over a real socket (CSRF, session fixation, hostile inputs, uploads and ranges, the limits at the limit and one over, workers sharing the listener, a page read while another process holds a write transaction).

`twin/render.py` renders the same block model with the Python standard library. It is the oracle for the one output where a subtle difference is a real difference; it has no server, session or upload.
