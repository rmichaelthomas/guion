use guion_compiler::compile;
use guion_model::{BigInt, Value};
use guion_runtime::{HostPolicy, Runtime};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    process::Command,
    thread,
    time::Duration,
};

fn resident_bytes() -> usize {
    let mut usage = unsafe { std::mem::zeroed::<libc::rusage>() };
    let result = unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    assert_eq!(result, 0, "getrusage failed to sample this test process");
    let maximum = usize::try_from(usage.ru_maxrss).expect("ru_maxrss is non-negative");
    if cfg!(target_os = "macos") {
        maximum
    } else {
        maximum.saturating_mul(1024)
    }
}

fn host_description() -> String {
    Command::new("uname")
        .args(["-srm"])
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_else(|_| format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH))
}

#[test]
fn repeated_invocation_closes_native_resources_and_reaches_a_memory_plateau() {
    let root = std::env::temp_dir().join(format!("guion-lifecycle-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let url = format!("http://{address}/lifecycle");
    let server = thread::spawn(move || {
        for _ in 0..57 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 1024];
            let _ = stream.read(&mut request);
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        }
    });
    let source = format!(
        r#"record HttpResponse:
    status: Int
    headers: Map[Text, Text]
    body: Bytes

fn exercise(url: Text) -> Int:
    let database = unwrap(native_sqlite_open("lifecycle.sqlite"))
    let transaction = unwrap(native_sqlite_begin(database))
    unwrap(native_sqlite_execute(transaction, "CREATE TABLE IF NOT EXISTS lifecycle (value INTEGER)", []))
    unwrap(native_sqlite_rollback(transaction))
    unwrap(native_sqlite_close(database))
    let document = unwrap(native_toml_parse("value = 1\n"))
    let response = unwrap(http_request_bytes("GET", url, {{}}, bytes_from_text(""), 1000))
    return response.status

fn main() -> Int:
    return exercise({url:?})
"#
    );
    let artifact = compile("lifecycle.gu", &source).unwrap();
    let policy = HostPolicy::default()
        .with_file_access(&root, true, true)
        .with_network_hosts(["127.0.0.1"], Duration::from_secs(2), 64);
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    let mut completed = 0usize;
    let mut samples = Vec::new();
    for batch in [1usize, 8, 16, 32] {
        for _ in 0..batch {
            assert_eq!(
                instance.invoke("main", vec![]).unwrap(),
                Value::Int(BigInt::from_i64(200))
            );
            assert_eq!(instance.open_native_resource_count(), 0);
            completed += 1;
        }
        samples.push(serde_json::json!({
            "invocations_completed": completed,
            "resident_bytes": resident_bytes(),
        }));
    }
    server.join().unwrap();
    assert!(root.join("lifecycle.sqlite").is_file());
    let warmed = samples[0]["resident_bytes"].as_u64().unwrap();
    let tail = samples[1..]
        .iter()
        .map(|sample| sample["resident_bytes"].as_u64().unwrap())
        .collect::<Vec<_>>();
    let tail_low = *tail.iter().min().unwrap();
    let tail_high = *tail.iter().max().unwrap();
    let relative_tail_spread = tail_high.saturating_sub(tail_low) as f64 / warmed.max(1) as f64;
    let plateau_observed = relative_tail_spread <= 0.10;
    instance.close();
    assert_eq!(instance.open_native_resource_count(), 0);
    assert_eq!(
        instance.invoke("main", vec![]).unwrap_err().code,
        "R_INSTANCE_CLOSED"
    );

    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "macos-aarch64",
        ("linux", "aarch64") => "debian-aarch64",
        ("linux", "x86_64") => "debian-x86_64",
        (os, arch) => {
            return eprintln!("RSS evidence skipped for unsupported test host {os}-{arch}")
        }
    };
    let report = serde_json::json!({
        "schema_version": 1,
        "kind": "runtime_lifecycle_memory_samples",
        "target": target,
        "host": host_description(),
        "measurement": "resource.getrusage(RUSAGE_SELF).ru_maxrss high-water mark for the test process; macOS values are bytes and other supported hosts are converted from KiB to bytes",
        "invocations": completed,
        "warmup_sample": samples[0],
        "tail_samples": &samples[1..],
        "tail_relative_spread": relative_tail_spread,
        "plateau_rule": "tail spread at or below 10% of warmed RSS; samples include OS scheduler and allocator noise",
        "plateau_observed": plateau_observed,
        "universal_absolute_rss_limit": null,
        "native_resource_count_after_each_invocation": 0,
        "native_resource_count_after_close": 0,
    });
    if let Some(results) = std::env::var_os("GUION_RUNTIME_MEMORY_RESULTS") {
        let results = std::path::PathBuf::from(results);
        fs::create_dir_all(&results).unwrap();
        fs::write(
            results.join(format!("runtime-memory-{target}.json")),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
    }
    assert!(
        plateau_observed,
        "post-warm-up RSS tail did not plateau: {report}"
    );
    let _ = fs::remove_dir_all(root);
}
