# web

Routing, middleware and serving for Guion applications, built on `std:http_response`, `std:http_server` and `std:log`.

Use it from a project with `guion add web --path PATH/TO/web` and `import "pkg:web/src/lib.gu"`. Check the package with `guion check tests.gu` and test it with `guion test tests.gu`. The package is not published to the registry.

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

Function values capture nothing, so a middleware cannot wrap a handler in a closure. A middleware is a record of two named functions, and an `App` holds the router and the layers:

```
fn require_key(request: Request) -> Result[Request, Response]:
    match request_header(request, "x-key"):
        Some(key):
            return Ok(request)
        None:
            return Err(response_text(401, "key required"))

fn stamp(request: Request, response: Response) -> Result[Response, HttpFailure]:
    return response_set_header(response, "X-App", "demo")

fn build(table: Router) -> App:
    return wrap(wrap(app(table), middleware_after(stamp)), middleware_before(require_key))
```

- `before(request) -> Result[Request, Response]`: `Ok(request)` passes the request inward, `Err(response)` answers now.
- `after(request, response) -> Result[Response, HttpFailure]`: sees the response on the way out.
- `middleware(before, after)`, `middleware_before(before)` and `middleware_after(after)` build a layer; `wrap(app, layer)` makes it the outermost; `app_with(router, layers)` lists layers outermost first.
- Order is deterministic: the outermost layer's `before` runs first and its `after` last. When a `before` answers with `Err(response)`, that layer's `after` does not run, and the `after` of every outer layer does, innermost first. An `Err(HttpFailure)` skips the remaining `after` functions.
- Middleware wraps the router's own 404, 405 and 400 responses too.
- Effects attach where a function value is formed, so the function that writes `middleware_before(audit)` reports the effects of `audit`.

## Failures

`app_handle(app, request)` returns the `Result`. `app_respond(app, request)` always returns a `Response`: an `Err` from a handler or middleware becomes a 500 with the fixed body `Internal Server Error` (`internal_error_body()`), whatever the failure's status, code or message, and the failure is logged through std:log when `app_log(app, logger)` set a logger, to standard error otherwise. A handler that wants a different status returns `Ok(response)` with that status.

A runtime fault inside a handler (an index out of range, say) is not recoverable per request. `parallel_map` stops every worker and `serve` returns an `Err` with the fault's code; connections in flight get no answer. Handlers must return `Err`, not fault.

## Serving

`serve(host, port, app, workers)` listens until the listener fails; `serve_for(host, port, app, workers, idle_ms)` lets each worker stop after `idle_ms` milliseconds with no connection and returns `Ok`. Both need `--allow-bind HOST:PORT` and follow the pattern of [docs/concurrency.md](../../docs/concurrency.md): `workers` workers share one listener, each accepts one connection, reads one request, runs the app and answers, so a stalled client holds one worker. The worker count is a ceiling for waiting, not a multiplier for computing.

Every response is sent with `Connection: close` (no keep-alive). Reading a request is limited to 10 seconds and 1 MiB. A malformed request is answered by `std:http_server` before any handler runs. If the client disconnects before the answer is written, the send fails, the failure is logged and the worker takes the next connection. No public function names a connection or listener.
