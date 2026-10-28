use guion_compiler::compile;
use guion_model::{BigInt, Float64, Type, Value};
use guion_runtime::{python_bridge::PythonBridge, CancellationToken, HostPolicy, Runtime};
use std::{collections::BTreeMap, path::PathBuf, process::Command, sync::Arc, time::Duration};

#[test]
fn cancellation_terminates_a_running_python_call() {
    let root = std::env::temp_dir().join(format!(
        "guion-python-cancel-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let marker = root.join("started");
    std::fs::write(
        root.join("slow_call.py"),
        format!(
            "import pathlib, time\ndef wait():\n    pathlib.Path({:?}).write_text('started')\n    time.sleep(10)\n    return 1\n",
            marker.to_string_lossy()
        ),
    )
    .unwrap();
    let mut bridge = PythonBridge::launch(&interpreter(), &root, Duration::from_secs(15)).unwrap();
    let cancellation = CancellationToken::new();
    let cancellation_from_thread = cancellation.clone();
    let marker_from_thread = marker.clone();
    let cancel_thread = std::thread::spawn(move || {
        for _ in 0..1_000 {
            if marker_from_thread.exists() {
                cancellation_from_thread.cancel();
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("Python function never started");
    });
    let started = std::time::Instant::now();

    let error = bridge
        .call_with_cancellation("slow_call", "wait", &[], &[], &Type::Int, &cancellation)
        .unwrap_err();
    cancel_thread.join().unwrap();

    assert_eq!(error.code, "R_CANCELLED");
    assert!(started.elapsed() < Duration::from_secs(3));
    std::fs::remove_dir_all(root).unwrap();
}

fn interpreter() -> PathBuf {
    let output = Command::new("python3")
        .args(["-c", "import sys; print(sys.executable)"])
        .output()
        .expect("the test host provides python3");
    assert!(output.status.success());
    PathBuf::from(String::from_utf8(output.stdout).unwrap().trim())
}

#[test]
fn bridge_preserves_large_integers_and_keyword_arguments() {
    let mut bridge = PythonBridge::launch(
        &interpreter(),
        &std::env::temp_dir(),
        Duration::from_secs(5),
    )
    .unwrap();
    let output = bridge
        .call(
            "builtins",
            "pow",
            &[Value::Int(BigInt::parse("2").unwrap())],
            &[("exp".into(), Value::Int(BigInt::parse("250").unwrap()))],
            &Type::Int,
        )
        .unwrap();
    assert_eq!(
        output.value,
        Value::Int(
            BigInt::parse(
                "1809251394333065553493296640760748560207343510400633813116524750123642650624"
            )
            .unwrap()
        )
    );
    assert!(output.stdout.is_empty());
}

#[test]
fn bridge_round_trips_typed_maps_as_python_dictionaries() {
    let mut values = BTreeMap::new();
    values.insert("pear".into(), Value::Int(BigInt::parse("4").unwrap()));
    values.insert("apple".into(), Value::Int(BigInt::parse("3").unwrap()));
    let mut bridge = PythonBridge::launch(
        &interpreter(),
        &std::env::temp_dir(),
        Duration::from_secs(5),
    )
    .unwrap();
    let output = bridge
        .call(
            "builtins",
            "dict",
            &[Value::Map(Arc::new(values))],
            &[],
            &Type::Map(Box::new(Type::Int)),
        )
        .expect("typed Map should cross the bridge");
    let Value::Map(values) = output.value else {
        panic!("expected Map result");
    };
    assert_eq!(
        values.get("apple"),
        Some(&Value::Int(BigInt::parse("3").unwrap()))
    );
    assert_eq!(
        values.get("pear"),
        Some(&Value::Int(BigInt::parse("4").unwrap()))
    );
}

#[test]
fn bridge_sends_packed_float64_lists_as_tagged_python_arguments() {
    let mut bridge = PythonBridge::launch(
        &interpreter(),
        &std::env::temp_dir(),
        Duration::from_secs(5),
    )
    .unwrap();
    let values = Value::Float64List(Arc::new(vec![
        Float64::new(0.25).unwrap(),
        Float64::new(0.5).unwrap(),
    ]));
    let output = bridge
        .call("builtins", "sum", &[values], &[], &Type::Float64)
        .expect("packed Float64 lists should use the same wire tags as regular lists");

    assert_eq!(output.value, Value::Float64(Float64::new(0.75).unwrap()));
}

#[test]
fn bridge_preserves_duplicate_keyword_error_precedence() {
    let mut bridge = PythonBridge::launch(
        &interpreter(),
        &std::env::temp_dir(),
        Duration::from_secs(5),
    )
    .unwrap();
    let error = bridge
        .call(
            "builtins",
            "dict",
            &[],
            &[
                ("value".into(), Value::Int(BigInt::parse("1").unwrap())),
                ("value".into(), Value::OptionNone),
            ],
            &Type::Unit,
        )
        .unwrap_err();
    assert_eq!(error.code, "R_PYTHON_CONVERSION");

    let error = bridge
        .call(
            "builtins",
            "dict",
            &[],
            &[
                ("value".into(), Value::Int(BigInt::parse("1").unwrap())),
                ("value".into(), Value::Int(BigInt::parse("2").unwrap())),
            ],
            &Type::Unit,
        )
        .unwrap_err();
    assert_eq!(error.code, "R_PYTHON_ARGUMENT");
}

#[test]
fn bridge_accepts_empty_packed_float_list_at_the_nesting_boundary() {
    let mut bridge = PythonBridge::launch(
        &interpreter(),
        &std::env::temp_dir(),
        Duration::from_secs(5),
    )
    .unwrap();
    let nested = (0..64).fold(Value::Float64List(Arc::new(Vec::new())), |value, _| {
        Value::List(Arc::new(vec![value]))
    });
    let output = bridge
        .call("builtins", "len", &[nested], &[], &Type::Int)
        .expect("an empty packed list has no child value that exceeds the depth limit");

    assert_eq!(output.value, Value::Int(BigInt::parse("1").unwrap()));
}

#[test]
fn bridge_round_trips_unicode_text_with_shared_storage() {
    let input = Value::Text("Guion: 東京 — café".into());
    let mut bridge = PythonBridge::launch(
        &interpreter(),
        &std::env::temp_dir(),
        Duration::from_secs(5),
    )
    .unwrap();
    let output = bridge
        .call(
            "builtins",
            "str",
            std::slice::from_ref(&input),
            &[],
            &Type::Text,
        )
        .expect("Text should cross the Python bridge in both directions");

    assert_eq!(output.value, input);
}

#[test]
fn bridge_transports_large_escaped_unicode_text_without_corruption() {
    let root = std::env::temp_dir().join(format!(
        "guion-python-large-text-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("text_size.py"),
        "def size(value):\n    return len(value)\n",
    )
    .unwrap();
    let repeated = "a\"\\\n東京";
    let input_text = repeated.repeat(300_000);
    let expected_characters = repeated.chars().count() * 300_000;
    let mut bridge = PythonBridge::launch(&interpreter(), &root, Duration::from_secs(10)).unwrap();
    let output = bridge
        .call(
            "text_size",
            "size",
            &[Value::Text(input_text.into())],
            &[],
            &Type::Int,
        )
        .expect("large Text should cross the framed Python bridge intact");

    assert_eq!(
        output.value,
        Value::Int(BigInt::parse(&expected_characters.to_string()).unwrap())
    );
    bridge.close();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn isolated_bridge_can_import_python_modules_from_the_project_root() {
    let root = std::env::temp_dir().join(format!(
        "guion-python-project-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("local_adapter.py"),
        "def answer():\n    return 42\n",
    )
    .unwrap();
    let mut bridge = PythonBridge::launch(&interpreter(), &root, Duration::from_secs(5)).unwrap();
    let output = bridge
        .call("local_adapter", "answer", &[], &[], &Type::Int)
        .expect("isolated Python should import the project's adapter module");
    assert_eq!(output.value, Value::Int(BigInt::parse("42").unwrap()));
    bridge.close();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn python_bridge_imports_from_project_but_uses_last_file_root_for_relative_paths() {
    let root = std::env::temp_dir().join(format!(
        "guion-python-file-roots-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let project = root.join("project");
    let caller = root.join("caller");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&caller).unwrap();
    std::fs::write(
        project.join("local_adapter.py"),
        "from pathlib import Path\ndef read_policy():\n    return Path('policy.json').read_text(encoding='utf-8')\n",
    )
    .unwrap();
    std::fs::write(caller.join("policy.json"), "caller policy\n").unwrap();

    let source = "extern python \"local_adapter:read_policy\" fn read_policy() -> Text\n\nfn main() -> Int:\n    print(read_policy())\n    return 0\n";
    let artifact = compile("bridge-roots.gu", source).unwrap();
    let policy = HostPolicy::default()
        .with_file_access(&project, true, true)
        .with_additional_file_root(&caller)
        .with_python(interpreter(), Duration::from_secs(5));
    let mut instance = Runtime::new().load(artifact, policy).unwrap();

    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::zero())
    );
    assert_eq!(instance.take_output(), "caller policy\n\n");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn source_programs_can_call_python_with_maps_and_receive_typed_maps() {
    let source = "extern python \"builtins:dict\" fn copy_map(values: Map[Text, Int]) -> Map[Text, Int]\n\nfn main() -> Int:\n    let values = {\"y\": 2, \"x\": 1}\n    let copied = copy_map(values)\n    print(copied[\"x\"])\n    return len(copied)\n";
    let artifact = compile("python-map.gu", source).expect("typed Map ABI compiles");
    let policy = HostPolicy::default().with_python(interpreter(), Duration::from_secs(5));
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::parse("2").unwrap())
    );
    assert_eq!(instance.take_output(), "1\n");
}

#[test]
fn source_python_calls_round_trip_nested_heterogeneous_json() {
    let source = r#"extern python "json:loads" fn decode_document(value: Text) -> Json
extern python "builtins:dict" fn copy_document(value: Json) -> Json

fn main() -> Int:
    let source = "{\"records\":[{\"id\":9007199254740993,\"labels\":[\"Crème\",\"東京\"],\"active\":true},{\"id\":2,\"labels\":[],\"active\":false}],\"meta\":{\"source\":\"python\",\"version\":1}}"
    let decoded = decode_document(source)
    let copied = copy_document(decoded)
    let records = json_get(copied, "records")
    assert(json_len(records) == 2)
    assert(json_as_int(json_get(json_index(records, 0), "id")) == 9007199254740993)
    assert(json_as_text(json_index(json_get(json_index(records, 0), "labels"), 1)) == "東京")
    assert(json_as_bool(json_get(json_index(records, 0), "active")))
    print(json_stringify(copied))
    return 0
"#;
    let artifact = compile("python-json.gu", source).expect("Json ABI should typecheck");
    let artifact = guion_model::Artifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let policy = HostPolicy::default().with_python(interpreter(), Duration::from_secs(5));
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::zero())
    );
    assert_eq!(
        instance.take_output(),
        "{\"meta\":{\"source\":\"python\",\"version\":1},\"records\":[{\"active\":true,\"id\":9007199254740993,\"labels\":[\"Crème\",\"東京\"]},{\"active\":false,\"id\":2,\"labels\":[]}]}\n"
    );
}

#[test]
fn numpy_arrays_convert_to_their_declared_guion_list_shape() {
    let source = "extern python \"numpy:arange\" fn sequence(stop: Int) -> List[Int]\nextern python \"numpy:eye\" fn identity(size: Int) -> List[List[Float64]]\n\nfn main() -> Int:\n    let values = sequence(6)\n    let matrix = identity(2)\n    assert(len(values) == 6)\n    assert(values[5] == 5)\n    assert(matrix[0][0] == 1.0)\n    assert(matrix[0][1] == 0.0)\n    assert(matrix[1][1] == 1.0)\n    return values[0] + values[1] + values[2] + values[3] + values[4] + values[5]\n";
    let artifact = compile("python-numpy.gu", source).expect("NumPy List ABI should typecheck");
    let policy = HostPolicy::default().with_python(interpreter(), Duration::from_secs(5));
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::parse("15").unwrap())
    );
}

#[test]
fn numpy_array_materialization_rejects_oversized_temporary_values() {
    let root = std::env::temp_dir().join(format!(
        "guion-python-numpy-size-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("numpy_boundary.py"),
        "import numpy as np\ndef large():\n    return np.arange(100_000, dtype=np.int64)\n",
    )
    .unwrap();
    let mut bridge = PythonBridge::launch(&interpreter(), &root, Duration::from_secs(5)).unwrap();
    let error = match bridge.call(
        "numpy_boundary",
        "large",
        &[],
        &[],
        &Type::List(Box::new(Type::Int)),
    ) {
        Err(error) => error,
        Ok(output) => {
            let count = match output.value {
                Value::List(values) => values.len(),
                _ => 0,
            };
            panic!("expected the array size guard to reject the result, got {count} items");
        }
    };
    assert_eq!(error.code, "R_PYTHON_CALL");
    assert_eq!(error.python_type.as_deref(), Some("ValueError"));
    bridge.close();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn python_json_results_reject_non_json_values_and_cycles() {
    let root = std::env::temp_dir().join(format!(
        "guion-python-json-boundary-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("json_boundary.py"),
        "def non_string_keys():\n    return {1: 'value'}\ndef cycle():\n    value = []\n    value.append(value)\n    return value\ndef non_finite():\n    return float('nan')\ndef too_deep():\n    value = None\n    for _ in range(65):\n        value = [value]\n    return value\n",
    )
    .unwrap();
    let mut bridge = PythonBridge::launch(&interpreter(), &root, Duration::from_secs(5)).unwrap();
    for (function, python_type) in [
        ("non_string_keys", "TypeError"),
        ("cycle", "ValueError"),
        ("non_finite", "ValueError"),
        ("too_deep", "ValueError"),
    ] {
        let error = bridge
            .call("json_boundary", function, &[], &[], &Type::Json)
            .expect_err("invalid Python values must not cross the Json ABI");
        assert_eq!(error.code, "R_PYTHON_CALL");
        assert_eq!(error.python_type.as_deref(), Some(python_type));
    }
    bridge.close();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn float64_values_cross_python_calls_and_return_with_exact_binary64_value() {
    let source = "extern python \"builtins:float\" fn echo(value: Float64) -> Float64\n\nfn main() -> Int:\n    let value = echo(0.125)\n    assert(value == 0.125)\n    return 0\n";
    let artifact = compile("python-float.gu", source).expect("Float64 ABI compiles");
    let policy = HostPolicy::default().with_python(interpreter(), Duration::from_secs(5));
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::zero())
    );

    let mut bridge = PythonBridge::launch(
        &interpreter(),
        &std::env::temp_dir(),
        Duration::from_secs(5),
    )
    .unwrap();
    let value = Float64::new(0.125).unwrap();
    let returned = bridge
        .call(
            "builtins",
            "float",
            &[Value::Float64(value)],
            &[],
            &Type::Float64,
        )
        .unwrap();
    assert_eq!(returned.value, Value::Float64(value));
}

#[test]
fn bridge_captures_package_output_separately_from_protocol_frames() {
    let mut bridge = PythonBridge::launch(
        &interpreter(),
        &std::env::temp_dir(),
        Duration::from_secs(5),
    )
    .unwrap();
    let output = bridge
        .call(
            "builtins",
            "print",
            &[Value::Text("kept out of the wire".into())],
            &[],
            &Type::Unit,
        )
        .unwrap();
    assert_eq!(output.value, Value::Unit);
    assert_eq!(output.stdout, "kept out of the wire\n");
}

#[test]
fn bridge_reports_python_exceptions_and_rejects_wrong_declared_return_types() {
    let mut bridge = PythonBridge::launch(
        &interpreter(),
        &std::env::temp_dir(),
        Duration::from_secs(5),
    )
    .unwrap();
    let error = bridge
        .call(
            "math",
            "factorial",
            &[Value::Int(BigInt::parse("-1").unwrap())],
            &[],
            &Type::Int,
        )
        .unwrap_err();
    assert_eq!(error.code, "R_PYTHON_CALL");
    assert_eq!(error.python_type.as_deref(), Some("ValueError"));
    assert!(error.traceback.unwrap().contains("factorial"));

    let wrong = bridge
        .call(
            "math",
            "factorial",
            &[Value::Int(BigInt::parse("5").unwrap())],
            &[],
            &Type::Text,
        )
        .unwrap_err();
    assert_eq!(wrong.code, "R_PYTHON_RETURN_TYPE");
}

#[test]
fn bridge_timeout_terminates_the_sidecar() {
    let mut bridge = PythonBridge::launch(
        &interpreter(),
        &std::env::temp_dir(),
        Duration::from_millis(100),
    )
    .unwrap();
    let error = bridge
        .call(
            "time",
            "sleep",
            &[Value::Int(BigInt::parse("2").unwrap())],
            &[],
            &Type::Unit,
        )
        .unwrap_err();
    assert_eq!(error.code, "R_PYTHON_TIMEOUT");
}

#[test]
fn source_external_calls_run_through_the_artifact_runtime_path() {
    let source = "extern python \"builtins:pow\" fn pow(base: Int, exponent: Int) -> Int\n\nfn main() -> Int:\n    let answer = pow(2, 80)\n    print(answer)\n    return 0\n";
    let artifact = compile("interop.gu", source).unwrap();
    let bytes = artifact.to_bytes().unwrap();
    let artifact = guion_model::Artifact::from_bytes(&bytes).unwrap();
    let policy = HostPolicy::default().with_python(interpreter(), Duration::from_secs(5));
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::zero())
    );
    assert_eq!(instance.take_output(), "1208925819614629174706176\n");
    assert!(instance.take_host_stderr().is_empty());
    instance.close();
}

#[test]
fn source_external_calls_require_an_explicit_interpreter() {
    let source = "extern python \"math:factorial\" fn factorial(value: Int) -> Int\n\nfn main() -> Int:\n    return factorial(6)\n";
    let artifact = compile("interop-unconfigured.gu", source).unwrap();
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap_err().code,
        "R_PYTHON_UNCONFIGURED"
    );
}

#[test]
fn binary_bytes_round_trip_through_file_host_calls_and_python_bridge() {
    let root = std::env::temp_dir().join(format!("guion-bytes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let input = [0, 0xff, 0x80, b'a', b'\n', 0, 0xc3, 0xa9];
    std::fs::write(root.join("input.bin"), input).unwrap();
    let source = "extern python \"builtins:bytes\" fn echo(data: Bytes) -> Bytes\n\nfn main() -> Int:\n    let raw = unwrap(read_bytes(\"input.bin\"))\n    assert(len(raw) == 8)\n    let copied = echo(raw)\n    unwrap(write_bytes(\"output.bin\", copied))\n    return 0\n";
    let artifact = compile("bytes.gu", source).expect("Bytes program should typecheck");
    let artifact = guion_model::Artifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let policy = HostPolicy::default()
        .with_file_access(root.clone(), true, true)
        .with_python(interpreter(), Duration::from_secs(5));
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::zero())
    );
    assert_eq!(std::fs::read(root.join("output.bin")).unwrap(), input);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn source_python_calls_accept_keyword_arguments_and_python_defaults() {
    let source = "extern python \"builtins:pow\" fn power(base: Int, exp: Int, mod: Int) -> Int\n\nfn main() -> Int:\n    return power(2, exp=10)\n";
    let artifact = compile("python-keywords.gu", source).expect("keyword call should typecheck");
    let artifact = guion_model::Artifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let policy = HostPolicy::default().with_python(interpreter(), Duration::from_secs(5));
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::parse("1024").unwrap())
    );
}

#[test]
fn source_python_calls_report_missing_required_python_arguments_at_runtime() {
    let source = "extern python \"builtins:pow\" fn power(base: Int, exp: Int) -> Int\n\nfn main() -> Int:\n    return power(2)\n";
    let artifact = compile("python-required-argument.gu", source)
        .expect("external signatures leave Python defaults to the runtime");
    let policy = HostPolicy::default().with_python(interpreter(), Duration::from_secs(5));
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap_err().code,
        "R_PYTHON_CALL"
    );
}

#[test]
fn source_python_calls_reject_unknown_duplicate_and_misordered_arguments() {
    let prefix = "extern python \"builtins:pow\" fn power(base: Int, exp: Int) -> Int\n\nfn main() -> Int:\n    return ";
    for (args, expected) in [
        ("power(2, exponent=10)", "E_PYTHON_KEYWORD"),
        ("power(2, exp=10, exp=11)", "E_PYTHON_KEYWORD_DUPLICATE"),
        ("power(exp=10, 2)", "E_PYTHON_ARGUMENT_ORDER"),
    ] {
        let error = compile("python-keyword-error.gu", &format!("{prefix}{args}\n")).unwrap_err();
        assert_eq!(error.code, expected, "unexpected diagnostic for {args}");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn bridge_reports_native_images_loaded_dynamically_by_python() {
    let root = std::env::temp_dir().join(format!(
        "guion-python-native-image-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let source = root.join("probe.c");
    let library = root.join("libguion_probe.dylib");
    std::fs::write(&source, "int guion_probe(void) { return 42; }\n").unwrap();
    let status = Command::new("cc")
        .args(["-dynamiclib", "-o"])
        .arg(&library)
        .arg(&source)
        .status()
        .expect("the macOS test host provides a C compiler");
    assert!(status.success(), "the native probe library should compile");
    std::fs::write(
        root.join("native_loader.py"),
        "import ctypes\ndef load(path):\n    library = ctypes.CDLL(path)\n    return library.guion_probe()\n",
    )
    .unwrap();
    let mut bridge = PythonBridge::launch(&interpreter(), &root, Duration::from_secs(5)).unwrap();
    let output = bridge
        .call(
            "native_loader",
            "load",
            &[Value::Text(library.to_string_lossy().into_owned().into())],
            &[],
            &Type::Int,
        )
        .expect("Python should load the native probe");
    assert_eq!(output.value, Value::Int(BigInt::parse("42").unwrap()));
    assert_eq!(bridge.native_image_observation(), "macos_dyld_snapshot");
    let library = std::fs::canonicalize(&library).unwrap();
    assert!(bridge
        .native_images()
        .contains(&library.to_string_lossy().into_owned()));
    bridge.close();
    std::fs::remove_dir_all(root).unwrap();
}
