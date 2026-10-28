# Typed HTTP response text and JSON

Date: 2026-10-28
Branch: `codex/guion-core-sprint-spec`
Artifact format: 45

## API and compatibility

`http_request_bytes(method, url, headers, body, timeout_ms)` remains the byte-preserving request API and retains its existing result type. The new `http_response_text(response: HttpResponse) -> Result[Text, HttpError]` checks `max_text_bytes`, accounts for the resulting text against the guest allocation budget, and returns `R_HTTP_ENCODING` for invalid UTF-8. It leaves `HttpResponse.status` as data, including for non-2xx responses.

The standard-library helper `std:http.http_response_json(response) -> Result[Json, StdError]` composes checked text decoding with `json_parse`. It preserves the existing HTTP or JSON error code and message in `StdError`. The new append-only opcode is tag 165; new artifacts use format 45. The model test round-trips format 45, confirms ordinary format-44 artifacts still round-trip, and rejects tag 165 when an artifact declares format 44.

## Verification

Focused checks run in the pinned Debian 12 ARM64 container:

```sh
cargo test --locked --offline -p guion-model http_response_text_requires_format_45_and_round_trips
cargo test --locked --offline -p guion-compiler --test pipeline http_response_text_is_typechecked_for_http_response_only
cargo test --locked --offline -p guion-runtime --test http http_response_
```

Results: model 1/1, compiler 1/1, and response runtime cases 6/6. The full runtime HTTP file passed 20/20; it includes the named host denial, timeout ceiling, cross-host redirect denial, response body ceiling, cancellation, and loopback response tests. The non-2xx loopback case returned `Ok(HttpResponse)` with status 418 and decoded body `hello`.

The agent example runner passed all 26 checks. It checked, linted, and formatted `docs/agents/examples/http_request.gu` without contacting an external host. The artifact-format audit, builtin catalog check, conformance inventory check, and generated agent-reference check also passed.

## Reproduction commands for error and host-boundary behavior

Run these from the repository root with locked dependencies already available:

```sh
cargo test --locked --offline -p guion-runtime --test http http_is_denied_by_default_with_a_typed_result
cargo test --locked --offline -p guion-runtime --test http per_request_http_timeout_is_enforced_below_the_manifest_ceiling
cargo test --locked --offline -p guion-runtime --test http byte_http_rejects_oversized_response_before_returning_partial_bytes
cargo test --locked --offline -p guion-runtime --test http http_response_json_returns_recoverable_error_for_malformed_json
cargo test --locked --offline -p guion-runtime --test http http_response_text_decodes_utf8_and_preserves_non_2xx_response
```

These tests use only loopback servers and synthetic response records. Malformed JSON returns the same `R_JSON_PARSE` code and message as direct `json_parse`; invalid UTF-8 returns `R_HTTP_ENCODING`; exceeding the text ceiling returns `R_HTTP_LIMIT`.
