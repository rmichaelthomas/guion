use guion_compiler::{compile, compile_project_with_roots};
use guion_model::{BigInt, ShareAcrossThreads, SharedJson, Value};
use guion_runtime::{CancellationToken, EffectKind, EffectOutcome, HostPolicy, Runtime};
use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

fn serve_once(response: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 1024];
        let _ = stream.read(&mut request);
        stream.write_all(response.as_bytes()).unwrap();
    });
    format!("http://{address}/feed")
}

fn serve_sequence(responses: Vec<&'static str>) -> (String, thread::JoinHandle<usize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let mut count = 0;
        for response in responses {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            let mut request = [0; 1024];
            let _ = stream.read(&mut request);
            stream.write_all(response.as_bytes()).unwrap();
            count += 1;
        }
        count
    });
    (format!("http://{address}/feed"), server)
}

fn serve_request(response: &'static str) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        loop {
            let read = stream.read(&mut buffer).unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
            if let Some(body_start) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..body_start]).to_ascii_lowercase();
                let body_length = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length: "))
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(0);
                if request.len() >= body_start + 4 + body_length {
                    break;
                }
            }
        }
        stream.write_all(response.as_bytes()).unwrap();
        String::from_utf8(request).unwrap()
    });
    (format!("http://{address}/sync"), server)
}

fn serve_delayed(response: &'static str) -> (String, mpsc::Receiver<()>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 1024];
        let _ = stream.read(&mut request);
        started_tx.send(()).unwrap();
        thread::sleep(Duration::from_millis(200));
        let _ = stream.write_all(response.as_bytes());
    });
    (format!("http://{address}/slow"), started_rx, server)
}

#[test]
fn http_is_denied_by_default_with_a_typed_result() {
    let source = "fn response() -> Result[Text, HttpError]:\n    return http_get(\"http://127.0.0.1:1/not-contacted\")\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile("http-denied.gu", source)
        .expect("http_get and HttpError are part of the public language surface");
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();

    let response = instance.invoke("response", vec![]).unwrap();
    let Value::ResultErr(error) = response else {
        panic!("expected typed network error");
    };
    let Value::HttpError(error) = *error else {
        panic!("expected HttpError payload");
    };
    assert_eq!(error.code, "R_NETWORK_DENIED");
    assert_eq!(error.trace.first().map(|span| span.line), Some(2));
    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::zero())
    );
}

#[test]
fn allowed_http_get_runs_through_a_serialized_artifact() {
    let url = serve_once("HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello");
    let source = format!(
        "fn response() -> Result[Text, HttpError]:\n    return http_get({url:?})\n\nfn main() -> Int:\n    return 0\n"
    );
    let artifact = compile("http-success.gu", &source).unwrap();
    let artifact = guion_model::Artifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let policy =
        HostPolicy::default().with_network_hosts(["127.0.0.1"], Duration::from_secs(2), 64);
    let mut instance = Runtime::new().load(artifact, policy).unwrap();

    assert_eq!(
        instance.invoke("response", vec![]).unwrap(),
        Value::ResultOk(Box::new(Value::Text("hello".into())))
    );
    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::zero())
    );
}

#[test]
fn http_cancellation_interrupts_in_flight_requests_and_marks_the_effect_interrupted() {
    for general_request in [false, true] {
        let (url, request_started, server) =
            serve_delayed("HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello");
        let source = if general_request {
            format!(
                "fn response() -> Result[Json, HttpError]:\n    let headers: Map[Text, Text] = {{}}\n    return http_request(\"GET\", {url:?}, headers, \"\")\n\nfn main() -> Int:\n    let result = response()\n    return 0\n"
            )
        } else {
            format!(
                "fn response() -> Result[Text, HttpError]:\n    return http_get({url:?})\n\nfn main() -> Int:\n    let result = response()\n    return 0\n"
            )
        };
        let artifact = compile("http-cancel-boundary.gu", &source).unwrap();
        let policy =
            HostPolicy::default().with_network_hosts(["127.0.0.1"], Duration::from_secs(2), 64);
        let cancellation = CancellationToken::new();
        let task_cancellation = cancellation.clone();
        // The artifact is loaded on another thread, which reads its constants.
        artifact.share_across_threads();
        let invocation = thread::spawn(move || {
            let mut instance = Runtime::new().load(artifact, policy).unwrap();
            let result = instance.invoke_with_cancellation("main", vec![], task_cancellation);
            (result, instance.effect_report().to_vec())
        });

        request_started
            .recv_timeout(Duration::from_secs(2))
            .expect("the HTTP request should be in flight before cancellation");
        let cancelled_at = Instant::now();
        cancellation.cancel();
        let (result, effects) = invocation.join().unwrap();
        let cancellation_wait = cancelled_at.elapsed();
        server.join().unwrap();

        assert_eq!(result.unwrap_err().code, "R_CANCELLED");
        assert!(cancellation_wait < Duration::from_millis(150));
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].kind, EffectKind::HttpRequest);
        assert_eq!(effects[0].outcome, EffectOutcome::Interrupted);
    }
}

#[test]
fn http_get_returns_typed_status_and_body_limit_errors() {
    let status_url = serve_once(
        "HTTP/1.1 404 Not Found\r\nContent-Length: 7\r\nConnection: close\r\n\r\nmissing",
    );
    let body_url = serve_once(
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n0\r\n\r\n",
    );
    let redirect_url = serve_once(
        "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.2/redirected\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    for (url, cap, expected_code) in [
        (status_url, 64, "R_HTTP_STATUS"),
        (body_url, 4, "R_HTTP_LIMIT"),
        (redirect_url, 64, "R_HTTP_REDIRECT"),
    ] {
        let source = format!(
            "fn response() -> Result[Text, HttpError]:\n    return http_get({url:?})\n\nfn main() -> Int:\n    return 0\n"
        );
        let artifact = compile("http-errors.gu", &source).unwrap();
        let policy =
            HostPolicy::default().with_network_hosts(["127.0.0.1"], Duration::from_secs(2), cap);
        let mut instance = Runtime::new().load(artifact, policy).unwrap();
        let error = instance.invoke("response", vec![]).unwrap();
        assert!(matches!(error, Value::ResultErr(_)));
        assert!(format!("{error:?}").contains(expected_code));
    }
}

#[test]
fn http_redirect_limit_allows_only_the_configured_number_of_hops() {
    let (url, server) = serve_sequence(vec![
        "HTTP/1.1 302 Found\r\nLocation: /second\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        "HTTP/1.1 302 Found\r\nLocation: /third\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    ]);
    let source = format!(
        "fn response() -> Result[Text, HttpError]:\n    return http_get({url:?})\n\nfn main() -> Int:\n    return 0\n"
    );
    let artifact = compile("http-redirect-limit.gu", &source).unwrap();
    let mut policy =
        HostPolicy::default().with_network_hosts(["127.0.0.1"], Duration::from_secs(2), 64);
    policy.max_http_redirects = 1;
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    let result = instance.invoke("response", vec![]).unwrap();
    assert!(format!("{result:?}").contains("R_HTTP_REDIRECT"));
    assert_eq!(server.join().unwrap(), 2);
}

#[test]
fn general_http_request_sends_method_headers_and_runtime_body_and_returns_status() {
    let (url, server) = serve_request(
        "HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nContent-Length: 13\r\nConnection: close\r\n\r\n{\"next\":null}",
    );
    let source = r#"fn send(token: Text, url: Text) -> Result[Json, HttpError]:
    let headers = {"authorization": "Bearer " + token, "content-type": "application/json"}
    return http_request("POST", url, headers, "{\"page\":2}")

fn main() -> Int:
    return 0
"#;
    let artifact = compile("http-request.gu", source).unwrap();
    let artifact = guion_model::Artifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let policy =
        HostPolicy::default().with_network_hosts(["127.0.0.1"], Duration::from_secs(2), 1024);
    let mut instance = Runtime::new().load(artifact, policy).unwrap();

    let response = instance
        .invoke(
            "send",
            vec![
                Value::Text("session-secret".into()),
                Value::Text(url.into()),
            ],
        )
        .unwrap();
    let Value::ResultOk(response) = response else {
        panic!("expected a successful HTTP response");
    };
    let Value::Json(response) = *response else {
        panic!("expected a JSON response envelope");
    };
    let response = response.to_serde();
    assert_eq!(response["status"], 429);
    assert_eq!(response["body"], "{\"next\":null}");
    assert_eq!(response["headers"]["content-type"], "application/json");
    let request = server.join().unwrap();
    let request_lower = request.to_ascii_lowercase();
    assert!(request.starts_with("POST /sync HTTP/1.1\r\n"));
    assert!(request_lower.contains("authorization: bearer session-secret\r\n"));
    assert!(request.contains("{\"page\":2}"));
}

#[test]
fn general_http_request_rejects_oversized_request_body_before_network_access() {
    let source = "fn send() -> Result[Json, HttpError]:\n    return http_request(\"POST\", \"http://127.0.0.1:1/sync\", {\"content-type\": \"application/json\"}, \"12345\")\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile("http-request-limit.gu", source).unwrap();
    let mut policy =
        HostPolicy::default().with_network_hosts(["127.0.0.1"], Duration::from_secs(2), 1024);
    policy.max_http_request_bytes = 4;
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    let response = instance.invoke("send", vec![]).unwrap();
    assert!(format!("{response:?}").contains("R_HTTP_LIMIT"));
}

#[test]
fn per_request_http_timeout_is_enforced_below_the_manifest_ceiling() {
    let (url, started, server) =
        serve_delayed("HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
    let source = r#"fn send(url: Text) -> Result[Json, HttpError]:
    let headers: Map[Text, Text] = {}
    return http_request_with_timeout("GET", url, headers, "", 30)

fn main() -> Int:
    return 0
"#;
    let artifact = compile("http-request-timeout.gu", source).unwrap();
    let artifact = guion_model::Artifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let policy =
        HostPolicy::default().with_network_hosts(["127.0.0.1"], Duration::from_secs(1), 1024);
    let mut instance = Runtime::new().load(artifact, policy).unwrap();

    let started_at = Instant::now();
    let response = instance
        .invoke("send", vec![Value::Text(url.into())])
        .unwrap();
    assert!(started.recv_timeout(Duration::from_secs(1)).is_ok());
    assert!(matches!(response, Value::ResultErr(_)));
    assert!(format!("{response:?}").contains("R_HTTP_TIMEOUT"));
    assert!(started_at.elapsed() < Duration::from_millis(150));
    server.join().unwrap();
}

#[test]
fn per_request_http_timeout_cannot_exceed_the_manifest_ceiling() {
    let (url, started, server) =
        serve_delayed("HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
    let source = r#"fn send(url: Text) -> Result[Json, HttpError]:
    let headers: Map[Text, Text] = {}
    return http_request_with_timeout("GET", url, headers, "", 500)

fn main() -> Int:
    return 0
"#;
    let artifact = compile("http-request-timeout-cap.gu", source).unwrap();
    let policy =
        HostPolicy::default().with_network_hosts(["127.0.0.1"], Duration::from_millis(40), 1024);
    let mut instance = Runtime::new().load(artifact, policy).unwrap();

    let started_at = Instant::now();
    let response = instance
        .invoke("send", vec![Value::Text(url.into())])
        .unwrap();
    assert!(started.recv_timeout(Duration::from_secs(1)).is_ok());
    assert!(matches!(response, Value::ResultErr(_)));
    assert!(format!("{response:?}").contains("R_HTTP_TIMEOUT"));
    assert!(started_at.elapsed() < Duration::from_millis(150));
    server.join().unwrap();
}

#[test]
fn byte_http_preserves_binary_request_and_response_bodies() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        loop {
            let read = stream.read(&mut buffer).unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
            if let Some(body_start) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                let header_end = body_start + 4;
                let headers = String::from_utf8_lossy(&request[..body_start]).to_ascii_lowercase();
                let content_length = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length: "))
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(0);
                if request.len() >= header_end + content_length {
                    break;
                }
            }
        }
        let response_body = [0x00, 0xff, 0x80, b'v'];
        let mut response = b"HTTP/1.1 201 Created\r\nContent-Type: application/octet-stream\r\nContent-Length: 4\r\nConnection: close\r\n\r\n".to_vec();
        response.extend_from_slice(&response_body);
        stream.write_all(&response).unwrap();
        request
    });
    let url = format!("http://{address}/webhook");
    let source = "record HttpResponse:\n    status: Int\n    headers: Map[Text, Text]\n    body: Bytes\n\nfn send(url: Text, payload: Bytes) -> Result[HttpResponse, HttpError]:\n    let headers: Map[Text, Text] = {\"content-type\": \"application/octet-stream\"}\n    return http_request_bytes(\"POST\", url, headers, payload, 1000)\n\nfn main() -> Int:\n    return 0\n".to_string();
    let artifact = compile("http-bytes.gu", &source).expect("byte HTTP is a typed builtin");
    let policy =
        HostPolicy::default().with_network_hosts(["127.0.0.1"], Duration::from_secs(2), 64);
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    let payload = vec![0x00, 0xff, 0x80, b'h', b'o', b'o', b'k'];
    let result = instance
        .invoke(
            "send",
            vec![
                Value::Text(url.into()),
                Value::Bytes(guion_model::Shared::new(payload.clone())),
            ],
        )
        .unwrap();
    let Value::ResultOk(response) = result else {
        panic!("expected successful byte response: {result:?}");
    };
    let Value::Record(name, fields) = *response else {
        panic!("expected HttpResponse record");
    };
    assert_eq!(name, "HttpResponse");
    assert_eq!(fields[0], Value::Int(BigInt::from_i64(201)));
    assert!(
        matches!(&fields[1], Value::Map(headers) if headers.get("content-type") == Some(&Value::Text("application/octet-stream".into())))
    );
    assert_eq!(
        fields[2],
        Value::Bytes(guion_model::Shared::new(vec![0x00, 0xff, 0x80, b'v']))
    );
    let request = server.join().unwrap();
    assert!(request
        .windows(payload.len())
        .any(|window| window == payload));
}

#[test]
fn byte_http_cancellation_interrupts_during_response_body_read() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (body_started_tx, body_started_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = [0; 1024];
        let _ = stream.read(&mut request);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: keep-alive\r\n\r\nxy")
            .unwrap();
        stream.flush().unwrap();
        body_started_tx.send(()).unwrap();
        let mut byte = [0; 1];
        stream.read(&mut byte).unwrap()
    });
    let url = format!("http://{address}/trickle");
    let source = format!(
        "record HttpResponse:\n    status: Int\n    headers: Map[Text, Text]\n    body: Bytes\n\nfn response(url: Text) -> Result[HttpResponse, HttpError]:\n    return http_request_bytes(\"GET\", url, {{}}, bytes_from_text(\"\"), 5000)\n\nfn main() -> Int:\n    let result = response({url:?})\n    return 0\n"
    );
    let artifact = compile("http-body-cancel.gu", &source).unwrap();
    let policy =
        HostPolicy::default().with_network_hosts(["127.0.0.1"], Duration::from_secs(5), 1024);
    let cancellation = CancellationToken::new();
    let task_cancellation = cancellation.clone();
    // The artifact is loaded on another thread, which reads its constants.
    artifact.share_across_threads();
    let invocation = thread::spawn(move || {
        let mut instance = Runtime::new().load(artifact, policy).unwrap();
        let result = instance.invoke_with_cancellation("main", vec![], task_cancellation);
        (result, instance.effect_report().to_vec())
    });
    body_started_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    let cancelled_at = Instant::now();
    cancellation.cancel();
    let (result, effects) = invocation.join().unwrap();
    assert_eq!(result.unwrap_err().code, "R_CANCELLED");
    assert!(cancelled_at.elapsed() < Duration::from_millis(150));
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].outcome, EffectOutcome::Interrupted);
    assert_eq!(
        server.join().unwrap(),
        0,
        "cancelled response socket closes promptly"
    );
}

#[test]
fn parallel_worker_failure_cancels_in_flight_http_sibling_and_closes_socket() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = thread::spawn(move || {
        let accept_deadline = Instant::now() + Duration::from_secs(3);
        let mut connections = Vec::new();
        while connections.len() < 2 {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut request = Vec::new();
                    let mut buffer = [0; 512];
                    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                        let count = stream.read(&mut buffer).unwrap();
                        assert_ne!(count, 0, "HTTP request ended before its headers");
                        request.extend_from_slice(&buffer[..count]);
                    }
                    let request_line = String::from_utf8_lossy(&request);
                    let path = request_line
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .expect("HTTP request has a path")
                        .to_string();
                    connections.push((path, stream));
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < accept_deadline,
                        "both HTTP workers must start"
                    );
                    thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("accepting HTTP worker request: {error}"),
            }
        }

        let (slow_started_tx, slow_started_rx) = mpsc::channel();
        let mut slow_started_rx = Some(slow_started_rx);
        let mut handlers = Vec::new();
        for (path, mut stream) in connections {
            if path == "/slow" {
                let slow_started_tx = slow_started_tx.clone();
                handlers.push(thread::spawn(move || {
                    stream
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: keep-alive\r\n\r\nxy",
                        )
                        .unwrap();
                    stream.flush().unwrap();
                    slow_started_tx.send(()).unwrap();
                    let mut byte = [0; 1];
                    let outcome = match stream.read(&mut byte) {
                        Ok(0) => "eof".to_string(),
                        Ok(count) => format!("still open: read {count} bytes"),
                        Err(error) => format!("read error: {:?}", error.kind()),
                    };
                    (path, outcome)
                }));
            } else if path == "/fail" {
                let slow_started_rx = slow_started_rx.take().unwrap();
                handlers.push(thread::spawn(move || {
                    let slow_is_in_flight =
                        slow_started_rx.recv_timeout(Duration::from_secs(2)).is_ok();
                    let response_sent = slow_is_in_flight
                        && stream
                            .write_all(
                                b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                            )
                            .is_ok();
                    let _ = stream.flush();
                    (path, format!("failure response sent: {response_sent}"))
                }));
            } else {
                handlers.push(thread::spawn(move || {
                    let _ = stream.write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                    (path, "unexpected path".to_string())
                }));
            }
        }
        handlers
            .into_iter()
            .map(|handler| handler.join().unwrap())
            .collect::<Vec<_>>()
    });

    let slow_url = format!("http://{address}/slow");
    let failing_url = format!("http://{address}/fail");
    let source = format!(
        "fn fetch(url: Text) -> Int:\n    let headers: Map[Text, Text] = {{}}\n    let response = http_request_with_timeout(\"GET\", url, headers, \"\", 3000)\n    if not is_ok(response):\n        return 0\n    let envelope = unwrap(response)\n    assert(json_as_int(json_get(envelope, \"status\")) == 200)\n    return 1\n\nfn fetch_all() -> Result[List[Int], TaskError]:\n    return parallel_map([{slow_url:?}, {failing_url:?}], fetch, 2)\n\nfn main() -> Int:\n    return 0\n"
    );
    let artifact = compile("parallel-http-failure.gu", &source).unwrap();
    let policy =
        HostPolicy::default().with_network_hosts(["127.0.0.1"], Duration::from_secs(3), 1024);
    let mut instance = Runtime::new().load(artifact, policy).unwrap();

    let Value::ResultErr(error) = instance.invoke("fetch_all", vec![]).unwrap() else {
        panic!("the failing HTTP worker must return a TaskError");
    };
    let Value::TaskError(error) = *error else {
        panic!("parallel_map Err contains a TaskError");
    };
    assert_eq!(error.index, BigInt::from_usize(1));
    assert_eq!(error.code, "R_ASSERTION");
    assert!(instance.parallel_threads_used() > 1);

    let outcomes = server.join().unwrap();
    assert!(
        outcomes.contains(&("/slow".to_string(), "eof".to_string())),
        "server outcomes: {outcomes:?}"
    );
    assert!(
        outcomes.contains(&(
            "/fail".to_string(),
            "failure response sent: true".to_string()
        )),
        "server outcomes: {outcomes:?}"
    );
}

#[test]
fn byte_http_total_deadline_applies_across_trickled_body_chunks() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 1024];
        let _ = stream.read(&mut request);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\na")
            .unwrap();
        stream.flush().unwrap();
        for _ in 1..8 {
            thread::sleep(Duration::from_millis(35));
            if stream.write_all(b"b").is_err() {
                break;
            }
            let _ = stream.flush();
        }
    });
    let url = format!("http://{address}/trickle");
    let source = "record HttpResponse:\n    status: Int\n    headers: Map[Text, Text]\n    body: Bytes\n\nfn response(url: Text) -> Result[HttpResponse, HttpError]:\n    return http_request_bytes(\"GET\", url, {}, bytes_from_text(\"\"), 120)\n\nfn main() -> Int:\n    return 0\n".to_string();
    let artifact = compile("http-total-deadline.gu", &source).unwrap();
    let policy =
        HostPolicy::default().with_network_hosts(["127.0.0.1"], Duration::from_secs(1), 1024);
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    let started = Instant::now();
    let result = instance
        .invoke("response", vec![Value::Text(url.into())])
        .unwrap();
    assert!(
        format!("{result:?}").contains("R_HTTP_TIMEOUT"),
        "{result:?}"
    );
    assert!(started.elapsed() < Duration::from_millis(250));
    server.join().unwrap();
}

#[test]
fn byte_http_rejects_oversized_response_before_returning_partial_bytes() {
    let url = serve_once("HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\n12345");
    let source = "record HttpResponse:\n    status: Int\n    headers: Map[Text, Text]\n    body: Bytes\n\nfn response(url: Text) -> Result[HttpResponse, HttpError]:\n    return http_request_bytes(\"GET\", url, {}, bytes_from_text(\"\"), 1000)\n\nfn main() -> Int:\n    return 0\n".to_string();
    let artifact = compile("http-byte-response-limit.gu", &source).unwrap();
    let policy = HostPolicy::default().with_network_hosts(["127.0.0.1"], Duration::from_secs(2), 4);
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    let result = instance
        .invoke("response", vec![Value::Text(url.into())])
        .unwrap();
    assert!(format!("{result:?}").contains("R_HTTP_LIMIT"));
}

fn compile_http_project(name: &str, source: &str) -> guion_model::Artifact {
    let root =
        std::env::temp_dir().join(format!("guion-http-project-{name}-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let entry = root.join("main.gu");
    fs::write(&entry, source).unwrap();
    let stdlib = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib");
    let artifact = compile_project_with_roots(&entry, &root, Some(&stdlib), &HashMap::new())
        .unwrap_or_else(|error| panic!("HTTP project compilation failed: {error:?}"));
    let _ = fs::remove_dir_all(root);
    artifact
}

fn http_response_value(status: i64, body: &[u8]) -> Value {
    Value::Record(
        "HttpResponse".into(),
        guion_model::Shared::new(vec![
            Value::Int(BigInt::from_i64(status)),
            Value::Map(guion_model::Shared::new(guion_model::TextMap::new())),
            Value::Bytes(guion_model::Shared::new(body.to_vec())),
        ]),
    )
}

#[test]
fn http_response_text_decodes_utf8_and_preserves_non_2xx_response() {
    let url = serve_once(
        "HTTP/1.1 418 I'm a teapot\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
    );
    let source = "record HttpResponse:\n    status: Int\n    headers: Map[Text, Text]\n    body: Bytes\n\nfn request(url: Text) -> Result[HttpResponse, HttpError]:\n    return http_request_bytes(\"GET\", url, {}, bytes_from_text(\"\"), 1000)\n\nfn decode(response: HttpResponse) -> Result[Text, HttpError]:\n    return http_response_text(response)\n\nfn main() -> Int:\n    return 0\n";
    let artifact =
        compile("http-response-text.gu", source).expect("typed HTTP response text helper compiles");
    let policy =
        HostPolicy::default().with_network_hosts(["127.0.0.1"], Duration::from_secs(2), 64);
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    let response = instance
        .invoke("request", vec![Value::Text(url.into())])
        .unwrap();
    let Value::ResultOk(response) = response else {
        panic!("non-2xx HTTP status remains a response value: {response:?}");
    };
    let Value::Record(name, fields) = response.as_ref() else {
        panic!("expected HttpResponse record");
    };
    assert_eq!(name, "HttpResponse");
    assert_eq!(fields[0], Value::Int(BigInt::from_i64(418)));
    assert_eq!(
        instance.invoke("decode", vec![*response]).unwrap(),
        Value::ResultOk(Box::new(Value::Text("hello".into())))
    );
}

#[test]
fn http_response_text_returns_recoverable_error_for_invalid_utf8() {
    let source = "record HttpResponse:\n    status: Int\n    headers: Map[Text, Text]\n    body: Bytes\n\nfn decode(response: HttpResponse) -> Result[Text, HttpError]:\n    return http_response_text(response)\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile("http-response-invalid-utf8.gu", source)
        .expect("typed HTTP response text helper compiles");
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    let result = instance
        .invoke("decode", vec![http_response_value(200, &[b'a', 0xff])])
        .unwrap();
    let Value::ResultErr(error) = result else {
        panic!("invalid UTF-8 must return a recoverable error: {result:?}");
    };
    let Value::HttpError(error) = *error else {
        panic!("invalid UTF-8 must return HttpError");
    };
    assert_eq!(error.code, "R_HTTP_ENCODING");
    assert_eq!(error.message, "HTTP response body is not valid UTF-8");
}

#[test]
fn http_response_text_observes_configured_text_limit() {
    let source = "record HttpResponse:\n    status: Int\n    headers: Map[Text, Text]\n    body: Bytes\n\nfn decode(response: HttpResponse) -> Result[Text, HttpError]:\n    return http_response_text(response)\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile("http-response-text-limit.gu", source)
        .expect("typed HTTP response text helper compiles");
    let policy = HostPolicy {
        max_text_bytes: 1,
        ..HostPolicy::default()
    };
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    let result = instance
        .invoke("decode", vec![http_response_value(200, b"ok")])
        .unwrap();
    let Value::ResultErr(error) = result else {
        panic!("text-size limit must return a recoverable error: {result:?}");
    };
    let Value::HttpError(error) = *error else {
        panic!("text-size limit must return HttpError");
    };
    assert_eq!(error.code, "R_HTTP_LIMIT");
}

#[test]
fn http_response_json_parses_valid_json_body() {
    let source = "import \"std:types\"\nimport \"std:http\"\n\nfn parse_response(response: HttpResponse) -> Result[Json, StdError]:\n    return http_response_json(response)\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile_http_project("http-response-json", source);
    let mut instance = Runtime::vm_only()
        .with_runtime_profile()
        .load(artifact, HostPolicy::default())
        .unwrap();
    let result = instance
        .invoke(
            "parse_response",
            vec![http_response_value(200, br#"{"ok":true,"count":42}"#)],
        )
        .unwrap();
    assert_eq!(
        result,
        Value::ResultOk(Box::new(Value::Json(SharedJson::from_serde(
            serde_json::json!({"ok": true, "count": 42}),
        ))))
    );
    let profile = instance.runtime_profile().expect("profile is enabled");
    assert_eq!(
        profile["host_operations"]["counts"]["HttpResponseJson"].as_u64(),
        Some(1),
        "typed JSON response should parse directly from the response bytes"
    );
    assert_eq!(
        profile["host_operations"]["counts"]["HttpResponseText"].as_u64(),
        None,
        "the typed JSON path must not construct an intermediate Text value"
    );
}

#[test]
fn http_response_json_maps_http_decode_error_to_std_error() {
    let source = "import \"std:types\"\nimport \"std:http\"\n\nfn parse_response(response: HttpResponse) -> Result[Json, StdError]:\n    return http_response_json(response)\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile_http_project("http-response-invalid-json-encoding", source);
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    let result = instance
        .invoke("parse_response", vec![http_response_value(200, &[0xff])])
        .unwrap();
    let Value::ResultErr(error) = result else {
        panic!("invalid response encoding must be a recoverable error: {result:?}");
    };
    let Value::Record(name, fields) = *error else {
        panic!("JSON response errors are normalized to StdError");
    };
    assert_eq!(name, "StdError");
    assert_eq!(fields[0], Value::Text("R_HTTP_ENCODING".into()));
    assert_eq!(
        fields[1],
        Value::Text("HTTP response body is not valid UTF-8".into())
    );
}

#[test]
fn http_response_json_returns_recoverable_error_for_malformed_json() {
    let source = "import \"std:json\"\nimport \"std:types\"\nimport \"std:http\"\n\nfn parse_response(response: HttpResponse) -> Result[Json, StdError]:\n    return http_response_json(response)\n\nfn expected_parse_code() -> Text:\n    let result = json_parse(\"{\")\n    return json_error_code(error(result))\n\nfn expected_parse_message() -> Text:\n    let result = json_parse(\"{\")\n    return json_error_message(error(result))\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile_http_project("http-response-malformed-json", source);
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    let result = instance
        .invoke("parse_response", vec![http_response_value(200, b"{")])
        .unwrap();
    let Value::ResultErr(error) = result else {
        panic!("malformed JSON must return a recoverable error: {result:?}");
    };
    let Value::Record(name, fields) = *error else {
        panic!("JSON response errors are normalized to StdError");
    };
    assert_eq!(name, "StdError");
    assert_eq!(
        fields[0],
        instance.invoke("expected_parse_code", vec![]).unwrap()
    );
    assert_eq!(
        fields[1],
        instance.invoke("expected_parse_message", vec![]).unwrap()
    );
    assert_eq!(fields[0], Value::Text("R_JSON_PARSE".into()));
}

#[test]
fn http_response_json_preserves_duplicate_key_errors() {
    let source = "import \"std:types\"\nimport \"std:http\"\n\nfn parse_response(response: HttpResponse) -> Result[Json, StdError]:\n    return http_response_json(response)\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile_http_project("http-response-duplicate-json-key", source);
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    let result = instance
        .invoke(
            "parse_response",
            vec![http_response_value(200, br#"{"ok":1,"ok":2}"#)],
        )
        .unwrap();
    let Value::ResultErr(error) = result else {
        panic!("duplicate JSON object keys must remain errors: {result:?}");
    };
    let Value::Record(name, fields) = *error else {
        panic!("JSON response errors are normalized to StdError");
    };
    assert_eq!(name, "StdError");
    assert_eq!(fields[0], Value::Text("R_JSON_DUPLICATE_KEY".into()));
    assert_eq!(
        fields[1],
        Value::Text("JSON object contains duplicate key `ok`".into())
    );
}

#[test]
fn http_response_json_observes_configured_text_limit() {
    let source = "import \"std:types\"\nimport \"std:http\"\n\nfn parse_response(response: HttpResponse) -> Result[Json, StdError]:\n    return http_response_json(response)\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile_http_project("http-response-json-text-limit", source);
    let policy = HostPolicy {
        max_text_bytes: 80,
        ..HostPolicy::default()
    };
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    let oversized_body = vec![b'a'; 100];
    let result = instance
        .invoke(
            "parse_response",
            vec![http_response_value(200, &oversized_body)],
        )
        .unwrap();
    let Value::ResultErr(error) = result else {
        panic!("HTTP JSON body over the text ceiling must return a typed error");
    };
    let Value::Record(name, fields) = *error else {
        panic!("HTTP JSON response errors are normalized to StdError");
    };
    assert_eq!(name, "StdError");
    assert_eq!(fields[0], Value::Text("R_HTTP_LIMIT".into()));
    assert_eq!(
        fields[1],
        Value::Text("HTTP response exceeds the configured text byte limit".into())
    );
}
