# The `web` package

`web` is a Guion package for HTTP applications: a router, middleware, sessions with CSRF tokens, static files and checked templates, over `std:http_response`, `std:cookie`, `std:http_body`, `std:http_static` and `std:http_server`. It is a package, not part of the standard library: it is added to a project, pinned in `guion.lock`, and its capabilities are shown to you before it is installed. This page is the user guide. The package's own README (`packages/web/README.md`) carries the same API in more detail.

The package is on the registry (0.2.0): `guion add web --approve-capabilities`. To use a checkout instead, `guion add web --path PATH/TO/packages/web --approve-capabilities`. The flag is needed because a project approves no package capabilities by default; it records what `web` declares (files, the local network, the clock) in your `guion.toml` under `[approved-capabilities.web]`.

## Quick start

A handler is a function from `Request` to `Result[Response, HttpFailure]`. It names no socket, so a test calls it with a `Request` value and reads the `Response`.

```guion
import "pkg:web/src/lib.gu"

fn show_user(request: Request) -> Result[Response, HttpFailure]:
    let id = unwrap(path_param_int(request, "id"))
    return Ok(response_text(200, "user " + int_to_text(id)))

fn main() -> Int:
    let table = unwrap(router([route("GET", "/users/{id:int}", show_user)]))
    let answer = app_respond(app(table), request_new("GET", "/users/7"))
    print(answer.status)
    return 0
```

Run it with `guion run main.gu`. To listen, end `main` with `serve("127.0.0.1", 8080, app(table), 4)` and run with `guion run --allow-bind 127.0.0.1:8080 main.gu`.

## Handler contract

- `fn(Request) -> Result[Response, HttpFailure]`. Guion has no type aliases, so the type is written out. `HttpFailure(status, code, message)` comes from `std:http_response`.
- `Ok(response)` is sent as it is, whatever its status. `Err(failure)` becomes a `500` with the fixed body `Internal Server Error` (`internal_error_body()`); the failure's status, code and message never reach the client, and are logged through `std:log` when `app_log(app, logger)` set a logger, to standard error otherwise.
- A runtime fault inside a handler (an index out of range, say) answers that request with the same `500` and the fixed body, logs the fault's code, and the server keeps serving. Handlers should still return `Err` for expected failures. A fault caused by the shared instruction limit running out is contained the same way, and so is every request after it.
- A handler is a named function, so state it needs (a database path, a policy) is a constant or a `config` map.

## Routing

`route(method, pattern, handler)` builds a `Route`; `router(routes)` returns `Result[Router, HttpFailure]`.

- A segment is a literal, `{name}` or `{name:text}` (any non-empty text), or `{name:int}` (digits only, no sign, no leading zero, at most 18 digits). `path_param(request, "name")` and `path_param_int(request, "id")` read them.
- `E_ROUTE_PATTERN` for a malformed pattern; `E_ROUTE_CONFLICT` when two routes of one method can match one path. There is no precedence: `/users/{id}` and `/users/new` conflict, `/users/{id:int}` and `/users/new` do not.
- Slashes are significant (no redirect, no trailing-slash folding). `{id:int}` given `abc` is a 404. An encoded slash (`%2F`) stays inside its segment. A path matching only under other methods is a 405 with `Allow`. A GET route does not answer HEAD.
- `router_handle(router, request)` dispatches one request.

## Middleware and applications

Function values capture nothing, so there is no `wrap(mw, handler)` closure. A `Middleware` is a record of two named functions and a settings map:

- `before(config, request) -> Result[Request, Response]`: `Ok(request)` passes the request inward; `Err(response)` answers now.
- `after(config, request, response) -> Result[Response, HttpFailure]`: sees the response on the way out.
- `middleware(before, after)`, `middleware_before(before)`, `middleware_after(after)`, `middleware_configured(before, after, config)` and `middleware_before_configured(before, config)` build one. `layer(before, after)`, `layer_before(before)` and `layer_after(after)` build one from closures that take no config map and may capture variables: `fn(Request) -> Result[Request, Response]` and `fn(Request, Response) -> Result[Response, HttpFailure]`. `wrap(app, layer)` makes it outermost; `app_with(router, layers)` lists layers outermost first; `app(router)` has none.
- The outermost layer's `before` runs first and its `after` last. Middleware wraps the router's own 404 and 405 too.
- `app_handle(app, request)` returns the `Result`; `app_respond(app, request)` always returns a `Response`.

```guion
import "pkg:web/src/lib.gu"

fn page(request: Request) -> Result[Response, HttpFailure]:
    return Ok(response_text(200, "page"))

fn require_key(config: Map[Text, Text], request: Request) -> Result[Request, Response]:
    match request_header(request, "x-key"):
        Some(key):
            return Ok(request)
        None:
            return Err(response_text(401, "key required"))

fn stamp(config: Map[Text, Text], request: Request, response: Response) -> Result[Response, HttpFailure]:
    return response_set_header(response, "X-App", "demo")

fn main() -> Int:
    let table = unwrap(router([route("GET", "/", page)]))
    let built = wrap(wrap(app(table), middleware_after(stamp)), middleware_before(require_key))
    print(app_respond(built, request_new("GET", "/")).status)
    return 0
```

## Serving

`serve(host, port, app, workers)` listens until the listener fails or, in a program that polls for signals, until it is told to stop (see below); `serve_for(host, port, app, workers, idle_ms)` lets a worker stop after `idle_ms` without a connection. `workers` workers share one listener and each handles one connection at a time (pattern of `docs/concurrency.md`), so a stalled client holds one worker and the worker count is a ceiling for waiting, not for computing. Every response carries `Connection: close`. A request is read for at most 10 seconds and 1 MiB. No public function names a connection or listener.

To stop cleanly, call `process_signal_poll()` once at the start of `main` (import `std:process`). That makes `INT`, `TERM` and `HUP` the program's own, and `process_stop_requested()` turns true when one arrives. Each worker accepts with a timeout of at most 200 ms and looks at it between connections: a request being handled is finished and answered, then `serve` (or `serve_for`) returns `Ok` and the lines after it run, within a fraction of a second when the server is idle. A program that does not poll for signals is unchanged: `guion` ends on the signal at once, in-flight requests and all.

## Sessions and CSRF

`session_store_open(path)` opens a SQLite-backed store every worker shares. The browser holds a 64-digit hex ID from the operating system's generator; the database holds only `sha256(ID)`, plus a server secret for CSRF tokens (protect the file like any secret). `session_middleware(store, policy)` finds or creates the session; `session_get`, `session_set`, `session_remove` read and write its values; `session_rotate(store, policy, request, response)` moves it to a new ID on every privilege change and kills the old one; `session_destroy` logs out; `session_purge_expired` removes expired rows.

Default policy, `session_policy_default()`: idle 30 minutes, absolute 12 hours, cookie `__Host-session`, `HttpOnly`, `Secure`, `SameSite=Lax`, `Path=/`, no `Domain`. An ID the server did not issue is never adopted; an ID of the wrong shape is refused before the store is touched.

`csrf_middleware(store, policy)` answers `403` with the fixed body `Forbidden` to any request whose method is not exactly `GET`, `HEAD` or `OPTIONS` unless it carries its session's token in the `x-csrf-token` header or the `_csrf` form field. It also answers `403` when the request's `Origin` header (or, with no `Origin`, its `Referer`) does not name the request's own `Host`; with neither header the token decides. `csrf_token(store, policy, request)` makes the token for a form. List `session_middleware` outside `csrf_middleware`.

```guion
import "pkg:web/src/lib.gu"

fn home(request: Request) -> Result[Response, HttpFailure]:
    return Ok(response_text(200, "home"))

fn main() -> Int:
    let store = unwrap(session_store_open("sessions.db"))
    let policy = session_policy_default()
    let layers = [unwrap(session_middleware(store, policy)), unwrap(csrf_middleware(store, policy))]
    let table = unwrap(router([route("GET", "/", home)]))
    let built = app_with(table, layers)
    print(app_respond(built, request_new("GET", "/")).status)
    return 0
```

Run with `--file-root DIR` for the database directory and `--allow-clock`: expiry reads the clock.

## Static files

`static_files(prefix, root)` is a middleware: a GET or HEAD for a path under `prefix` is answered from the directory `root` by `static_serve` of `std:http_static` (content types, weak `ETag`, `If-None-Match`, `If-Modified-Since`, one byte range, HEAD, confinement under the granted file root, symbolic-link escapes refused). Anything else passes to the router. The prefix matches whole segments: `/assets` serves `/assets/app.css` and not `/assetsX/app.css`. List it outermost to keep session cookies off assets.

```guion
import "pkg:web/src/lib.gu"

fn home(request: Request) -> Result[Response, HttpFailure]:
    return Ok(response_text(200, "home"))

fn main() -> Int:
    let table = unwrap(router([route("GET", "/", home)]))
    let built = app_with(table, [static_files("/assets", "public")])
    print(app_respond(built, request_new("GET", "/assets/app.css")).status)
    return 0
```

## Checked templates

A project that depends on `web` writes `NAME.gut` files; `guion check`, `build`, `run` and `test` compile them to Guion functions returning `Rendered` (syntax in `docs/templates.md`). `html_response(status, rendered)` turns a `Rendered` into a `text/html; charset=utf-8` response. `html_escape_text`, `html_escape_attr`, `html_escape_url` and `html_script_string` are the escapers generated code calls; `html_finish` and `html_embed_rendered` are for generated code only. Templates take `Text` parameters and lists of records; they do not take a `Rendered` from an arbitrary string.

## Reference: the lower-level session functions

Most applications use only the middleware and `session_get`, `session_set`, `session_rotate` and `csrf_token`. The rest of the session surface is for handlers and tests that want the parts:

- Records: `SessionStore` (the database path and the server secret) and `SessionPolicy(idle_seconds, absolute_seconds, cookie_name, secure, same_site)`.
- `session_policy_check(policy)` validates a policy; `session_policy_from_config(config)` and `session_config(store, policy)` carry a store and policy through a middleware's `config` map.
- `session_id_valid(id)` checks the shape of an ID; `session_id_new()` makes one; `session_id_of(policy, request)` reads the one the request presents; `session_cookie(policy, id)` builds the cookie.
- `session_open(store, policy, request)` finds or creates the session and returns its ID; `session_exists(store, policy, request)` only looks.
- `session_open_at`, `session_get_at` and `session_purge_expired_at` take the time in milliseconds instead of reading the clock, for tests.
- `csrf_exempt_method(method)`, `csrf_origin_ok(request)` and `csrf_verify(store, policy, request)` are the three parts of `csrf_middleware`.
- `web_version()` is the package version as text.

## Effects and capabilities

Every public function declares its effects in `guion doc --json`.

| Piece | Effects |
| --- | --- |
| `route`, `router`, `router_handle`, `path_param`, `app`, `wrap`, escapers, `html_response` | none |
| `serve`, `serve_for` | `network.socket`, `concurrency.parallel`, `process.signals` |
| `static_files` | `filesystem.read` |
| `session_*` and `csrf_*` that read the time | `clock.read` |
| `app_respond` | `filesystem.write` (the standard-error log path) |

The package's `[capabilities]` are `files = ["read", "write"]` (sessions database, static files), `network = ["127.0.0.1"]` (listening) and `clock = true` (session expiry). It reads no environment variable and starts no process. A project that installs `web` approves those grants once; widening them in a later release is refused until you approve the change.

## What `web` does not do

- Middleware is a record of named functions plus a `config` map; the package has not been rewritten to use closures, although the language has them now.
- A runtime fault in a handler is contained per request (a 500 and a log line); an `Err` is contained the same way.
- `guion doc` reports the session database as file access (`filesystem.read` and `filesystem.write`), so the `files` capability and `--file-root` cover it.
- CSRF is the token plus the `Origin`/`Referer` check against `Host` (the scheme is not compared, and a proxy must pass `Host` through); there is no defence against a script already running in your page.
- Templates are `Text` and records only; there is no filter, flag or argument that turns escaping off.
- No keep-alive, no TLS, no HTTP/2, no websockets, no request logging beyond failures.
- No password hashing, user accounts or authorisation: sessions hold values, nothing more.
