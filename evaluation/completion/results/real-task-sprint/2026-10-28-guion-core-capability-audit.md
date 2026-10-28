# Guion core capability and resource-boundary audit

Date: 2026-10-28
Revision under review: `codex/guion-core-sprint-spec`, based on `2058967`
Exercised target: Debian 12 ARM64 in the sprint's pinned container

## Outcome

The reviewed capability boundaries match the runtime and CLI behavior covered by the deterministic tests below. This audit found no enforcement defect requiring a code change. Inspection remains a static report; a runtime URL cannot be proven safe from source declarations alone.

## Evidence

| Boundary | Evidence exercised | Result |
| --- | --- | --- |
| Network defaults to deny | `http_is_denied_by_default_with_a_typed_result` | Denied requests return the typed network error without a grant. |
| Exact host grants and redirects | `project_network_manifest_grants_only_the_listed_host`, `run_accepts_an_explicit_one_invocation_http_host_grant`, and `http_get_returns_typed_status_and_body_limit_errors` | Project and one-run grants are explicit; a redirect to an ungranted host is rejected. |
| Dynamic URL inspection | `inspect_json_explains_exact_http_grants_for_runtime_urls`, `inspect_json_compares_reachable_effects_with_manifest_grants` | Inspection reports the required host grant and marks a manifest grant `present_but_target_dynamic`; caller CLI grants are not assumed. A marker-file assertion confirms inspection does not execute the program. |
| File roots and traversal | `file_host_calls_need_a_root_and_cannot_escape_it`, `named_file_root_aliases_cannot_fall_through_to_another_grant_or_escape`, `cli_file_capabilities_are_limited_to_the_source_directory` | File access requires a root and rejects escape paths and aliases that would cross grant boundaries. |
| Environment grants | `environment_access_requires_a_grant_and_preserves_unicode_values` | Names require exact grants; granted values preserve Unicode. |
| Cancellation | `http_cancellation_interrupts_in_flight_requests_and_marks_the_effect_interrupted`, `byte_http_cancellation_interrupts_during_response_body_read`, `host_cancellation_joins_parallel_native_workers_and_returns_cancelled` | In-flight HTTP and worker operations observe cancellation; completed external effects are not rolled back. |
| Time, body, text, and allocation ceilings | `per_request_http_timeout_is_enforced_below_the_manifest_ceiling`, `per_request_http_timeout_cannot_exceed_the_manifest_ceiling`, `byte_http_rejects_oversized_response_before_returning_partial_bytes`, `http_response_text_observes_configured_text_limit`, and allocation-limit tests in `execution.rs` | Requests respect deadlines and response caps; response text observes `max_text_bytes`; guest allocations use the shared budget. |

## Reproduction

Run from the repository root with the locked dependencies available offline:

```sh
cargo test --locked --offline -p guion-runtime --test http
cargo test --locked --offline -p guion-runtime --test execution
cargo test --locked --offline -p guion-cli --test cli
```

On the exercised target, the runtime HTTP file passed 20/20 tests and the runtime execution file passed 119/119. The full CLI file passed 51/52 when run as root; its sole failure was the read-only package-cache fixture, because root can write through the mode bits that the fixture uses to simulate a read-only directory. Running that same test binary as UID 65534 passed the case (1/1). The direct inspection filter passed 3/3.

All HTTP host-boundary tests use local loopback fixtures. The inspection tests use `example.invalid` as source text only and make no request.

## Limits of the claim

This evidence covers the named runtime and CLI cases on Debian 12 ARM64. It does not establish operating-system process isolation or prevent DNS rebinding; network egress confinement requires controls outside Guion's runtime. Static inspection reports reachable effects and unknown dynamic targets, not the future value of arbitrary runtime inputs.
