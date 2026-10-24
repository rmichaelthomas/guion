use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use veritas_compiler::compile;
use veritas_model::{BigInt, Value};
use veritas_runtime::{CancellationToken, EffectKind, EffectOutcome, HostPolicy, Runtime};

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
    let artifact = compile("http-denied.vr", source)
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
    let artifact = compile("http-success.vr", &source).unwrap();
    let artifact = veritas_model::Artifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
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
fn http_cancellation_waits_for_the_blocking_request_and_keeps_its_completed_effect() {
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
        let artifact = compile("http-cancel-boundary.vr", &source).unwrap();
        let policy =
            HostPolicy::default().with_network_hosts(["127.0.0.1"], Duration::from_secs(2), 64);
        let cancellation = CancellationToken::new();
        let task_cancellation = cancellation.clone();
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
        assert!(cancellation_wait >= Duration::from_millis(150));
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].kind, EffectKind::HttpRequest);
        assert_eq!(effects[0].outcome, EffectOutcome::Returned);
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
        let artifact = compile("http-errors.vr", &source).unwrap();
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
    let artifact = compile("http-redirect-limit.vr", &source).unwrap();
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
    let artifact = compile("http-request.vr", source).unwrap();
    let artifact = veritas_model::Artifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let policy =
        HostPolicy::default().with_network_hosts(["127.0.0.1"], Duration::from_secs(2), 1024);
    let mut instance = Runtime::new().load(artifact, policy).unwrap();

    let response = instance
        .invoke(
            "send",
            vec![Value::Text("session-secret".into()), Value::Text(url)],
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
    let artifact = compile("http-request-limit.vr", source).unwrap();
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
    let artifact = compile("http-request-timeout.vr", source).unwrap();
    let artifact = veritas_model::Artifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let policy =
        HostPolicy::default().with_network_hosts(["127.0.0.1"], Duration::from_secs(1), 1024);
    let mut instance = Runtime::new().load(artifact, policy).unwrap();

    let started_at = Instant::now();
    let response = instance.invoke("send", vec![Value::Text(url)]).unwrap();
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
    let artifact = compile("http-request-timeout-cap.vr", source).unwrap();
    let policy =
        HostPolicy::default().with_network_hosts(["127.0.0.1"], Duration::from_millis(40), 1024);
    let mut instance = Runtime::new().load(artifact, policy).unwrap();

    let started_at = Instant::now();
    let response = instance.invoke("send", vec![Value::Text(url)]).unwrap();
    assert!(started.recv_timeout(Duration::from_secs(1)).is_ok());
    assert!(matches!(response, Value::ResultErr(_)));
    assert!(format!("{response:?}").contains("R_HTTP_TIMEOUT"));
    assert!(started_at.elapsed() < Duration::from_millis(150));
    server.join().unwrap();
}
