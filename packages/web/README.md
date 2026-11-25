# web

Routing, middleware and serving for Guion applications, built on `std:http_response`, `std:http_server` and `std:log`.

Use it from a project with `guion add web --path PATH/TO/web --approve-capabilities` and `import "pkg:web/src/lib.gu"`. Check the package with `guion check tests.gu` and test it with `guion test tests.gu`. The package is published to the registry as `web` 0.2.0, so `guion add web --approve-capabilities` installs it without a checkout. The user guide is `docs/web.md` in the repository.

```
import "pkg:web/src/lib.gu"

fn show_user(request: Request) -> Result[Response, HttpFailure]:
    let id = unwrap(path_param_int(request, "id"))
    return Ok(response_text(200, "user " + int_to_text(id)))

fn main() -> Int:
    let table = unwrap(router([route("GET", "/users/{id:int}", show_user)]))
    match serve("127.0.0.1", 8080, app(table), 4):
        Ok(done):
            return 0
        Err(failure):
            print(failure.code)
            return 1
```

Run it with `guion run --allow-bind 127.0.0.1:8080 main.gu`.

## Handlers

A handler is `fn(Request) -> Result[Response, HttpFailure]`. Guion has no type aliases, so write the type out. It names no socket, so a test calls it with `request_new("GET", "/")` and reads the `Response`. `HttpFailure(status, code, message)` is the failure type of `std:http_response`; the built-in `HttpError` cannot be built in Guion code.

`request.peer` (new in 0.3.0, needs Guion 0.9.0 or later) is the client's address as `"host:port"`, an IPv6 host in brackets (`"[::1]:4000"`), and `""` when the request did not come from a connection (a `request_new` value in a test). The router and the built-in middleware pass it on unchanged, so handlers and middleware see it. Behind a proxy it is the proxy's address, not the end client's.

## Routes

`route(method, pattern, handler)` returns a `Route`. A pattern starts with `/`; a segment is a literal, `{name}` or `{name:text}` (any non-empty text) or `{name:int}` (digits only, no sign, no leading zero, at most 18 digits). `router(routes)` returns `Result[Router, HttpFailure]`:

- `E_ROUTE_PATTERN`: a malformed method or pattern, such as `a`, `/a/`, `/a//b`, `/a/{id`, `/a/{id:float}` or a parameter named twice.
- `E_ROUTE_CONFLICT`: two routes with the same method that can match one path. There is no precedence order: `/users/{id}` and `/users/new` conflict; `/users/{id:int}` and `/users/new` do not. Different methods on one path are fine.

`router_handle(router, request)` finds the route and calls its handler with `Request.params` filled in. `path_param(request, "slug")` returns `Option[Text]`; `path_param_int(request, "id")` returns `Option[Int]`.

- The raw path is split on `/` first and each segment is percent-decoded after, so `/files/a%2Fb` gives the parameter `a/b` and `/files/a/b` is a 404. A bad percent escape, invalid UTF-8 or `%00` is a 400.
- `{id:int}` given `abc` (or `-1`, `007`, `1.5`) is a 404, never a 500.
- Trailing slash policy: slashes are significant. `/a/` does not match a route registered as `/a`, a pattern cannot end in `/` (except the root), and there is no redirect. Empty segments (`//`) never match. The router does not interpret `.` or `..`.
- A path that matches under other methods is a 405 with an `Allow` header; any other path is a 404. A GET route does not answer HEAD.

## Middleware and applications

A middleware is a record of two functions and the settings they read (`config`, a `Map[Text, Text]` passed to both on every request), and an `App` holds the router and the layers:

```
fn require_key(config: Map[Text, Text], request: Request) -> Result[Request, Response]:
    match request_header(request, "x-key"):
        Some(key):
            return Ok(request)
        None:
            return Err(response_text(401, "key required"))

fn stamp(config: Map[Text, Text], request: Request, response: Response) -> Result[Response, HttpFailure]:
    return response_set_header(response, "X-App", "demo")

fn build(table: Router) -> App:
    return wrap(wrap(app(table), middleware_after(stamp)), middleware_before(require_key))
```

- `before(config, request) -> Result[Request, Response]`: `Ok(request)` passes the request inward, `Err(response)` answers now.
- `after(config, request, response) -> Result[Response, HttpFailure]`: sees the response on the way out.
- `middleware(before, after)`, `middleware_before(before)` and `middleware_after(after)` build a layer with an empty `config`; `middleware_configured(before, after, config)` and `middleware_before_configured(before, config)` build one with settings (the session layer's store and policy travel this way); `layer(before, after)`, `layer_before(before)` and `layer_after(after)` take closures that read no config map and may capture variables; `wrap(app, layer)` makes it the outermost; `app_with(router, layers)` lists layers outermost first.
- Order is deterministic: the outermost layer's `before` runs first and its `after` last. When a `before` answers with `Err(response)`, that layer's `after` does not run, and the `after` of every outer layer does, innermost first. An `Err(HttpFailure)` skips the remaining `after` functions.
- Middleware wraps the router's own 404, 405 and 400 responses too.
- Effects attach where a function value is formed, so the function that writes `middleware_before(audit)` reports the effects of `audit`.

## Static files

`static_files(prefix, root)` is a middleware: a GET or HEAD for a path below `prefix` is answered from the directory `root` by `static_serve` of `std:http_static` (content types, weak `ETag`, conditional requests, one byte range, HEAD, confinement under the file root); everything else goes on to the router. The prefix matches whole segments (`/assets` serves `/assets/app.css`, not `/assetsX/app.css`). List it outermost to keep session cookies off assets. Effect: `filesystem.read`; run with `--file-root` for the asset directory.

```
let site = app_with(table, [static_files("/assets", "public")])
```

## Sessions

`session.gu` keeps sessions in a SQLite database that every worker opens, so a request can land on any worker. The browser holds only an ID: 32 bytes from the operating system's generator, written as 64 lowercase hex digits. The database stores `sha256(ID)`, never the ID, so a copy of the file does not hand out sessions. It also holds a server secret (32 random bytes, made once) for the CSRF tokens; protect the file like any other secret.

```
fn db() -> Text:
    return "sessions.db"

fn login(request: Request) -> Result[Response, HttpFailure]:
    let store = session_store_open(db())?
    let policy = session_policy_default()
    let saved = session_set(store, policy, request, "user", "alice")?
    return session_rotate(store, policy, request, response_text(200, "welcome"))

fn build(table: Router) -> App:
    let store = unwrap(session_store_open(db()))
    let policy = session_policy_default()
    let layers = [unwrap(session_middleware(store, policy)), unwrap(csrf_middleware(store, policy))]
    return app_with(table, layers)
```

Handlers cannot hold a store, so a handler opens it with `session_store_open(path)` (the path is the application's own constant; opening is a read once the file exists) and passes the same policy. Run the app with `--file-root` for the database directory and `--allow-clock`: expiry reads the clock.

- `SessionPolicy(idle_seconds, absolute_seconds, cookie_name, secure, same_site)`. `session_policy_default()` is idle 30 minutes, absolute 12 hours, cookie `__Host-session`, `HttpOnly`, `Secure`, `SameSite=Lax`, `Path=/`, no `Domain`. For local development over plain HTTP use a name without the `__Host-` prefix and `secure` false. `SameSite=None` without `Secure` is `E_COOKIE_SAMESITE_NONE_NEEDS_SECURE`; a `__Host-` name without `Secure`, or a limit below 1, is `E_SESSION_POLICY`. `session_middleware` and `csrf_middleware` check the policy when the app is built and return `Err`.
- `session_middleware(store, policy) -> Result[Middleware, HttpFailure]` finds or creates the session before the handler runs and puts the valid ID first in the request's `Cookie` header (any pair of that name the client sent is removed), and adds the `Set-Cookie` header after, unless the handler already set that cookie. Put it outside `csrf_middleware` and outside any layer that reads the session.
- `session_get(store, policy, request, key) -> Result[Option[Text], HttpFailure]`, `session_set(store, policy, request, key, value)` (keys 1 to 128 characters, values up to 65,536; `E_SESSION_NONE` without a live session, `E_SESSION_KEY`, `E_SESSION_VALUE`), `session_remove`, `session_exists`, `session_id_of(policy, request) -> Option[Text]`.
- **Rotation.** `session_rotate(store, policy, request, response)` moves the data to a new ID, deletes the old one in the same transaction and puts the new cookie on the response. Call it on every privilege change (login, role change, password change). The new session gets a new absolute lifetime. The request's old ID is dead afterwards, so do not use the session through that request again.
- **Logout.** `session_destroy(store, policy, request, response)` deletes the session and sends the cookie with `Max-Age=0`.
- **Fixation.** An ID the server did not issue is never adopted. A cookie that is missing, of the wrong length or alphabet, unknown to the store or expired is answered with a new ID. An ID of the wrong shape is rejected before the store is touched.
- **Expiry.** A session ends after `idle_seconds` without a request and `absolute_seconds` after it was made, whichever comes first. The visit time is renewed at most once per tenth of the idle window, so most reads write nothing. Expired rows are deleted when their ID is next presented and by `session_purge_expired(store, policy) -> Result[Int, HttpFailure]`, which returns how many sessions it removed; run it at start-up or hourly. `session_open_at`, `session_get_at` and `session_purge_expired_at` take the time in milliseconds for tests.
- Reads use `sqlite_begin_read` and writes `sqlite_begin`, so a held write transaction does not stall a read.
- Effects: the functions that read the time report `clock.read`. The effect analysis does not model SQLite, so opening the session database reports no file effect in `guion doc --json`; the package declares `files` in its capabilities, and a project that uses sessions must allow the database's directory.
- Failures are `HttpFailure` values with fixed messages (`E_SESSION_STORE`, `E_SESSION_ENTROPY`): no ID, token or value ever appears in a failure, and the package writes nothing to a log. When the session layer's `before` cannot reach the store it answers 500 with the fixed `Internal Server Error` body and nothing is logged, because a `before` can only return a `Response`; run the store on a path the process can write.
- Looking a session up compares `sha256(ID)` inside SQLite; an attacker who can submit IDs cannot choose a hash, so the lookup does not leak the ID through timing.

## CSRF

`csrf_middleware(store, policy) -> Result[Middleware, HttpFailure]` refuses with 403 and the fixed body `Forbidden` every request whose method is not exactly `GET`, `HEAD` or `OPTIONS` (so `POST`, `PUT`, `PATCH`, `DELETE`, `TRACE`, `CONNECT`, any other method and a lower-case method all need the token) unless it presents the token of its own session. Put it inside `session_middleware`: without a live session behind the cookie no token verifies.

- `csrf_token(store, policy, request) -> Result[Text, HttpFailure]` is `base64url(HMAC-SHA256(server secret, session ID + "csrf"))`, 44 characters. A handler that renders a form calls it and puts the value in a hidden `_csrf` field or a header. `E_CSRF_NO_SESSION` when the request has no live session.
- The token is read from the `x-csrf-token` header or, when that header is absent and the body is `application/x-www-form-urlencoded`, from the `_csrf` field. A header that is present is the only thing read. A token of the wrong length or alphabet is refused before any comparison; a right-length token is checked with `crypto_verify_hmac_sha256`, which compares in constant time.
- A token belongs to one session: a token from session A fails in session B, and the token dies with its session (after `session_rotate`, `session_destroy` or expiry), so a page rendered before login must be rendered again after it.
- `csrf_exempt_method(method)`, `csrf_origin_ok(request)` and `csrf_verify(store, policy, request) -> Result[Bool, HttpFailure]` are the parts, for a handler that wants to check itself.
- Before the token, a request with an `Origin` header (or, with none, a `Referer`) must name its own `Host`: the authority of the URL (host and port, compared without regard to case) equals the `Host` header, else 403. `Origin: null`, a value that is not an http or https URL, a user name in front of the host, and an Origin with no `Host` header are refused; with neither header the token alone decides. The scheme is not compared, because behind a proxy that ends TLS the server does not see it, and the proxy must pass `Host` through. `csrf_origin_ok(request) -> Bool` is this check. It does not defend against a script running in your own page.

## Templates

`html.gu` is the runtime side of checked templates (`NAME.gut` files; see `docs/templates.md` in the repository for the syntax). A project that depends on `web` writes `src/templates/page.gut`, and `guion check`, `build`, `run` and `test` compile it to `page.gut.gu` with `render_page(params) -> Rendered`.

- `Rendered` holds markup built from escaped parts. Read it with `.html` or turn it into a response with `html_response(status, rendered) -> Result[Response, HttpFailure]` (`text/html; charset=utf-8`).
- `html_escape_text`, `html_escape_attr`, `html_escape_url` and `html_script_string` are the escapers generated code calls (element text and quoted attributes; quoted attributes plus the backtick; one URL component; a JSON string literal safe inside `<script>`). All are pure.
- `html_finish` and `html_embed_rendered` are for generated code. In a project that has templates, a hand-written module that names `Rendered(`, `html_finish` or `html_embed_rendered` is refused by `guion check`.

## Failures

`app_handle(app, request)` returns the `Result`. `app_respond(app, request)` always returns a `Response`: an `Err` from a handler or middleware becomes a 500 with the fixed body `Internal Server Error` (`internal_error_body()`), whatever the failure's status, code or message, and the failure is logged through std:log when `app_log(app, logger)` set a logger, to standard error otherwise. A handler that wants a different status returns `Ok(response)` with that status.

A runtime fault inside a handler (an index out of range, say) is not recoverable per request. `parallel_map` stops every worker and `serve` returns an `Err` with the fault's code; connections in flight get no answer. Handlers must return `Err`, not fault.

## Serving

`serve(host, port, app, workers)` listens until the listener fails or the program is told to stop (below); `serve_for(host, port, app, workers, idle_ms)` lets each worker stop after `idle_ms` milliseconds with no connection and returns `Ok`. Both need `--allow-bind HOST:PORT` and follow the pattern of [docs/concurrency.md](../../docs/concurrency.md): `workers` workers share one listener, each accepts one connection, reads one request, runs the app and answers, so a stalled client holds one worker. The worker count is a ceiling for waiting, not a multiplier for computing.

**Clean stop.** Call `process_signal_poll()` once at the start of `main` (`import "std:process"`) and `INT`, `TERM` and `HUP` become the program's own. Workers accept with a timeout of at most 200 ms and check `process_stop_requested()` between connections: a request being handled is finished and answered, then `serve` and `serve_for` return `Ok` and the code after them runs (close a database, print a line, return 0). A program that does not poll for signals is unchanged: the signal ends `guion` at once. `serve_for`'s `idle_ms` counts time without a connection and starts over after each one.

Every response is sent with `Connection: close` (no keep-alive). Reading a request is limited to 10 seconds and 1 MiB. A malformed request is answered by `std:http_server` before any handler runs. If the client disconnects before the answer is written, the send fails, the failure is logged and the worker takes the next connection. No public function names a connection or listener.

## Limits

- No closures: a middleware is a record of named functions plus a `config` map.
- A runtime fault in a handler stops the server; only `Err` is contained.
- The effect analysis does not model SQLite, so `guion doc` shows no file effect for opening the session database.
- CSRF is the token plus the `Origin`/`Referer` check against `Host`; the scheme is not compared.
- Templates take `Text` and records only; escaping cannot be turned off.
- No keep-alive, TLS, HTTP/2 or websockets.
