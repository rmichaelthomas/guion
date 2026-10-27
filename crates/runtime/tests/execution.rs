use guion_compiler::compile;
use guion_model::{Artifact, BigInt, Float64, Op, SharedJson, Value};
use guion_runtime::{
    CancellationToken, EffectKind, EffectOutcome, EffectRecord, HostPolicy, Runtime,
};
use std::time::Duration;

#[test]
fn text_length_and_index_use_unicode_scalar_positions() {
    let source =
        "fn main() -> Int:\n    print(len(\"a🙂é\"))\n    print(\"a🙂é\"[1])\n    return 0\n";
    let artifact = compile("text-index.gu", source).expect("text operations compile");
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .expect("runtime loads the artifact");

    instance.invoke("main", vec![]).expect("program runs");

    assert_eq!(instance.take_output(), "3\n🙂\n");
}

#[test]
fn runtime_profile_is_absent_by_default_and_covers_native_and_vm_execution() {
    let source = "record Marker:\n    value: Int\n\nfn main() -> Int:\n    var values = [1, 2, 3]\n    set values = list_set(values, 1, 9)\n    let shared = values\n    set values = list_set(values, 0, 7)\n    let marker = Marker(values[1])\n    var counter = 0\n    while counter < 2:\n        set counter = counter + 1\n    print(shared[1])\n    print(marker.value)\n    return 0\n";
    let artifact = compile("runtime-profile.gu", source).expect("profile fixture compiles");

    let mut ordinary = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .expect("runtime loads without profiling");
    ordinary.invoke("main", vec![]).expect("program runs");
    assert_eq!(ordinary.take_output(), "9\n9\n");
    assert!(ordinary.runtime_profile().is_none());

    for runtime in [
        Runtime::new().with_runtime_profile(),
        Runtime::vm_only().with_runtime_profile(),
    ] {
        let mut instance = runtime
            .load(artifact.clone(), HostPolicy::default())
            .expect("runtime loads with profiling");
        instance
            .invoke("main", vec![])
            .expect("profiled program runs");
        assert_eq!(instance.take_output(), "9\n9\n");

        let profile = instance.runtime_profile().expect("profile is enabled");
        let allocation = &profile["estimated_guest_allocation_budget_charges"];
        assert!(allocation["successful_events"].as_u64().unwrap() > 0);
        assert!(allocation["estimated_net_bytes"].as_u64().unwrap() > 0);
        assert_eq!(profile["list_set"]["operations"].as_u64(), Some(2));
        assert!(profile["list_set"]["in_place_updates"].as_u64().unwrap() > 0);
        assert!(
            profile["list_set"]["copy_on_write_copies"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert!(
            profile["list_set"]["estimated_copy_bytes"]
                .as_u64()
                .unwrap()
                > 0
        );
        let histogram = &profile["list_set"]["copy_size_histogram"]["buckets"];
        let bucket_total: u64 = ["0-8", "9-32", "33-128", "129+"]
            .into_iter()
            .map(|bucket| histogram[bucket]["copies"].as_u64().unwrap())
            .sum();
        assert_eq!(
            bucket_total,
            profile["list_set"]["copy_on_write_copies"]
                .as_u64()
                .unwrap()
        );
        assert_eq!(profile["record_charges"]["events"].as_u64(), Some(1));
        assert!(
            profile["record_charges"]["estimated_bytes"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert!(
            profile["dynamic_guest_conditional_branch_executions"]
                .as_u64()
                .unwrap()
                >= 3
        );
    }
}

#[cfg(not(feature = "python-interop"))]
#[test]
fn core_profile_reports_python_externals_as_unavailable_without_launching_them() {
    let source = "extern python \"builtins:pow\" fn power(base: Int, exponent: Int) -> Int\n\nfn main() -> Int:\n    return power(2, 10)\n";
    let artifact = compile("python-optional-profile.gu", source)
        .expect("optional external declarations remain type checked");
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .expect("core runtime loads external declarations");
    let error = instance.invoke("main", vec![]).unwrap_err();
    assert_eq!(error.code, "R_PYTHON_UNAVAILABLE");
    assert!(error.message.contains("python-interop"));
}

#[test]
fn text_chars_materializes_unicode_scalars_for_linear_scanning() {
    let source = "fn main() -> Int:\n    let characters = text_chars(\"a🙂é\")\n    print(len(characters))\n    print(characters[1])\n    return 0\n";
    let artifact = compile("text-chars.gu", source).expect("text_chars compiles");
    let mut previous_format = artifact.clone();
    previous_format.format_version = 30;
    assert_eq!(
        previous_format.to_bytes().unwrap_err().code,
        "E_ARTIFACT_VERSION"
    );
    let artifact = Artifact::from_bytes(&artifact.to_bytes().unwrap())
        .expect("format 31 preserves the text_chars operation");
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .expect("runtime loads the artifact");

    instance.invoke("main", vec![]).expect("program runs");

    assert_eq!(instance.take_output(), "3\n🙂\n");
}

#[test]
fn text_chars_reuses_immutable_storage_for_ascii_scalars() {
    let source = "fn ascii_characters() -> List[Text]:\n    return text_chars(\"aa\")\n\nfn main() -> Int:\n    return len(ascii_characters())\n";
    let artifact = compile("text-chars-shared-ascii.gu", source)
        .expect("text_chars compiles for an ASCII pair");
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .expect("runtime loads the artifact");

    instance.invoke("main", vec![]).expect("entry point runs");
    let Value::List(characters) = instance
        .invoke("ascii_characters", vec![])
        .expect("program returns its characters")
    else {
        panic!("text_chars must return a list");
    };
    let (Value::Text(first), Value::Text(second)) = (&characters[0], &characters[1]) else {
        panic!("text_chars must return text values");
    };

    assert!(std::sync::Arc::ptr_eq(first, second));
}

#[test]
fn bounded_byte_construction_primitives_remain_compatible_with_format_34() {
    let source = "fn main() -> Int:\n    let raw = bytes_from_ints([0, 255])\n    let utf8 = bytes_from_text(\"é\")\n    let joined = bytes_join([raw, utf8])\n    print(len(joined))\n    print(float64_bits(1.0))\n    return 0\n";
    let artifact =
        compile("artifact-byte-primitives.gu", source).expect("byte construction source compiles");
    assert_eq!(artifact.format_version, 39);
    let mut previous_format = artifact.clone();
    previous_format.format_version = 33;
    assert_eq!(
        previous_format.to_bytes().unwrap_err().code,
        "E_ARTIFACT_VERSION"
    );
    let mut format_34 = artifact;
    format_34.format_version = 34;
    let artifact = Artifact::from_bytes(&format_34.to_bytes().unwrap())
        .expect("format 34 preserves bounded byte operations");
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .expect("runtime verifies bounded byte operations");
    instance.invoke("main", vec![]).expect("program runs");
    assert_eq!(instance.take_output(), "4\n4607182418800017408\n");
}

#[test]
fn bytes_from_ints_rejects_values_outside_byte_range() {
    let source = "fn main() -> Int:\n    let invalid = bytes_from_ints([256])\n    return 0\n";
    let artifact = compile("artifact-byte-range.gu", source)
        .expect("range is checked by the bounded byte constructor");
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .expect("runtime loads byte constructor");
    assert_eq!(
        instance.invoke("main", vec![]).unwrap_err().code,
        "R_BYTE_RANGE"
    );
}

#[test]
fn host_cancellation_interrupts_native_execution() {
    let source = "fn spin(value: Int) -> Int:\n    var i = 0\n    while i < value:\n        set i = i + 1\n    return i\n\nfn main() -> Int:\n    return spin(1)\n";
    let artifact = compile("native-cancel.gu", source).expect("native loop compiles");
    let policy = HostPolicy {
        instruction_limit: 1_000_000_000_000,
        ..HostPolicy::default()
    };
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    instance.invoke("main", vec![]).unwrap(); // Compile the native backend before timing cancellation.
    let cancellation = CancellationToken::new();
    let cancellation_from_thread = cancellation.clone();
    let cancel_thread = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        cancellation_from_thread.cancel();
    });
    let started = std::time::Instant::now();
    let error = instance
        .invoke_with_cancellation(
            "spin",
            vec![Value::Int(BigInt::parse("1000000000000").unwrap())],
            cancellation,
        )
        .unwrap_err();
    cancel_thread.join().unwrap();

    assert_eq!(error.code, "R_CANCELLED");
    assert!(instance
        .native_function_names()
        .iter()
        .any(|name| name == "spin"));
    assert!(
        instance.native_calls() > 0,
        "cancellation reached the JIT function"
    );
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn host_cancellation_interrupts_vm_execution() {
    let source = "fn main() -> Int:\n    print(\"running\")\n    return 0\n";
    let artifact = compile("vm-cancel.gu", source).expect("host effect stays on VM");
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    let cancellation = CancellationToken::new();
    cancellation.cancel();

    let error = instance
        .invoke_with_cancellation("main", vec![], cancellation)
        .unwrap_err();

    assert_eq!(error.code, "R_CANCELLED");
}

#[test]
fn program_stderr_is_separate_and_reported_as_an_effect() {
    let source = "fn main() -> Int:\n    eprint(\"duplicate id\")\n    return 0\n";
    let artifact = compile("program-stderr.gu", source).unwrap();
    let mut legacy = artifact.clone();
    legacy.format_version = 27;
    assert!(legacy.to_bytes().unwrap_err().message.contains("format 28"));
    let artifact = guion_model::Artifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();

    instance.invoke("main", vec![]).unwrap();

    assert_eq!(instance.take_output(), "");
    assert_eq!(instance.take_program_stderr(), "duplicate id\n");
    assert_eq!(
        instance.effect_report(),
        &[EffectRecord {
            kind: EffectKind::ProgramErrorOutput,
            target: "13 bytes".into(),
            target_truncated: false,
            outcome: EffectOutcome::Returned,
        }]
    );
}

#[test]
fn host_cancellation_joins_parallel_native_workers_and_returns_cancelled() {
    let source = "fn spin(value: Int) -> Int:\n    var i = 0\n    while true:\n        set i = i + value\n    return i\n\nfn run() -> Result[List[Int], TaskError]:\n    return parallel_map([1, 2, 3, 4], spin, 4)\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile("parallel-host-cancel.gu", source).unwrap();
    let policy = HostPolicy {
        instruction_limit: 1_000_000_000_000,
        max_parallelism: 4,
        ..HostPolicy::default()
    };
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    let cancellation = CancellationToken::new();
    let cancellation_from_thread = cancellation.clone();
    let cancel_thread = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        cancellation_from_thread.cancel();
    });
    let started = std::time::Instant::now();

    let error = instance
        .invoke_with_cancellation("run", vec![], cancellation)
        .unwrap_err();
    cancel_thread.join().unwrap();

    assert_eq!(error.code, "R_CANCELLED");
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(instance.parallel_threads_used() > 0);
}

#[test]
fn cancellation_report_preserves_completed_file_effects() {
    let root = std::env::temp_dir().join(format!(
        "guion-effect-report-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let source = "fn spin(value: Int) -> Int:\n    var i = 0\n    while true:\n        set i = i + value\n    return i\n\nfn main() -> Int:\n    let saved = unwrap(write_text_atomic(\"result.txt\", \"persisted\"))\n    let never = spin(1)\n    return 0\n";
    let artifact = compile("effect-report.gu", source).unwrap();
    let policy = HostPolicy {
        instruction_limit: 1_000_000_000_000,
        ..HostPolicy::default()
    }
    .with_file_access(&root, true, true);
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    let destination = root.join("result.txt");
    let cancellation = CancellationToken::new();
    let cancellation_from_thread = cancellation.clone();
    let destination_from_thread = destination.clone();
    let cancel_thread = std::thread::spawn(move || {
        for _ in 0..1_000 {
            if destination_from_thread.exists() {
                cancellation_from_thread.cancel();
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("Guion did not complete the atomic file write");
    });

    let error = instance
        .invoke_with_cancellation("main", vec![], cancellation)
        .unwrap_err();
    cancel_thread.join().unwrap();

    assert_eq!(error.code, "R_CANCELLED");
    assert_eq!(std::fs::read_to_string(&destination).unwrap(), "persisted");
    assert_eq!(
        instance.effect_report(),
        &[EffectRecord {
            kind: EffectKind::FileWrite,
            target: "result.txt".into(),
            target_truncated: false,
            outcome: EffectOutcome::Returned,
        }]
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn effect_reporting_is_bounded_and_marks_omitted_events() {
    let source = "fn main() -> Int:\n    var i = 0\n    while i < 1100:\n        print(i)\n        set i = i + 1\n    return 0\n";
    let artifact = compile("bounded-effects.gu", source).unwrap();
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();

    instance.invoke("main", vec![]).unwrap();

    assert_eq!(instance.effect_report().len(), 1024);
    assert_eq!(instance.omitted_effects(), 76);
    instance.invoke("main", vec![]).unwrap();
    assert_eq!(instance.effect_report().len(), 1024);
    assert_eq!(instance.omitted_effects(), 76);
}

#[test]
fn typed_empty_collections_work_in_locals_mutations_and_calls() {
    let source = "fn count(values: List[Int]) -> Int:\n    return len(values)\n\nfn empty_values() -> List[Int]:\n    return []\n\nfn main() -> Int:\n    var values: List[Int] = []\n    set values = [1, 2]\n    let groups: Map[Text, Int] = {}\n    let updated = map_set(groups, \"one\", count([]))\n    return len(values) + updated[\"one\"] + len(empty_values())\n";
    let artifact = compile("typed-empty-collections.gu", source)
        .expect("annotations provide element types to empty collection literals");
    assert_eq!(artifact.format_version, Artifact::CURRENT_VERSION);
    let mut old_format = artifact.clone();
    old_format.format_version = 24;
    assert_eq!(
        old_format.to_bytes().unwrap_err().code,
        "E_ARTIFACT_VERSION"
    );
    let encoded = artifact.to_bytes().unwrap();
    let artifact = guion_model::Artifact::from_bytes(&encoded).unwrap();
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();

    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::parse("2").unwrap())
    );
}

#[test]
fn lists_support_immutable_push_and_index_replacement() {
    let source = "fn main() -> Int:\n    let original = [1, 2]\n    let appended = list_push(original, 3)\n    let replaced = list_set(appended, 1, 9)\n    assert(original[1] == 2)\n    assert(appended[1] == 2)\n    assert(replaced[1] == 9)\n    assert(len(replaced) == 3)\n    return replaced[0] + replaced[1] + replaced[2]\n";
    let artifact = compile("immutable-list-update.gu", source)
        .expect("typed lists support immutable updates without aliasing");
    assert_eq!(artifact.format_version, Artifact::CURRENT_VERSION);
    let mut old_format = artifact.clone();
    old_format.format_version = 25;
    assert_eq!(
        old_format.to_bytes().unwrap_err().code,
        "E_ARTIFACT_VERSION"
    );
    let artifact = guion_model::Artifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::parse("13").unwrap())
    );
}

#[test]
fn list_set_assignment_keeps_old_aliases_unchanged() {
    let source = "fn main() -> Int:\n    let original = [1, 2]\n    var updated = original\n    set updated = list_set(updated, 0, 9)\n    print(original[0])\n    print(updated[0])\n    return 0\n";
    let artifact = compile("list-alias.gu", source).unwrap();
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    instance.invoke("main", vec![]).unwrap();
    assert_eq!(instance.take_output(), "1\n9\n");
}

#[test]
fn native_list_set_uses_element_typed_helpers_without_changing_copy_semantics() {
    let source = "fn main() -> Int:\n    let original_ints = [1, 2, 3]\n    let updated_ints = list_set(original_ints, 1, 9)\n    let original_bools = [true, false, true]\n    let updated_bools = list_set(original_bools, 1, true)\n    let original_floats = [1.0, 2.0, 3.0]\n    let updated_floats = list_set(original_floats, 1, 9.0)\n    var local_ints = [1, 2, 3]\n    set local_ints = list_set(local_ints, 1, 8)\n    var local_bools = [true, false, true]\n    set local_bools = list_set(local_bools, 1, true)\n    var local_floats = [1.0, 2.0, 3.0]\n    set local_floats = list_set(local_floats, 1, 8.0)\n    print(updated_ints[1])\n    print(updated_bools[1])\n    print(updated_floats[1])\n    print(original_ints[1])\n    print(original_bools[1])\n    print(original_floats[1])\n    print(local_ints[1])\n    print(local_bools[1])\n    print(local_floats[1])\n    return 0\n";
    let artifact = compile("native-typed-list-set.gu", source).expect("source compiles");
    assert!(
        artifact.functions[0]
            .code
            .iter()
            .filter(|instruction| matches!(instruction.op, Op::ListSetLocal(_)))
            .count()
            >= 3,
        "all local element types should use the in-place-capable opcode"
    );
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .expect("native runtime loads");
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .expect("VM runtime loads");

    for runtime in [&mut native, &mut vm] {
        assert_eq!(
            runtime.invoke("main", vec![]).unwrap(),
            Value::Int(BigInt::from_i64(0))
        );
        assert_eq!(
            runtime.take_output(),
            "9\ntrue\n9\n2\nfalse\n2\n8\ntrue\n8\n"
        );
    }

    let profile = native.native_compile_profile();
    let main_instructions = profile["functions"]
        .as_array()
        .expect("native compile profile has functions")
        .iter()
        .find(|function| function["name"].as_str() == Some("main"))
        .expect("main has a native compile profile")["cranelift_ir_instructions"]
        .as_u64()
        .expect("main IR instruction count is present");
    assert!(
        main_instructions <= 461,
        "typed list-set calls should remove runtime kind and copy-mode operands: {main_instructions} instructions"
    );
}

#[test]
fn same_local_list_updates_do_not_accumulate_full_list_copies() {
    let source = "fn main() -> Int:\n    var values = [0.0]\n    var i = 0\n    while i < 5000:\n        set values = list_set(values, 0, values[0] + 1.0)\n        set i = i + 1\n    print(values[0])\n    return i\n";
    let artifact = compile("list-update-budget.gu", source).unwrap();
    assert!(artifact.functions[0]
        .code
        .iter()
        .any(|instruction| matches!(instruction.op, Op::ListSetLocal(_))));
    let legacy = {
        let mut artifact = artifact.clone();
        artifact.format_version = 25;
        artifact
    };
    assert_eq!(legacy.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");
    let encoded = artifact.to_bytes().unwrap();
    let artifact = guion_model::Artifact::from_bytes(&encoded).unwrap();
    let policy = HostPolicy {
        allocation_limit_bytes: 1024,
        ..HostPolicy::default()
    };
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::from_usize(5000))
    );
    assert_eq!(instance.take_output(), "5000\n");
}

#[test]
fn same_local_map_updates_fit_a_linear_allocation_budget() {
    let source = "fn main() -> Int:\n    var values: Map[Text, Float64] = {}\n    let key = \"seen\"\n    var i = 0\n    while i < 5000:\n        set values = map_set(values, key, 1.0)\n        set i = i + 1\n    assert(values[key] == 1.0)\n    return i\n";
    let artifact = compile("map-update-budget.gu", source).unwrap();
    assert!(artifact.functions[0].code.windows(2).any(|window| {
        matches!(window[0].op, Op::MapSet) && matches!(window[1].op, Op::Store(_))
    }));
    let policy = HostPolicy {
        allocation_limit_bytes: 256 * 1024,
        ..HostPolicy::default()
    };
    let mut instance = Runtime::new().load(artifact, policy).unwrap();

    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::from_usize(5000))
    );
}

#[test]
fn same_local_map_update_preserves_other_shared_aliases() {
    let source = "fn main() -> Int:\n    var values: Map[Text, Int] = {\"one\": 1}\n    let previous = values\n    set values = map_set(values, \"two\", 2)\n    assert(previous[\"one\"] == 1)\n    assert(not map_has(previous, \"two\"))\n    assert(values[\"two\"] == 2)\n    return len(previous) + len(values)\n";
    let artifact = compile("map-update-alias.gu", source).unwrap();
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();

    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::from_usize(3))
    );
}

#[test]
fn same_local_map_update_still_enforces_text_limits_on_new_values() {
    let source = "fn main() -> Int:\n    var values: Map[Text, Text] = {}\n    set values = map_set(values, \"ok\", \"long\")\n    return 0\n";
    let artifact = compile("map-update-text-limit.gu", source).unwrap();
    let policy = HostPolicy {
        max_text_bytes: 3,
        ..HostPolicy::default()
    };
    let mut instance = Runtime::new().load(artifact, policy).unwrap();

    assert_eq!(
        instance.invoke("main", vec![]).unwrap_err().code,
        "R_TEXT_LIMIT"
    );
}

#[test]
fn same_local_map_update_still_enforces_collection_limits() {
    let source = "fn main() -> Int:\n    var values: Map[Text, Int] = {\"one\": 1}\n    set values = map_set(values, \"two\", 2)\n    return 0\n";
    let artifact = compile("map-update-collection-limit.gu", source).unwrap();
    let policy = HostPolicy {
        max_collection_items: 1,
        ..HostPolicy::default()
    };
    let mut instance = Runtime::new().load(artifact, policy).unwrap();

    assert_eq!(
        instance.invoke("main", vec![]).unwrap_err().code,
        "R_COLLECTION_LIMIT"
    );
}

#[test]
fn same_local_list_pushes_fit_a_linear_allocation_budget() {
    let source = "fn main() -> Int:\n    var values: List[Int] = []\n    var i = 0\n    while i < 6000:\n        set values = list_push(values, i)\n        set i = i + 1\n    return len(values)\n";
    let artifact = compile("list-push-budget.gu", source).unwrap();
    assert_eq!(artifact.format_version, Artifact::CURRENT_VERSION);
    assert!(artifact.functions[0]
        .code
        .iter()
        .any(|instruction| matches!(instruction.op, Op::ListPushLocal(_))));
    let mut legacy = artifact.clone();
    legacy.format_version = 31;
    assert_eq!(legacy.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");
    let artifact = Artifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let policy = HostPolicy {
        allocation_limit_bytes: 64 * 1024 * 1024,
        ..HostPolicy::default()
    };
    let mut instance = Runtime::new().load(artifact, policy).unwrap();

    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::from_usize(6000))
    );
}

#[test]
fn optimized_list_push_preserves_older_shared_aliases() {
    let source = "fn main() -> Int:\n    var values = [1]\n    let previous = values\n    set values = list_push(values, 2)\n    assert(len(previous) == 1)\n    assert(previous[0] == 1)\n    assert(len(values) == 2)\n    assert(values[1] == 2)\n    var floats: List[Float64] = []\n    set floats = list_push(floats, 1.5)\n    assert(floats[0] == 1.5)\n    return 0\n";
    let artifact = compile("list-push-alias.gu", source).unwrap();
    assert!(artifact.functions[0]
        .code
        .iter()
        .any(|instruction| matches!(instruction.op, Op::ListPushLocal(_))));
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();

    instance.invoke("main", vec![]).unwrap();
}

#[test]
fn mutable_json_array_push_is_linear_and_preserves_aliases() {
    let source = "fn main() -> Int:\n    var values = json_array()\n    var cursor = 0\n    while cursor < 2500:\n        set values = json_push(values, json_int(cursor))\n        set cursor = cursor + 1\n    let previous = values\n    set values = json_push(values, json_int(2500))\n    assert(json_len(previous) == 2500)\n    assert(json_len(values) == 2501)\n    assert(json_as_int(json_index(previous, 2499)) == 2499)\n    assert(json_as_int(json_index(values, 2500)) == 2500)\n    return json_len(values)\n";
    let artifact = compile("json-push-local.gu", source).unwrap();
    assert_eq!(artifact.format_version, Artifact::CURRENT_VERSION);
    assert!(artifact.functions[0]
        .code
        .iter()
        .any(|instruction| matches!(instruction.op, Op::JsonPushLocal(_))));
    let mut legacy = artifact.clone();
    legacy.format_version = 32;
    assert_eq!(legacy.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");
    let artifact = Artifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let policy = HostPolicy {
        allocation_limit_bytes: 64 * 1024 * 1024,
        ..HostPolicy::default()
    };
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::from_usize(2501))
    );
}

#[test]
fn float64_square_root_runs_in_native_code_and_checks_domain() {
    let source = "fn hypotenuse(x: Float64, y: Float64) -> Float64:\n    return sqrt(x * x + y * y)\n\nfn invalid_root() -> Float64:\n    return sqrt(-1.0)\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile("float-sqrt.gu", source).expect("Float64 square root compiles");
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    instance.invoke("main", vec![]).unwrap();
    assert!(instance
        .native_function_names()
        .contains(&"hypotenuse".to_owned()));
    assert_eq!(
        instance
            .invoke(
                "hypotenuse",
                vec![
                    Value::Float64(Float64::new(3.0).unwrap()),
                    Value::Float64(Float64::new(4.0).unwrap()),
                ],
            )
            .unwrap(),
        Value::Float64(Float64::new(5.0).unwrap())
    );
    assert_eq!(
        instance.invoke("invalid_root", vec![]).unwrap_err().code,
        "R_MATH_DOMAIN"
    );
}

#[test]
fn immutable_list_updates_reject_wrong_types_and_out_of_range_indices() {
    let mismatched = compile(
        "list-update-type.gu",
        "fn main() -> Int:\n    let values = list_push([1], \"wrong\")\n    return 0\n",
    )
    .unwrap_err();
    assert_eq!(mismatched.code, "E_TYPE_ARGUMENT");

    let out_of_range = compile(
        "list-update-index.gu",
        "fn main() -> Int:\n    let values = list_set([1], 1, 2)\n    return 0\n",
    )
    .unwrap();
    let mut instance = Runtime::new()
        .load(out_of_range, HostPolicy::default())
        .unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap_err().code,
        "R_LIST_INDEX"
    );
}

#[test]
fn local_type_annotation_rejects_a_mismatched_initializer() {
    let source = "fn main() -> Int:\n    let values: List[Int] = [\"wrong\"]\n    return 0\n";
    let error = compile("mismatched-local-annotation.gu", source).unwrap_err();
    assert_eq!(error.code, "E_TYPE_ANNOTATION");
}

#[test]
fn executes_source_through_the_shared_artifact_path() {
    let source = "fn sum_to(n: Int) -> Int:\n    var total = 0\n    var i = n\n    while i > 0:\n        set total = total + i\n        set i = i - 1\n    return total\n\nfn main() -> Int:\n    let result = sum_to(5)\n    print(result)\n    return result\n";
    let artifact = compile("sum.gu", source).expect("source compiles");
    let runtime = Runtime::new();
    let mut instance = runtime
        .load(artifact, HostPolicy::default())
        .expect("artifact loads");
    let result = instance.invoke("main", vec![]).expect("entry runs");
    assert_eq!(result, Value::Int(BigInt::parse("15").unwrap()));
    assert_eq!(instance.take_output(), "15\n");
    instance.close();
}

#[test]
fn args_from_skips_cli_subcommands_and_round_trips_in_current_format() {
    let source = "fn main() -> Int:\n    let values = args_from(1)\n    print(len(values))\n    for value in values:\n        print(value)\n    return 0\n";
    let artifact = compile("args-from.gu", source).expect("args_from source compiles");
    assert_eq!(artifact.format_version, Artifact::CURRENT_VERSION);
    let mut legacy = artifact.clone();
    legacy.format_version = 21;
    assert_eq!(legacy.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");
    let encoded = artifact.to_bytes().unwrap();
    let decoded = guion_model::Artifact::from_bytes(&encoded).unwrap();
    let mut policy = HostPolicy::default();
    policy = policy.with_arguments(vec![
        "refresh".into(),
        "https://one.example/feed".into(),
        "https://two.example/feed".into(),
    ]);
    let mut instance = Runtime::new().load(decoded, policy).unwrap();
    instance.invoke("main", vec![]).unwrap();
    assert_eq!(
        instance.take_output(),
        "2\nhttps://one.example/feed\nhttps://two.example/feed\n"
    );
}

#[test]
fn args_from_rejects_negative_and_past_end_indices() {
    for index in ["-1", "2"] {
        let source = format!(
            "fn main() -> Int:\n    let values = args_from({index})\n    return len(values)\n"
        );
        let artifact = compile("args-from-index.gu", &source).expect("args_from source compiles");
        let policy = HostPolicy::default().with_arguments(vec!["one".into()]);
        let mut instance = Runtime::new().load(artifact, policy).unwrap();
        assert_eq!(
            instance.invoke("main", vec![]).unwrap_err().code,
            "R_ARGUMENT_INDEX"
        );
    }
}

#[test]
fn integer_division_floor_division_and_remainder_follow_the_numeric_contract() {
    let source = "fn main() -> Int:\n    print(-7 / 3)\n    print(-7 // 3)\n    print(-7 % 3)\n    print(7 / -3)\n    print(7 // -3)\n    print(7 % -3)\n    print(123456789012345678901234567890 / 7)\n    print(7.0 / 2.0)\n    return 0\n";
    let artifact = compile("integer-division.gu", source).expect("numeric operators compile");
    let bytes = artifact.to_bytes().unwrap();
    let artifact = guion_model::Artifact::from_bytes(&bytes).unwrap();
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    instance.invoke("main", vec![]).unwrap();
    assert_eq!(
        instance.take_output(),
        "-2\n-3\n2\n-2\n-3\n-2\n17636684144620811271604938270\n3.5\n"
    );
}

#[test]
fn native_constant_division_by_two_uses_compact_code_and_preserves_floor_semantics() {
    let source = "fn floor_half(value: Int) -> Int:\n    return value // 2\n\nfn odd_bit(value: Int) -> Int:\n    return value % 2\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile("native-divide-by-two.gu", source).expect("source compiles");
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .expect("native runtime loads");
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .expect("VM runtime loads");

    for (value, expected_half, expected_bit) in [
        (i64::MIN, "-4611686018427387904", "0"),
        (-9, "-5", "1"),
        (-3, "-2", "1"),
        (-1, "-1", "1"),
        (0, "0", "0"),
        (1, "0", "1"),
        (3, "1", "1"),
        (9, "4", "1"),
        (i64::MAX, "4611686018427387903", "1"),
    ] {
        let argument = || vec![Value::Int(BigInt::from_i64(value))];
        let expected_half = Value::Int(BigInt::parse(expected_half).unwrap());
        let expected_bit = Value::Int(BigInt::parse(expected_bit).unwrap());
        assert_eq!(
            native.invoke("floor_half", argument()).unwrap(),
            expected_half
        );
        assert_eq!(vm.invoke("floor_half", argument()).unwrap(), expected_half);
        assert_eq!(native.invoke("odd_bit", argument()).unwrap(), expected_bit);
        assert_eq!(vm.invoke("odd_bit", argument()).unwrap(), expected_bit);
    }

    let ir_instructions = native.native_cranelift_ir_instructions();
    assert!(
        ir_instructions < 451,
        "constant divide/remainder by two should use compact native code, got {ir_instructions} IR instructions"
    );
}

#[test]
fn native_positive_power_of_two_divisors_use_compact_code_and_preserve_floor_semantics() {
    let source = "fn floor_by_one(value: Int) -> Int:\n    return value // 1\n\nfn remainder_by_one(value: Int) -> Int:\n    return value % 1\n\nfn floor_by_four(value: Int) -> Int:\n    return value // 4\n\nfn remainder_by_eight(value: Int) -> Int:\n    return value % 8\n\nfn floor_by_large(value: Int) -> Int:\n    return value // 4611686018427387904\n\nfn remainder_by_large(value: Int) -> Int:\n    return value % 4611686018427387904\n\nfn floor_by_dynamic(value: Int, divisor: Int) -> Int:\n    return value // divisor\n\nfn remainder_by_dynamic(value: Int, divisor: Int) -> Int:\n    return value % divisor\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile("native-power-of-two-divisors.gu", source).expect("source compiles");
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .expect("native runtime loads");
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .expect("VM runtime loads");

    for (
        value,
        expected_one,
        expected_mod_one,
        expected_four,
        expected_mod_eight,
        expected_large,
        expected_mod_large,
    ) in [
        (
            i64::MIN,
            "-9223372036854775808",
            "0",
            "-2305843009213693952",
            "0",
            "-2",
            "0",
        ),
        (-17, "-17", "0", "-5", "7", "-1", "4611686018427387887"),
        (-9, "-9", "0", "-3", "7", "-1", "4611686018427387895"),
        (-1, "-1", "0", "-1", "7", "-1", "4611686018427387903"),
        (0, "0", "0", "0", "0", "0", "0"),
        (1, "1", "0", "0", "1", "0", "1"),
        (9, "9", "0", "2", "1", "0", "9"),
        (17, "17", "0", "4", "1", "0", "17"),
        (
            i64::MAX,
            "9223372036854775807",
            "0",
            "2305843009213693951",
            "7",
            "1",
            "4611686018427387903",
        ),
    ] {
        let argument = || vec![Value::Int(BigInt::from_i64(value))];
        for (function, expected, divisor) in [
            ("floor_by_one", expected_one, 1),
            ("remainder_by_one", expected_mod_one, 1),
            ("floor_by_four", expected_four, 4),
            ("remainder_by_eight", expected_mod_eight, 8),
            ("floor_by_large", expected_large, 4_611_686_018_427_387_904),
            (
                "remainder_by_large",
                expected_mod_large,
                4_611_686_018_427_387_904,
            ),
        ] {
            let expected = Value::Int(BigInt::parse(expected).unwrap());
            assert_eq!(native.invoke(function, argument()).unwrap(), expected);
            assert_eq!(vm.invoke(function, argument()).unwrap(), expected);
            let dynamic_arguments = || {
                vec![
                    Value::Int(BigInt::from_i64(value)),
                    Value::Int(BigInt::from_i64(divisor)),
                ]
            };
            let dynamic_function = if function.starts_with("floor") {
                "floor_by_dynamic"
            } else {
                "remainder_by_dynamic"
            };
            assert_eq!(
                native
                    .invoke(dynamic_function, dynamic_arguments())
                    .unwrap(),
                expected
            );
            assert_eq!(
                vm.invoke(dynamic_function, dynamic_arguments()).unwrap(),
                expected
            );
        }
    }

    let profile = native.native_compile_profile();
    let functions = profile["functions"]
        .as_array()
        .expect("native profile has functions");
    let ir_instructions = |name: &str| {
        functions
            .iter()
            .find(|function| function["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("missing native profile for {name}"))
            ["cranelift_ir_instructions"]
            .as_u64()
            .unwrap() as usize
    };
    let dynamic_floor_instructions = ir_instructions("floor_by_dynamic");
    for name in ["floor_by_one", "floor_by_four", "floor_by_large"] {
        assert!(
            ir_instructions(name) + 15 < dynamic_floor_instructions,
            "literal {name} should emit at least 16 fewer native instructions than dynamic floor division"
        );
    }
    let dynamic_remainder_instructions = ir_instructions("remainder_by_dynamic");
    for name in [
        "remainder_by_one",
        "remainder_by_eight",
        "remainder_by_large",
    ] {
        assert!(
            ir_instructions(name) + 15 < dynamic_remainder_instructions,
            "literal {name} should emit at least 16 fewer native instructions than dynamic remainder"
        );
    }
}

#[test]
fn native_positive_power_of_two_integer_division_truncates_toward_zero() {
    let source = "fn trunc_by_one(value: Int) -> Int:\n    return value / 1\n\nfn trunc_by_two(value: Int) -> Int:\n    return value / 2\n\nfn trunc_by_four(value: Int) -> Int:\n    return value / 4\n\nfn trunc_by_large(value: Int) -> Int:\n    return value / 4611686018427387904\n\nfn trunc_by_dynamic(value: Int, divisor: Int) -> Int:\n    return value / divisor\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile("native-truncating-power-of-two.gu", source).expect("source compiles");
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .expect("native runtime loads");
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .expect("VM runtime loads");

    for (value, expected_one, expected_two, expected_four, expected_large) in [
        (
            i64::MIN,
            "-9223372036854775808",
            "-4611686018427387904",
            "-2305843009213693952",
            "-2",
        ),
        (-17, "-17", "-8", "-4", "0"),
        (-9, "-9", "-4", "-2", "0"),
        (-5, "-5", "-2", "-1", "0"),
        (-1, "-1", "0", "0", "0"),
        (0, "0", "0", "0", "0"),
        (1, "1", "0", "0", "0"),
        (5, "5", "2", "1", "0"),
        (9, "9", "4", "2", "0"),
        (17, "17", "8", "4", "0"),
        (
            i64::MAX,
            "9223372036854775807",
            "4611686018427387903",
            "2305843009213693951",
            "1",
        ),
    ] {
        let argument = || vec![Value::Int(BigInt::from_i64(value))];
        for (function, expected, divisor) in [
            ("trunc_by_one", expected_one, 1),
            ("trunc_by_two", expected_two, 2),
            ("trunc_by_four", expected_four, 4),
            ("trunc_by_large", expected_large, 4_611_686_018_427_387_904),
        ] {
            let expected = Value::Int(BigInt::parse(expected).unwrap());
            assert_eq!(native.invoke(function, argument()).unwrap(), expected);
            assert_eq!(vm.invoke(function, argument()).unwrap(), expected);
            let dynamic_arguments = || {
                vec![
                    Value::Int(BigInt::from_i64(value)),
                    Value::Int(BigInt::from_i64(divisor)),
                ]
            };
            assert_eq!(
                native
                    .invoke("trunc_by_dynamic", dynamic_arguments())
                    .unwrap(),
                expected
            );
            assert_eq!(
                vm.invoke("trunc_by_dynamic", dynamic_arguments()).unwrap(),
                expected
            );
        }
    }

    let profile = native.native_compile_profile();
    let functions = profile["functions"]
        .as_array()
        .expect("native profile has functions");
    let ir_instructions = |name: &str| {
        functions
            .iter()
            .find(|function| function["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("missing native profile for {name}"))
            ["cranelift_ir_instructions"]
            .as_u64()
            .unwrap() as usize
    };
    let dynamic_instructions = ir_instructions("trunc_by_dynamic");
    for name in [
        "trunc_by_one",
        "trunc_by_two",
        "trunc_by_four",
        "trunc_by_large",
    ] {
        assert!(
            ir_instructions(name) + 15 < dynamic_instructions,
            "literal {name} should emit at least 16 fewer native instructions than dynamic integer division"
        );
    }
}

#[test]
fn native_negative_power_of_two_divisors_preserve_signed_integer_semantics() {
    let source = "fn floor_by_negative_four(value: Int) -> Int:\n    return value // -4\n\nfn floor_by_negative_large(value: Int) -> Int:\n    return value // -4611686018427387904\n\nfn remainder_by_negative_eight(value: Int) -> Int:\n    return value % -8\n\nfn remainder_by_negative_large(value: Int) -> Int:\n    return value % -4611686018427387904\n\nfn trunc_by_negative_four(value: Int) -> Int:\n    return value / -4\n\nfn trunc_by_negative_large(value: Int) -> Int:\n    return value / -4611686018427387904\n\nfn floor_by_negative_one(value: Int) -> Int:\n    return value // -1\n\nfn floor_by_dynamic(value: Int, divisor: Int) -> Int:\n    return value // divisor\n\nfn remainder_by_dynamic(value: Int, divisor: Int) -> Int:\n    return value % divisor\n\nfn trunc_by_dynamic(value: Int, divisor: Int) -> Int:\n    return value / divisor\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile("native-negative-power-of-two.gu", source).expect("source compiles");
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .expect("native runtime loads");
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .expect("VM runtime loads");

    for (
        value,
        expected_floor_four,
        expected_floor_large,
        expected_remainder_eight,
        expected_remainder_large,
        expected_trunc_four,
        expected_trunc_large,
        expected_floor_negative_one,
    ) in [
        (
            i64::MIN,
            "2305843009213693952",
            "2",
            "0",
            "0",
            "2305843009213693952",
            "2",
            "9223372036854775808",
        ),
        (-17, "4", "0", "-1", "-17", "4", "0", "17"),
        (-9, "2", "0", "-1", "-9", "2", "0", "9"),
        (-1, "0", "0", "-1", "-1", "0", "0", "1"),
        (0, "0", "0", "0", "0", "0", "0", "0"),
        (1, "-1", "-1", "-7", "-4611686018427387903", "0", "0", "-1"),
        (5, "-2", "-1", "-3", "-4611686018427387899", "-1", "0", "-5"),
        (
            17,
            "-5",
            "-1",
            "-7",
            "-4611686018427387887",
            "-4",
            "0",
            "-17",
        ),
        (
            i64::MAX,
            "-2305843009213693952",
            "-2",
            "-1",
            "-1",
            "-2305843009213693951",
            "-1",
            "-9223372036854775807",
        ),
    ] {
        let argument = || vec![Value::Int(BigInt::from_i64(value))];
        for (function, expected, divisor, dynamic_function) in [
            (
                "floor_by_negative_four",
                expected_floor_four,
                -4,
                "floor_by_dynamic",
            ),
            (
                "floor_by_negative_large",
                expected_floor_large,
                -4_611_686_018_427_387_904,
                "floor_by_dynamic",
            ),
            (
                "remainder_by_negative_eight",
                expected_remainder_eight,
                -8,
                "remainder_by_dynamic",
            ),
            (
                "remainder_by_negative_large",
                expected_remainder_large,
                -4_611_686_018_427_387_904,
                "remainder_by_dynamic",
            ),
            (
                "trunc_by_negative_four",
                expected_trunc_four,
                -4,
                "trunc_by_dynamic",
            ),
            (
                "trunc_by_negative_large",
                expected_trunc_large,
                -4_611_686_018_427_387_904,
                "trunc_by_dynamic",
            ),
            (
                "floor_by_negative_one",
                expected_floor_negative_one,
                -1,
                "floor_by_dynamic",
            ),
        ] {
            let expected = Value::Int(BigInt::parse(expected).unwrap());
            assert_eq!(native.invoke(function, argument()).unwrap(), expected);
            assert_eq!(vm.invoke(function, argument()).unwrap(), expected);
            let dynamic_arguments = || {
                vec![
                    Value::Int(BigInt::from_i64(value)),
                    Value::Int(BigInt::from_i64(divisor)),
                ]
            };
            assert_eq!(
                native
                    .invoke(dynamic_function, dynamic_arguments())
                    .unwrap(),
                expected
            );
            assert_eq!(
                vm.invoke(dynamic_function, dynamic_arguments()).unwrap(),
                expected
            );
        }
    }
    assert!(native.native_deoptimizations() > 0);

    let profile = native.native_compile_profile();
    let functions = profile["functions"]
        .as_array()
        .expect("native profile has functions");
    let ir_instructions = |name: &str| {
        functions
            .iter()
            .find(|function| function["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("missing native profile for {name}"))
            ["cranelift_ir_instructions"]
            .as_u64()
            .unwrap() as usize
    };
    for (constant, dynamic) in [
        ("floor_by_negative_four", "floor_by_dynamic"),
        ("floor_by_negative_large", "floor_by_dynamic"),
        ("remainder_by_negative_eight", "remainder_by_dynamic"),
        ("remainder_by_negative_large", "remainder_by_dynamic"),
        ("trunc_by_negative_four", "trunc_by_dynamic"),
        ("trunc_by_negative_large", "trunc_by_dynamic"),
    ] {
        assert!(
            ir_instructions(constant) + 7 < ir_instructions(dynamic),
            "literal {constant} should emit at least 8 fewer native instructions than dynamic division ({} vs {})",
            ir_instructions(constant),
            ir_instructions(dynamic)
        );
    }
}

#[test]
fn native_integer_division_overflow_deoptimizes_to_arbitrary_precision_vm() {
    let source = "fn quotient(left: Int, right: Int) -> Int:\n    return left // right\n\nfn main() -> Int:\n    return quotient(-9223372036854775808, -1)\n";
    let artifact = compile("native-int-divide-overflow.gu", source).unwrap();
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .unwrap();

    let native_result = native.invoke("main", vec![]).unwrap();
    assert_eq!(native_result, vm.invoke("main", vec![]).unwrap());
    assert_eq!(
        native_result,
        Value::Int(BigInt::parse("9223372036854775808").unwrap())
    );
    assert!(native.native_deoptimizations() > 0);
}

#[test]
fn integer_division_operators_report_division_by_zero() {
    for operator in ["/", "//", "%"] {
        let source =
            format!("fn main() -> Int:\n    let result = 1 {operator} 0\n    return result\n");
        let artifact = compile("divide-by-zero.gu", &source).expect("integer operator compiles");
        let mut instance = Runtime::new()
            .load(artifact, HostPolicy::default())
            .unwrap();
        assert_eq!(
            instance.invoke("main", vec![]).unwrap_err().code,
            "R_DIV_ZERO"
        );
    }
}

#[test]
fn native_backend_executes_pure_integer_loops_and_falls_back_for_bigints() {
    let source = "fn sum_to(n: Int) -> Int:\n    var total = 0\n    var i = n\n    while i > 0:\n        set total = total + i\n        set i = i - 1\n    return total\n\nfn main() -> Int:\n    return sum_to(5)\n\nfn big() -> Int:\n    return 9223372036854775808\n\nfn increment(value: Int) -> Int:\n    return value + 1\n\nfn quotient(left: Int, right: Int) -> Int:\n    return left / right\n\nfn remainder(left: Int, right: Int) -> Int:\n    return left % right\n";
    let artifact = compile("native-loop.gu", source).expect("source compiles");
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .expect("artifact loads");
    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::parse("15").unwrap())
    );
    assert!(instance.native_function_count() >= 1);
    assert_eq!(
        instance.invoke("big", vec![]).unwrap(),
        Value::Int(BigInt::parse("9223372036854775808").unwrap())
    );
    assert_eq!(
        instance
            .invoke(
                "increment",
                vec![Value::Int(BigInt::parse("9223372036854775807").unwrap())]
            )
            .unwrap(),
        Value::Int(BigInt::parse("9223372036854775808").unwrap())
    );
    assert_eq!(
        instance
            .invoke(
                "quotient",
                vec![
                    Value::Int(BigInt::parse("9").unwrap()),
                    Value::Int(BigInt::parse("0").unwrap()),
                ]
            )
            .unwrap_err()
            .code,
        "R_DIV_ZERO"
    );
    for (left, right, expected) in [
        ("-7", "3", "2"),
        ("7", "-3", "-2"),
        ("-7", "-3", "-1"),
        ("7", "3", "1"),
        ("-9223372036854775808", "-1", "0"),
    ] {
        assert_eq!(
            instance
                .invoke(
                    "remainder",
                    vec![
                        Value::Int(BigInt::parse(left).unwrap()),
                        Value::Int(BigInt::parse(right).unwrap()),
                    ],
                )
                .unwrap(),
            Value::Int(BigInt::parse(expected).unwrap())
        );
    }
}

#[test]
fn json_columns_feed_native_integer_aggregation_with_stable_category_codes() {
    let source = r#"record Analysis:
    total: Int
    counts: List[Int]
    totals: List[Int]

fn zeros(count: Int) -> List[Int]:
    var values: List[Int] = []
    var index = 0
    while index < count:
        set values = list_push(values, 0)
        set index = index + 1
    return values

fn aggregate(values: List[Int], categories: List[Int], initial_counts: List[Int], initial_totals: List[Int]) -> Analysis:
    var counts = list_set(initial_counts, 0, initial_counts[0])
    var totals = list_set(initial_totals, 0, initial_totals[0])
    var total = 0
    var index = 0
    while index < len(values):
        let value = values[index]
        let category = categories[index]
        set counts = list_set(counts, category, counts[category] + 1)
        set totals = list_set(totals, category, totals[category] + value)
        set total = total + value
        set index = index + 1
    return Analysis(total, counts, totals)

fn main() -> Int:
    let document = unwrap(json_parse("{\"rows\":[{\"value\":12,\"tag\":\"b\"},{\"value\":8,\"tag\":\"a\"},{\"value\":21,\"tag\":\"b\"}]}"))
    let rows = json_get(document, "rows")
    let values = json_int_column(rows, "value")
    let categorized = json_text_categories(rows, "tag")
    let labels = json_get(categorized, "labels")
    let categories = json_as_int_list(json_get(categorized, "codes"))
    assert(len(values) == 3)
    assert(json_as_text(json_index(labels, 0)) == "b")
    assert(json_as_text(json_index(labels, 1)) == "a")
    let result = aggregate(values, categories, zeros(json_len(labels)), zeros(json_len(labels)))
    assert(result.total == 41)
    assert(result.counts[0] == 2)
    assert(result.counts[1] == 1)
    assert(result.totals[0] == 33)
    assert(result.totals[1] == 8)
    return 0
"#;
    let artifact = compile("json-column-native.gu", source).expect("source compiles");
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .expect("artifact loads");

    instance
        .invoke("main", vec![])
        .expect("native reduction runs");

    assert!(
        instance
            .native_function_names()
            .iter()
            .any(|name| name == "aggregate"),
        "native functions: {:?}",
        instance.native_function_names()
    );
    assert!(
        instance.native_calls() > 0,
        "aggregation executes through the native backend"
    );
}

#[test]
fn json_column_builtins_preserve_exact_values_order_and_artifact_version() {
    let source = r#"fn main() -> Int:
    let rows = unwrap(json_parse("[{\"n\":9007199254740993,\"kind\":\"é\"},{\"n\":-7,\"kind\":\"a\"},{\"n\":12,\"kind\":\"é\"}]"))
    let numbers = json_int_column(rows, "n")
    let categories = json_text_categories(rows, "kind")
    let codes = json_as_int_list(json_get(categories, "codes"))
    let labels = json_get(categories, "labels")
    assert(numbers[0] == 9007199254740993)
    assert(numbers[1] == -7)
    assert(codes[0] == 0)
    assert(codes[1] == 1)
    assert(codes[2] == 0)
    assert(json_as_text(json_index(labels, 0)) == "é")
    assert(json_as_text(json_index(labels, 1)) == "a")
    return 0
"#;
    let artifact = compile("json-column-artifact.gu", source).unwrap();
    assert_eq!(artifact.format_version, Artifact::CURRENT_VERSION);
    let decoded = Artifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    assert_eq!(decoded.to_bytes().unwrap(), artifact.to_bytes().unwrap());
    let mut legacy = artifact.clone();
    legacy.format_version = 28;
    assert_eq!(legacy.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");

    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    instance.invoke("main", vec![]).unwrap();
}

#[test]
fn json_column_builtins_reject_wrong_shapes_and_non_integer_values() {
    for (source, expected_code) in [
        (
            "fn main() -> Int:\n    let rows = unwrap(json_parse(\"[{\\\"n\\\":1.5}]\"))\n    return len(json_int_column(rows, \"n\"))\n",
            "R_JSON_TYPE",
        ),
        (
            "fn main() -> Int:\n    let values = unwrap(json_parse(\"[1, 2.5]\"))\n    return len(json_as_int_list(values))\n",
            "R_JSON_TYPE",
        ),
        (
            "fn main() -> Int:\n    let rows = unwrap(json_parse(\"[{\\\"tag\\\":true}]\"))\n    return json_len(json_text_categories(rows, \"tag\"))\n",
            "R_JSON_TYPE",
        ),
    ] {
        let artifact = compile("json-column-invalid.gu", source).unwrap();
        let mut instance = Runtime::new()
            .load(artifact, HostPolicy::default())
            .unwrap();
        assert_eq!(instance.invoke("main", vec![]).unwrap_err().code, expected_code);
    }
}

#[test]
fn json_projection_builds_only_requested_columns_and_preserves_duplicate_checks() {
    let source = r#"fn main() -> Int:
    let raw = "{\"meta\":{\"ignored\":[1,2]},\"payload\":{\"rows\":[{\"n\":9007199254740993,\"kind\":\"東京\",\"unused\":{\"$serde_json::private::Number\":{\"nested\":1}}},{\"n\":-7,\"kind\":\"é\",\"unused\":null},{\"n\":12,\"kind\":\"東京\"}]}}"
    let columns = unwrap(json_project_columns(raw, ["payload", "rows"], ["n", "kind"]))
    let numbers = json_as_int_list(json_get(columns, "n"))
    let categories = json_text_categories_values(json_get(columns, "kind"))
    let codes = json_as_int_list(json_get(categories, "codes"))
    let labels = json_get(categories, "labels")
    assert(numbers[0] == 9007199254740993)
    assert(numbers[1] == -7)
    assert(codes[0] == 0 and codes[1] == 1 and codes[2] == 0)
    assert(json_as_text(json_index(labels, 0)) == "東京")
    assert(json_as_text(json_index(labels, 1)) == "é")
    return 0
"#;
    let artifact = compile("json-project-columns.gu", source).unwrap();
    assert_eq!(artifact.format_version, Artifact::CURRENT_VERSION);
    let artifact = guion_model::Artifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    instance.invoke("main", vec![]).unwrap();

    let duplicate = r#"fn main() -> Int:
    let result = json_project_columns("{\"rows\":[{\"n\":1,\"ignored\":{\"x\":1,\"x\":2}}]}", ["rows"], ["n"])
    assert(not is_ok(result))
    assert(json_error_code(error(result)) == "R_JSON_DUPLICATE_KEY")
    return 0
"#;
    let artifact = compile("json-project-duplicate.gu", duplicate).unwrap();
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    instance.invoke("main", vec![]).unwrap();
}

#[test]
fn json_projection_reports_shape_and_field_errors_as_results() {
    let source = r#"fn main() -> Int:
    let missing_path = json_project_columns("{}", ["rows"], ["n"])
    assert(not is_ok(missing_path))
    assert(json_error_code(error(missing_path)) == "R_JSON_FIELD")
    let wrong_rows = json_project_columns("{\"rows\":{}}", ["rows"], ["n"])
    assert(not is_ok(wrong_rows))
    assert(json_error_code(error(wrong_rows)) == "R_JSON_TYPE")
    let wrong_row = json_project_columns("{\"rows\":[1]}", ["rows"], ["n"])
    assert(not is_ok(wrong_row))
    assert(json_error_code(error(wrong_row)) == "R_JSON_TYPE")
    let missing_field = json_project_columns("{\"rows\":[{}]}", ["rows"], ["n"])
    assert(not is_ok(missing_field))
    assert(json_error_code(error(missing_field)) == "R_JSON_FIELD")
    let repeated_field = json_project_columns("{\"rows\":[]}", ["rows"], ["n", "n"])
    assert(not is_ok(repeated_field))
    assert(json_error_code(error(repeated_field)) == "R_JSON_FIELD")
    return 0
"#;
    let artifact = compile("json-project-shapes.gu", source).unwrap();
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    instance.invoke("main", vec![]).unwrap();
}

#[test]
fn json_projection_preserves_reserved_looking_user_keys_and_numeric_text() {
    let source = r#"fn main() -> Int:
    let projected = json_project_columns("{\"rows\":[{\"special\":{\"$serde_json::private::Number\":\"123\"}}]}", ["rows"], ["special"])
    assert(is_ok(projected))
    let row = json_index(json_get(unwrap(projected), "special"), 0)
    assert(json_as_text(json_get(row, "$serde_json::private::Number")) == "123")
    return 0
"#;
    let artifact = compile("json-project-reserved-key.gu", source).unwrap();
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    instance.invoke("main", vec![]).unwrap();
}

#[test]
fn json_projection_enforces_limits_inside_ignored_data() {
    let source = r#"fn main() -> Int:
    let result = json_project_columns("{\"rows\":[{\"n\":1,\"ignored\":12345}]}", ["rows"], ["n"])
    assert(not is_ok(result))
    assert(json_error_code(error(result)) == "R_INTEGER_LIMIT")
    return 0
"#;
    let artifact = compile("json-project-integer-limit.gu", source).unwrap();
    let policy = HostPolicy {
        max_integer_digits: 4,
        ..HostPolicy::default()
    };
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    instance.invoke("main", vec![]).unwrap();

    let source = r#"fn main() -> Int:
    let result = json_project_columns("{\"rows\":[{\"n\":1}]}", ["rows"], ["n"])
    assert(not is_ok(result))
    assert(json_error_code(error(result)) == "R_COLLECTION_LIMIT")
    return 0
"#;
    let artifact = compile("json-project-item-limit.gu", source).unwrap();
    let policy = HostPolicy {
        max_collection_items: 2,
        ..HostPolicy::default()
    };
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    instance.invoke("main", vec![]).unwrap();
}

#[test]
fn native_backend_executes_float64_loop_and_preserves_float_faults() {
    let source = "fn escape_steps(cx: Float64, cy: Float64, limit: Int) -> Int:\n    var x = 0.0\n    var y = 0.0\n    var steps = 0\n    while steps < limit and x * x + y * y <= 4.0:\n        let next_x = x * x - y * y + cx\n        set y = 2.0 * x * y + cy\n        set x = next_x\n        set steps = steps + 1\n    return steps\n\nfn quotient(left: Float64, right: Float64) -> Float64:\n    return left / right\n\nfn overflow() -> Float64:\n    return 1.0e308 * 1.0e308\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile("native-float-loop.gu", source).expect("source compiles");
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .expect("artifact loads");
    instance.invoke("main", vec![]).unwrap();
    let native_names = instance.native_function_names();
    assert!(native_names.contains(&"escape_steps".to_owned()));
    assert!(native_names.contains(&"quotient".to_owned()));
    assert!(native_names.contains(&"overflow".to_owned()));
    let result = instance
        .invoke(
            "escape_steps",
            vec![
                Value::Float64(Float64::new(0.0).unwrap()),
                Value::Float64(Float64::new(0.0).unwrap()),
                Value::Int(BigInt::parse("80").unwrap()),
            ],
        )
        .unwrap();
    assert_eq!(result, Value::Int(BigInt::parse("80").unwrap()));
    assert_eq!(
        instance
            .invoke(
                "quotient",
                vec![
                    Value::Float64(Float64::new(1.0).unwrap()),
                    Value::Float64(Float64::new(-0.0).unwrap()),
                ],
            )
            .unwrap_err()
            .code,
        "R_DIV_ZERO"
    );
    assert_eq!(
        instance.invoke("overflow", vec![]).unwrap_err().code,
        "R_FLOAT_NONFINITE"
    );
    assert!(instance.native_calls() >= 3);
}

#[test]
fn native_backend_reads_float64_lists_with_vm_parity() {
    let source = "fn sum(values: List[Float64]) -> Float64:\n    var total = 0.0\n    var i = 0\n    while i < len(values):\n        set total = total + values[i]\n        set i = i + 1\n    return total\n\nfn main() -> Int:\n    assert(sum([1.25, 2.5, 3.75]) == 7.5)\n    return 0\n";
    let artifact = compile("native-float-list.gu", source).unwrap();
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .unwrap();

    let native_value = native.invoke("main", vec![]).unwrap();
    let vm_value = vm.invoke("main", vec![]).unwrap();

    assert_eq!(native_value, vm_value);
    assert_eq!(native_value, Value::Int(BigInt::zero()));
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "sum"));
    assert!(native.native_calls() > 0);
}

#[test]
fn native_backend_reads_integer_lists_and_preserves_big_integer_fallback() {
    let source = "fn sum(values: List[Int]) -> Int:\n    var total = 0\n    var i = 0\n    while i < len(values):\n        set total = total + values[i]\n        set i = i + 1\n    return total\n\nfn main() -> Int:\n    assert(sum([1, 2, 3]) == 6)\n    assert(sum([9223372036854775808, 2]) == 9223372036854775810)\n    return 0\n";
    let artifact = compile("native-int-list.gu", source).unwrap();
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .unwrap();

    assert_eq!(
        native.invoke("main", vec![]).unwrap(),
        vm.invoke("main", vec![]).unwrap()
    );
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "sum"));
    assert!(native.native_calls() > 0);
}

#[test]
fn native_backend_keeps_integer_record_list_updates_in_a_repeated_loop() {
    let source = "record Queue:\n    values: List[Int]\n\nfn run_steps(queue: Queue, steps: Int) -> Queue:\n    var values = queue.values\n    var index = 0\n    while index < steps:\n        let slot = index % len(values)\n        set values = list_set(values, slot, values[slot] + 1)\n        set index = index + 1\n    return Queue(values)\n\nfn main() -> Int:\n    let original = Queue([0, 0, 0])\n    let changed = run_steps(original, 3000)\n    assert(original.values[0] == 0)\n    assert(changed.values[0] == 1000)\n    assert(changed.values[1] == 1000)\n    assert(changed.values[2] == 1000)\n    return 0\n";
    let artifact = compile("native-int-record-list-loop.gu", source).unwrap();
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .unwrap();

    assert_eq!(
        native.invoke("main", vec![]).unwrap(),
        vm.invoke("main", vec![]).unwrap()
    );
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "run_steps"));
}

#[test]
fn native_index_bounds_use_one_unsigned_check_and_preserve_faults() {
    let source = "fn at(index: Int) -> Int:\n    let values = [7, 8]\n    return values[index]\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile("native-index-bounds.gu", source).expect("index source compiles");
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .expect("native artifact loads");
    let value = native
        .invoke("at", vec![Value::Int(BigInt::from_usize(1))])
        .expect("valid index runs natively");
    assert_eq!(value, Value::Int(BigInt::from_usize(8)));
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "at"));
    let ir_blocks = native.native_cranelift_ir_blocks();
    assert!(
        ir_blocks < 25,
        "a native list index should use one unsigned bounds check: {ir_blocks} IR blocks"
    );

    for index in [-1, 2] {
        let args = vec![Value::Int(BigInt::from_i64(index))];
        let native_error = native
            .invoke("at", args.clone())
            .expect_err("invalid list index fails");
        let mut vm = Runtime::vm_only()
            .load(artifact.clone(), HostPolicy::default())
            .expect("VM artifact loads");
        let vm_error = vm
            .invoke("at", args)
            .expect_err("VM rejects the same invalid index");
        assert_eq!(native_error, vm_error);
    }
}

#[test]
fn native_integer_list_bounds_fault_deopts_to_the_vm() {
    let source = "record Queue:\n    values: List[Int]\n\nfn update(queue: Queue) -> Queue:\n    var values = queue.values\n    set values = list_set(values, 4, 9)\n    return Queue(values)\n\nfn main() -> Int:\n    let queue = Queue([1, 2])\n    let changed = update(queue)\n    return 0\n";
    let artifact = compile("native-int-list-bounds.gu", source).unwrap();
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .unwrap();

    let native_error = native.invoke("main", vec![]).unwrap_err();
    let vm_error = vm.invoke("main", vec![]).unwrap_err();

    assert_eq!(native_error.code, vm_error.code);
    assert_eq!(native_error.code, "R_LIST_INDEX");
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "update"));
    assert!(native.native_deoptimizations() > 0);
}

#[test]
fn native_backend_updates_boolean_lists_in_record_results() {
    let source = "record Flags:\n    active: Bool\n    pending: List[Bool]\n\nfn update(flags: Flags) -> Flags:\n    let active = not flags.active\n    var pending = flags.pending\n    set pending = list_set(pending, 1, not pending[1])\n    return Flags(active, pending)\n\nfn main() -> Int:\n    let original = Flags(true, [true, false, true])\n    let changed = update(original)\n    assert(original.active == true)\n    assert(original.pending[1] == false)\n    assert(changed.active == false)\n    assert(changed.pending[1] == true)\n    return 0\n";
    let artifact = compile("native-bool-record-list.gu", source).unwrap();
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .unwrap();

    assert_eq!(
        native.invoke("main", vec![]).unwrap(),
        vm.invoke("main", vec![]).unwrap()
    );
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "update"));
}

#[test]
fn native_backend_constructs_typed_lists_and_updates_them_in_a_loop() {
    let source = "fn main() -> Int:\n    var values = [1, 2, 3]\n    var index = 0\n    while index < 300:\n        let slot = index % len(values)\n        set values = list_set(values, slot, values[slot] + 1)\n        set index = index + 1\n    return values[0] + values[1] + values[2]\n";
    let artifact = compile("native-list-construction-loop.gu", source).unwrap();
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .unwrap();

    let native_value = native.invoke("main", vec![]).unwrap();
    assert_eq!(native_value, vm.invoke("main", vec![]).unwrap());
    assert_eq!(native_value, Value::Int(BigInt::parse("306").unwrap()));
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "main"));
}

#[test]
fn native_backend_preserves_aliases_for_functional_list_updates() {
    let source = "record Frame:\n    values: List[Int]\n\nfn replace(frame: Frame) -> Frame:\n    let changed = list_set(frame.values, 1, 99)\n    return Frame(changed)\n\nfn main() -> Int:\n    let original = Frame([1, 2, 3])\n    let changed = replace(original)\n    assert(original.values[1] == 2)\n    assert(changed.values[1] == 99)\n    return 0\n";
    let artifact = compile("native-functional-list-set.gu", source).unwrap();
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .unwrap();

    assert_eq!(
        native.invoke("main", vec![]).unwrap(),
        vm.invoke("main", vec![]).unwrap()
    );
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "replace"));
}

#[test]
fn native_backend_composes_record_valued_calls_with_list_fields() {
    let source = "record Frame:\n    values: List[Int]\n    total: Int\n\nfn replace(frame: Frame) -> Frame:\n    let changed = list_set(frame.values, 1, 99)\n    return Frame(changed, frame.total + 1)\n\nfn inspect(frame: Frame) -> Int:\n    return frame.values[1] + frame.total\n\nfn main() -> Int:\n    let original = Frame([1, 2, 3], 4)\n    let changed = replace(original)\n    return inspect(changed)\n";
    let artifact = compile("native-record-call-composition.gu", source).unwrap();
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .unwrap();

    let native_result = native.invoke("main", vec![]).unwrap();
    assert_eq!(native_result, vm.invoke("main", vec![]).unwrap());
    assert_eq!(native_result, Value::Int(BigInt::parse("104").unwrap()));
    for name in ["replace", "inspect", "main"] {
        assert!(native
            .native_function_names()
            .iter()
            .any(|native| native == name));
    }
}

#[test]
fn native_list_local_update_keeps_later_record_reads_aliased_correctly() {
    let source = "record Frame:\n    values: List[Int]\n\nfn update(frame: Frame) -> Int:\n    var values = frame.values\n    set values = list_set(values, 1, 99)\n    return frame.values[1] + values[1]\n\nfn main() -> Int:\n    return update(Frame([1, 2, 3]))\n";
    let artifact = compile("native-list-update-record-alias.gu", source).unwrap();
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .unwrap();

    let native_result = native.invoke("main", vec![]).unwrap();
    assert_eq!(native_result, vm.invoke("main", vec![]).unwrap());
    assert_eq!(native_result, Value::Int(BigInt::parse("101").unwrap()));
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "update"));
}

#[test]
fn native_print_preserves_output_effects_and_falls_back_without_partial_output() {
    let source = "fn main() -> Int:\n    print(42)\n    print(true)\n    return 0\n";
    let artifact = compile("native-print.gu", source).unwrap();
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();

    assert_eq!(
        native.invoke("main", vec![]).unwrap(),
        vm.invoke("main", vec![]).unwrap()
    );
    assert_eq!(native.take_output(), vm.take_output());
    assert_eq!(native.effect_report(), vm.effect_report());
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "main"));

    let limited = HostPolicy {
        output_limit_bytes: 2,
        ..HostPolicy::default()
    };
    let mut native = Runtime::new()
        .load(artifact.clone(), limited.clone())
        .unwrap();
    let mut vm = Runtime::vm_only().load(artifact, limited).unwrap();
    let native_error = native.invoke("main", vec![]).unwrap_err();
    let vm_error = vm.invoke("main", vec![]).unwrap_err();
    assert_eq!(native_error.code, vm_error.code);
    assert_eq!(native_error.code, "R_OUTPUT_LIMIT");
    assert_eq!(native.take_output(), "");
}

#[test]
fn native_functional_list_updates_match_vm_allocation_limit_outcomes() {
    let source = "record Frame:\n    values: List[Int]\n\nfn replace(frame: Frame) -> Frame:\n    let changed = list_set(frame.values, 1, 99)\n    return Frame(changed)\n\nfn main() -> Int:\n    let original = Frame([1, 2, 3])\n    let changed = replace(original)\n    return changed.values[1]\n";
    let artifact = compile("native-functional-list-budget.gu", source).unwrap();

    for allocation_limit_bytes in [0, 64, 128, 192, 256, 384, 512, 1024, 4096] {
        let policy = HostPolicy {
            allocation_limit_bytes,
            ..HostPolicy::default()
        };
        let mut native = Runtime::new()
            .load(artifact.clone(), policy.clone())
            .unwrap();
        let mut vm = Runtime::vm_only().load(artifact.clone(), policy).unwrap();
        let native_result = native.invoke("main", vec![]);
        let vm_result = vm.invoke("main", vec![]);
        match (native_result, vm_result) {
            (Ok(native), Ok(vm)) => assert_eq!(native, vm),
            (Err(native), Err(vm)) => assert_eq!(native.code, vm.code),
            (native, vm) => panic!(
                "native/VM allocation outcome differs at {allocation_limit_bytes} bytes: {native:?} vs {vm:?}"
            ),
        }
        if allocation_limit_bytes == 4096 {
            assert!(native
                .native_function_names()
                .iter()
                .any(|name| name == "replace"));
        }
    }
}

#[test]
fn native_backend_reads_float64_lists_from_record_fields() {
    let source = "record Frame:\n    values: List[Float64]\n\nfn sum(frame: Frame) -> Float64:\n    let values = frame.values\n    var total = 0.0\n    var i = 0\n    while i < len(values):\n        set total = total + values[i]\n        set i = i + 1\n    return total\n\nfn main() -> Int:\n    assert(sum(Frame([1.25, 2.5, 3.75])) == 7.5)\n    return 0\n";
    let artifact = compile("native-record-float-list.gu", source).unwrap();
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .unwrap();

    assert_eq!(
        native.invoke("main", vec![]).unwrap(),
        vm.invoke("main", vec![]).unwrap()
    );
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "sum"));
}

#[test]
fn native_backend_updates_copied_record_lists_and_returns_records() {
    let source = "record Frame:\n    values: List[Float64]\n\nfn update(frame: Frame) -> Frame:\n    var values = frame.values\n    set values = list_set(values, 0, values[0] + 1.0)\n    return Frame(values)\n\nfn main() -> Int:\n    let original = Frame([1.0, 2.0])\n    let updated = update(original)\n    assert(original.values[0] == 1.0)\n    assert(updated.values[0] == 2.0)\n    return 0\n";
    let artifact = compile("native-record-list-update.gu", source).unwrap();
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .unwrap();

    assert_eq!(
        native.invoke("main", vec![]).unwrap(),
        vm.invoke("main", vec![]).unwrap()
    );
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "update"));
    assert!(native.native_calls() > 0);
}

#[test]
fn native_backend_keeps_record_list_updates_inside_a_repeated_native_loop() {
    let source = "record Frame:\n    values: List[Float64]\n\nfn run_steps(frame: Frame, steps: Int, delta: Float64) -> Frame:\n    var values = frame.values\n    var index = 0\n    while index < steps:\n        set values = list_set(values, 0, values[0] + delta)\n        set index = index + 1\n    return Frame(values)\n\nfn main() -> Int:\n    let original = Frame([1.0, 2.0])\n    let changed = run_steps(original, 3, 0.5)\n    assert(original.values[0] == 1.0)\n    assert(changed.values[0] == 2.5)\n    return 0\n";
    let artifact = compile("native-record-list-loop.gu", source).unwrap();
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .unwrap();

    assert_eq!(
        native.invoke("main", vec![]).unwrap(),
        vm.invoke("main", vec![]).unwrap()
    );
    let native_names = native.native_function_names();
    assert!(native_names.iter().any(|name| name == "run_steps"));
    assert!(native.native_calls() > 0);
}

#[test]
fn native_record_list_updates_match_vm_allocation_limit_outcomes() {
    let source = "record Frame:\n    values: List[Float64]\n\nfn update(frame: Frame) -> Frame:\n    var values = frame.values\n    set values = list_set(values, 0, values[0] + 1.0)\n    return Frame(values)\n\nfn main() -> Int:\n    let original = Frame([1.0, 2.0, 3.0, 4.0])\n    let updated = update(original)\n    assert(original.values[0] == 1.0)\n    assert(updated.values[0] == 2.0)\n    return 0\n";
    let artifact = compile("native-record-list-budget.gu", source).unwrap();

    for allocation_limit_bytes in [0, 64, 128, 256, 512, 1024, 2048, 4096] {
        let policy = HostPolicy {
            allocation_limit_bytes,
            ..HostPolicy::default()
        };
        let mut native = Runtime::new()
            .load(artifact.clone(), policy.clone())
            .unwrap();
        let mut vm = Runtime::vm_only().load(artifact.clone(), policy).unwrap();
        let native_result = native.invoke("main", vec![]);
        let vm_result = vm.invoke("main", vec![]);
        match (native_result, vm_result) {
            (Ok(native), Ok(vm)) => assert_eq!(native, vm),
            (Err(native), Err(vm)) => assert_eq!(native.code, vm.code),
            (native, vm) => panic!(
                "native/VM allocation outcome differs at {allocation_limit_bytes} bytes: {native:?} vs {vm:?}"
            ),
        }
        if allocation_limit_bytes == 4096 {
            assert!(native
                .native_function_names()
                .iter()
                .any(|name| name == "update"));
        }
    }
}

#[test]
fn native_record_list_update_bounds_fault_deopts_to_the_vm() {
    let source = "record Frame:\n    values: List[Float64]\n\nfn update(frame: Frame) -> Frame:\n    var values = frame.values\n    set values = list_set(values, 3, 9.0)\n    return Frame(values)\n\nfn main() -> Int:\n    let frame = Frame([1.0])\n    let updated = update(frame)\n    return 0\n";
    let artifact = compile("native-record-list-bounds.gu", source).unwrap();
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();

    let error = instance.invoke("main", vec![]).unwrap_err();

    assert_eq!(error.code, "R_LIST_INDEX");
    assert!(instance
        .native_function_names()
        .iter()
        .any(|name| name == "update"));
    assert!(instance.native_deoptimizations() > 0);
}

#[test]
fn float64_lists_keep_immutable_updates_and_aliases() {
    let source = "fn main() -> Int:\n    let original = [1.0, 2.0]\n    let pushed = list_push(original, 3.0)\n    let replaced = list_set(pushed, 0, 4.0)\n    var local = [5.0, 6.0]\n    let alias = local\n    set local = list_set(local, 1, 7.0)\n    assert(original[0] == 1.0)\n    assert(pushed[0] == 1.0)\n    assert(replaced[0] == 4.0)\n    assert(alias[1] == 6.0)\n    assert(local[1] == 7.0)\n    return 0\n";
    let artifact = compile("float64-list-updates.gu", source).unwrap();
    let artifact = guion_model::Artifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();

    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::zero())
    );
}

#[test]
fn native_float_list_bounds_faults_deopt_to_the_vm_trace() {
    let source = "fn at(values: List[Float64], index: Int) -> Float64:\n    return values[index]\n\nfn main() -> Int:\n    let value = at([1.0], 2)\n    return 0\n";
    let artifact = compile("native-float-list-bounds.gu", source).unwrap();
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();

    let error = instance.invoke("main", vec![]).unwrap_err();

    assert_eq!(error.code, "R_INDEX_BOUNDS");
    assert!(instance
        .native_function_names()
        .iter()
        .any(|name| name == "at"));
    assert!(instance.native_deoptimizations() > 0);
}

#[test]
fn native_backend_charges_each_guest_instruction_exactly() {
    let source = "fn value() -> Int:\n    return 7\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile("native-fuel.gu", source).expect("source compiles");
    let mut limited = Runtime::new()
        .load(
            artifact.clone(),
            HostPolicy {
                instruction_limit: 1,
                ..HostPolicy::default()
            },
        )
        .expect("artifact loads");
    assert_eq!(
        limited.invoke("value", vec![]).unwrap_err().code,
        "R_STEP_LIMIT"
    );

    let mut exact = Runtime::new()
        .load(
            artifact,
            HostPolicy {
                instruction_limit: 2,
                ..HostPolicy::default()
            },
        )
        .expect("artifact loads");
    assert_eq!(
        exact.invoke("value", vec![]).unwrap(),
        Value::Int(BigInt::parse("7").unwrap())
    );
}

#[test]
fn native_fuel_metering_adds_control_flow_per_bytecode_block() {
    let mut source = String::from("fn main() -> Int:\n    let value_0 = 0\n");
    for index in 1..=80 {
        source.push_str(&format!(
            "    let value_{index} = value_{} + 1\n",
            index - 1
        ));
    }
    source.push_str("    return value_80\n");
    let artifact = compile("native-metering-blocks.gu", &source).expect("source compiles");
    let mut instance = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .expect("artifact loads");

    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::parse("80").unwrap())
    );
    let instructions = instance.native_bytecode_instructions();
    let ir_blocks = instance.native_cranelift_ir_blocks();
    let ir_instructions = instance.native_cranelift_ir_instructions();
    assert!(
        ir_blocks < instructions * 2,
        "fuel control flow should scale by bytecode block, not instruction: {ir_blocks} IR blocks for {instructions} instructions"
    );
    assert!(
        ir_instructions < 1_200,
        "step and fuel bookkeeping should batch updates at bytecode boundaries: {ir_instructions} IR instructions for {instructions} guest instructions"
    );

    let exact_policy = HostPolicy {
        instruction_limit: instructions as u64,
        ..HostPolicy::default()
    };
    let mut exact = Runtime::new()
        .load(artifact.clone(), exact_policy.clone())
        .expect("artifact loads with exact fuel");
    assert_eq!(
        exact.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::parse("80").unwrap())
    );

    let limited_policy = HostPolicy {
        instruction_limit: instructions as u64 - 1,
        ..HostPolicy::default()
    };
    let native_error = Runtime::new()
        .load(artifact.clone(), limited_policy.clone())
        .expect("native artifact loads")
        .invoke("main", vec![])
        .unwrap_err();
    let vm_error = Runtime::vm_only()
        .load(artifact, limited_policy)
        .expect("VM artifact loads")
        .invoke("main", vec![])
        .unwrap_err();
    assert_eq!(native_error, vm_error);
}

#[test]
fn environment_access_requires_a_grant_and_preserves_unicode_values() {
    let key = format!("GUION_TEST_SECRET_{}", std::process::id());
    unsafe { std::env::set_var(&key, "café🔑") };
    let source = format!(
        "fn read_secret() -> Option[Text]:\n    return env({key:?})\n\nfn main() -> Int:\n    return 0\n"
    );
    let artifact = compile("env-grant.gu", &source).expect("env should be a typed host operation");
    let denied = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap()
        .invoke("read_secret", vec![])
        .unwrap_err();
    assert_eq!(denied.code, "R_ENV_DENIED");

    let mut policy = HostPolicy::default();
    policy.allowed_environment_variables.insert(key.clone());
    let value = Runtime::new()
        .load(artifact, policy)
        .unwrap()
        .invoke("read_secret", vec![])
        .unwrap();
    assert_eq!(
        value,
        Value::OptionSome(Box::new(Value::Text("café🔑".into())))
    );
    unsafe { std::env::remove_var(key) };
}

#[test]
fn clock_access_requires_an_explicit_grant_and_can_be_pinned() {
    let source = "record Date:\n    year: Int\n    month: Int\n    day: Int\n\nfn read_time() -> Int:\n    return unix_time_ms()\n\nfn read_date() -> Date:\n    return time_today_local()\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile("clock-grant.gu", source).expect("clock operations compile");
    let mut legacy = artifact.clone();
    legacy.format_version = 38;
    assert_eq!(legacy.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");

    let mut denied = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .expect("artifact loads without a clock grant");
    assert_eq!(
        denied.invoke("read_time", vec![]).unwrap_err().code,
        "R_CLOCK_DENIED"
    );
    assert_eq!(
        denied.invoke("read_date", vec![]).unwrap_err().code,
        "R_CLOCK_DENIED"
    );

    let mut granted = Runtime::new()
        .load(
            artifact,
            HostPolicy::default().with_clock_unix_time_ms(1_579_089_600_000),
        )
        .expect("fixed clock grants access");
    assert_eq!(
        granted.invoke("read_time", vec![]).unwrap(),
        Value::Int(BigInt::from_i64(1_579_089_600_000))
    );
    let timestamp = libc::time_t::try_from(1_579_089_600_i64).unwrap();
    let mut expected_local = unsafe { std::mem::zeroed::<libc::tm>() };
    assert!(!unsafe { libc::localtime_r(&timestamp, &mut expected_local) }.is_null());
    let Value::Record(name, fields) = granted.invoke("read_date", vec![]).unwrap() else {
        panic!("local date operation should return Date")
    };
    assert_eq!(name, "Date");
    assert_eq!(fields.len(), 3);
    assert_eq!(
        fields[0],
        Value::Int(BigInt::from_i64(i64::from(expected_local.tm_year + 1900)))
    );
    assert_eq!(
        fields[1],
        Value::Int(BigInt::from_i64(i64::from(expected_local.tm_mon + 1)))
    );
    assert_eq!(
        fields[2],
        Value::Int(BigInt::from_i64(i64::from(expected_local.tm_mday)))
    );
    assert!(granted
        .effect_report()
        .iter()
        .any(|effect| effect.kind == EffectKind::ClockRead));
}

#[test]
fn native_nested_failure_keeps_every_call_site_in_the_runtime_trace() {
    let source = "fn leaf(value: Int) -> Int:\n    return value + 1\n\nfn middle(value: Int) -> Int:\n    return leaf(value)\n\nfn main() -> Int:\n    return middle(1)\n";
    let artifact = compile("native-trace.gu", source).expect("source compiles");
    let policy = HostPolicy {
        call_depth_limit: 2,
        ..HostPolicy::default()
    };
    let mut instance = Runtime::new()
        .load(artifact, policy)
        .expect("artifact loads");

    let error = instance
        .invoke("main", vec![])
        .expect_err("nested call exceeds the depth limit");

    assert!(instance
        .native_function_names()
        .contains(&"leaf".to_owned()));
    assert!(instance
        .native_function_names()
        .contains(&"middle".to_owned()));
    assert!(instance
        .native_function_names()
        .contains(&"main".to_owned()));
    assert_eq!(error.code, "R_CALL_DEPTH");
    assert_eq!(
        error.trace.iter().map(|span| span.line).collect::<Vec<_>>(),
        [2, 5, 8]
    );
}

#[test]
fn native_backend_calls_native_functions_and_obeys_call_depth_limit() {
    let source = "fn increment(value: Int) -> Int:\n    return value + 1\n\nfn outer(value: Int) -> Int:\n    return increment(value)\n\nfn sum_calls(count: Int) -> Int:\n    var index = 0\n    var total = 0\n    while index < count:\n        set total = total + increment(index)\n        set index = index + 1\n    return total\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile("native-call.gu", source).expect("source compiles");
    let mut instance = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .expect("artifact loads");
    instance.invoke("main", vec![]).unwrap();
    assert!(instance
        .native_function_names()
        .contains(&"sum_calls".to_owned()));
    assert!(instance
        .native_function_names()
        .contains(&"increment".to_owned()));
    assert!(instance
        .native_function_names()
        .contains(&"outer".to_owned()));
    assert_eq!(
        instance
            .invoke("sum_calls", vec![Value::Int(BigInt::parse("5").unwrap())])
            .unwrap(),
        Value::Int(BigInt::parse("15").unwrap())
    );
    assert_eq!(
        instance
            .invoke(
                "outer",
                vec![Value::Int(BigInt::parse("9223372036854775807").unwrap())]
            )
            .unwrap(),
        Value::Int(BigInt::parse("9223372036854775808").unwrap())
    );
    assert_eq!(instance.native_calls(), 10);

    let mut shallow = Runtime::new()
        .load(
            artifact,
            HostPolicy {
                call_depth_limit: 1,
                ..HostPolicy::default()
            },
        )
        .expect("artifact loads");
    assert_eq!(
        shallow
            .invoke("sum_calls", vec![Value::Int(BigInt::parse("1").unwrap())])
            .unwrap_err()
            .code,
        "R_CALL_DEPTH"
    );
}

#[test]
fn vm_guest_recursion_uses_heap_frames_and_enforces_one_beyond_depth() {
    let source = "fn descend(remaining: Int) -> Int:\n    if remaining <= 0:\n        return 0\n    return descend(remaining - 1) + 1\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile("vm-guest-frames.gu", source).expect("recursive source compiles");
    let policy = HostPolicy {
        call_depth_limit: 512,
        ..HostPolicy::default()
    };
    let mut vm = Runtime::vm_only()
        .load(artifact.clone(), policy.clone())
        .expect("artifact loads");
    let mut automatic = Runtime::new()
        .load(artifact, policy)
        .expect("artifact loads with native execution enabled");

    let result = vm
        .invoke("descend", vec![Value::Int(BigInt::parse("400").unwrap())])
        .expect("deep valid recursion stays within the guest call limit");
    assert_eq!(result, Value::Int(BigInt::parse("400").unwrap()));
    assert_eq!(
        automatic
            .invoke("descend", vec![Value::Int(BigInt::parse("400").unwrap())])
            .expect("native-enabled recursion falls back to heap frames"),
        result
    );
    assert!(!automatic
        .native_function_names()
        .iter()
        .any(|name| name == "descend"));

    let error = vm
        .invoke("descend", vec![Value::Int(BigInt::parse("512").unwrap())])
        .expect_err("the first call beyond the limit is rejected");
    assert_eq!(error.code, "R_CALL_DEPTH");
    assert_eq!(error.trace.len(), 513);
    assert_eq!(
        error.trace[0].line, 2,
        "depth error starts at the rejected function"
    );
    assert!(error.trace.iter().skip(1).all(|span| span.line == 4));
    assert_eq!(
        automatic
            .invoke("descend", vec![Value::Int(BigInt::parse("512").unwrap())])
            .expect_err("native-enabled recursion preserves the same depth outcome"),
        error
    );
}

#[test]
fn nested_calls_keep_exact_instruction_budget_parity_across_vm_jit_and_fallback() {
    let source = "fn leaf(value: Int) -> Int:\n    return value + 1\n\nfn descend(remaining: Int) -> Int:\n    if remaining <= 0:\n        return 0\n    return descend(remaining - 1) + 1\n\nfn twice(value: Int) -> Int:\n    return leaf(value) * 2\n\nfn combined(depth: Int) -> Int:\n    return twice(descend(depth))\n\nfn main() -> Int:\n    return combined(3)\n";
    let artifact = compile("nested-fuel-parity.gu", source).expect("nested source compiles");

    let mut exact_budget = 1;
    loop {
        let mut vm = Runtime::vm_only()
            .load(
                artifact.clone(),
                HostPolicy {
                    instruction_limit: exact_budget,
                    ..HostPolicy::default()
                },
            )
            .expect("artifact loads in VM mode");
        match vm.invoke("combined", vec![Value::Int(BigInt::from_usize(3))]) {
            Ok(value) => {
                assert_eq!(value, Value::Int(BigInt::from_usize(8)));
                break;
            }
            Err(error) => {
                assert_eq!(error.code, "R_STEP_LIMIT");
                exact_budget += 1;
                assert!(
                    exact_budget < 256,
                    "nested call budget stayed unexpectedly high"
                );
            }
        }
    }

    let mut vm_exact = Runtime::vm_only()
        .load(
            artifact.clone(),
            HostPolicy {
                instruction_limit: exact_budget,
                ..HostPolicy::default()
            },
        )
        .unwrap();
    let vm_success = vm_exact.invoke("combined", vec![Value::Int(BigInt::from_usize(3))]);
    let mut jit_exact = Runtime::new()
        .load(
            artifact.clone(),
            HostPolicy {
                instruction_limit: exact_budget,
                ..HostPolicy::default()
            },
        )
        .unwrap();
    let jit_success = jit_exact.invoke("combined", vec![Value::Int(BigInt::from_usize(3))]);
    assert!(jit_exact
        .native_function_names()
        .iter()
        .any(|name| name == "twice"));
    assert!(!jit_exact
        .native_function_names()
        .iter()
        .any(|name| name == "descend"));
    assert_eq!(jit_success, vm_success);
    assert!(
        jit_exact.native_calls() > 0,
        "the nested invocation exercised JIT code"
    );

    let mut vm_below = Runtime::vm_only()
        .load(
            artifact.clone(),
            HostPolicy {
                instruction_limit: exact_budget - 1,
                ..HostPolicy::default()
            },
        )
        .unwrap();
    let vm_failure = vm_below
        .invoke("combined", vec![Value::Int(BigInt::from_usize(3))])
        .unwrap_err();
    assert_eq!(vm_failure.code, "R_STEP_LIMIT");
    let mut jit_below = Runtime::new()
        .load(
            artifact,
            HostPolicy {
                instruction_limit: exact_budget - 1,
                ..HostPolicy::default()
            },
        )
        .unwrap();
    assert_eq!(
        jit_below
            .invoke("combined", vec![Value::Int(BigInt::from_usize(3))])
            .unwrap_err(),
        vm_failure
    );
}

#[test]
fn records_construct_access_compare_and_round_trip_through_artifacts() {
    let source = "record Point:\n    x: Int\n    y: Int\n\nfn shift(point: Point, dx: Int) -> Point:\n    return Point(point.x + dx, point.y)\n\nfn main() -> Int:\n    let original = Point(3, 4)\n    let moved = shift(original, 5)\n    assert(moved == Point(8, 4))\n    print(moved.x)\n    print(moved.y)\n    return 0\n";
    let artifact = compile("records.gu", source).expect("record program compiles");
    let encoded = artifact.to_bytes().unwrap();
    let artifact = guion_model::Artifact::from_bytes(&encoded).unwrap();
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    instance.invoke("main", vec![]).unwrap();
    assert_eq!(instance.take_output(), "8\n4\n");
}

#[test]
fn option_and_result_matches_are_exhaustive_and_run_through_artifacts() {
    let source = "fn present() -> Option[Int]:\n    return Some(42)\n\nfn absent() -> Option[Int]:\n    return None\n\nfn success() -> Result[Int, Text]:\n    return Ok(17)\n\nfn failure() -> Result[Int, Text]:\n    return Err(\"nope\")\n\nfn from_option(value: Option[Int]) -> Int:\n    match value:\n        None:\n            return 0\n        Some(found):\n            return found\n\nfn with_continuation(value: Option[Int]) -> Int:\n    var total = 0\n    match value:\n        Some(found):\n            set total = found + 1\n        None:\n            set total = 0\n    return total\n\nfn from_result(value: Result[Int, Text]) -> Int:\n    match value:\n        Err(message):\n            return 99\n        Ok(found):\n            return found\n\nfn main() -> Int:\n    print(from_option(present()))\n    print(from_option(absent()))\n    print(with_continuation(present()))\n    print(with_continuation(absent()))\n    print(from_result(success()))\n    print(from_result(failure()))\n    return 0\n";
    let artifact = compile("matching.gu", source).expect("match source compiles");
    let bytes = artifact.to_bytes().unwrap();
    let artifact = guion_model::Artifact::from_bytes(&bytes).unwrap();
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    instance.invoke("main", vec![]).unwrap();
    assert_eq!(instance.take_output(), "42\n0\n43\n0\n17\n99\n");
}

#[test]
fn option_and_result_matches_reject_incomplete_duplicate_and_wrong_variants() {
    let cases = [
        (
            "fn main() -> Int:\n    let option = make()\n    return 0\nfn make() -> Option[Int]:\n    return None\nfn use(value: Option[Int]) -> Int:\n    match value:\n        Some(item):\n            return item\n",
            "E_MATCH_NONEXHAUSTIVE",
        ),
        (
            "fn use(value: Option[Int]) -> Int:\n    match value:\n        Some(item):\n            return item\n        None:\n            return 0\n        None:\n            return 1\nfn main() -> Int:\n    return 0\n",
            "E_MATCH_DUPLICATE",
        ),
        (
            "fn use(value: Option[Int]) -> Int:\n    match value:\n        Ok(item):\n            return item\n        Err(error):\n            return 0\nfn main() -> Int:\n    return 0\n",
            "E_MATCH_VARIANT",
        ),
    ];
    for (source, expected_code) in cases {
        assert_eq!(
            compile("invalid-match.gu", source).unwrap_err().code,
            expected_code
        );
    }
}

#[test]
fn records_reject_unknown_fields_types_and_wrong_constructor_arguments() {
    let cases = [
        (
            "record Point:\n    x: Int\n\nfn main() -> Int:\n    let point = Point(1)\n    return point.missing\n",
            "E_FIELD_UNKNOWN",
        ),
        (
            "record Point:\n    x: Int\n\nfn main() -> Int:\n    let point = Point(\"wrong\")\n    return 0\n",
            "E_TYPE_ARGUMENT",
        ),
        (
            "record Point:\n    x: Imaginary\n\nfn main() -> Int:\n    return 0\n",
            "E_TYPE_UNKNOWN",
        ),
        (
            "record Point:\n    x: Int\n    x: Text\n\nfn main() -> Int:\n    return 0\n",
            "E_RECORD_FIELD_DUPLICATE",
        ),
    ];
    for (source, expected_code) in cases {
        assert_eq!(
            compile("bad-record.gu", source).unwrap_err().code,
            expected_code
        );
    }
}

#[test]
fn artifact_verifier_rejects_record_instructions_with_invalid_record_indices() {
    let source = "record Point:\n    x: Int\n\nfn main() -> Int:\n    let point = Point(1)\n    return point.x\n";
    let mut artifact = compile("bad-record-artifact.gu", source).unwrap();
    let constructor = artifact.functions[0]
        .code
        .iter_mut()
        .find(|instruction| matches!(instruction.op, Op::MakeRecord(_, _)))
        .unwrap();
    constructor.op = Op::MakeRecord(99, 1);
    assert_eq!(
        Runtime::new()
            .load(artifact, HostPolicy::default())
            .unwrap_err()
            .code,
        "R_ARTIFACT_INVALID"
    );
}

#[test]
fn instruction_budget_stops_unbounded_programs_with_a_source_trace() {
    let source = "fn main() -> Int:\n    while true:\n        let unchanged = 1\n    return 0\n";
    let artifact = compile("loop.gu", source).expect("source compiles");
    let runtime = Runtime::new();
    let policy = HostPolicy {
        instruction_limit: 80,
        ..HostPolicy::default()
    };
    let mut instance = runtime.load(artifact, policy).expect("artifact loads");
    let error = instance
        .invoke("main", vec![])
        .expect_err("loop is bounded");
    assert_eq!(error.code, "R_STEP_LIMIT");
    assert!(error.trace.iter().any(|frame| frame.line > 0));
}

#[test]
fn logical_operators_return_a_boolean_on_both_short_circuit_paths() {
    let source = "fn main() -> Int:\n    let value = false and true\n    if value:\n        return 1\n    else:\n        return 0\n    return 0\n";
    let artifact = compile("boolean.gu", source).expect("source compiles");
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .expect("artifact loads");
    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::zero())
    );
}

#[test]
fn artifact_loader_rejects_invalid_stack_control_flow_before_execution() {
    let source = "fn main() -> Int:\n    return 1\n";
    let mut artifact = compile("bad.gu", source).unwrap();
    artifact.functions[0].code[0].op = Op::Pop;
    let error = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap_err();
    assert_eq!(error.code, "R_ARTIFACT_INVALID");

    let mut artifact = guion_compiler::compile(
        "record.gu",
        "record Point:\n    x: Int\nfn main() -> Int:\n    print(Point(1).x)\n    return 0\n",
    )
    .unwrap();
    artifact.records[0].fields[0].ty = guion_model::Type::Record("Missing".into());
    let error = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap_err();
    assert_eq!(error.code, "R_ARTIFACT_INVALID");

    let mut artifact = compile("bad-source-map.gu", "fn main() -> Int:\n    return 0\n").unwrap();
    artifact.functions[0].code[0].span.source_id = 42;
    assert_eq!(
        Runtime::new()
            .load(artifact, HostPolicy::default())
            .unwrap_err()
            .code,
        "R_ARTIFACT_INVALID"
    );
}

#[test]
fn artifact_loader_rejects_forged_function_references_and_invalid_branches() {
    let mut forged_reference = compile(
        "forged-reference.gu",
        "fn identity(value: Int) -> Int:\n    return value\nfn main() -> Int:\n    return identity(1)\n",
    )
    .unwrap();
    let call = forged_reference
        .functions
        .iter_mut()
        .flat_map(|function| function.code.iter_mut())
        .find(|instruction| matches!(instruction.op, Op::Call(_, _)))
        .expect("compiled program contains a function reference");
    call.op = Op::Call(usize::MAX, 1);
    assert_eq!(
        Runtime::new()
            .load(forged_reference, HostPolicy::default())
            .unwrap_err()
            .code,
        "R_ARTIFACT_INVALID"
    );

    let mut invalid_branch = compile(
        "invalid-branch.gu",
        "fn main() -> Int:\n    if true:\n        return 1\n    else:\n        return 0\n",
    )
    .unwrap();
    let branch = invalid_branch.functions[0]
        .code
        .iter_mut()
        .find(|instruction| matches!(instruction.op, Op::JumpIfFalse(_)))
        .expect("compiled conditional contains a branch");
    branch.op = Op::JumpIfFalse(usize::MAX);
    assert_eq!(
        Runtime::new()
            .load(invalid_branch, HostPolicy::default())
            .unwrap_err()
            .code,
        "R_ARTIFACT_INVALID"
    );
}

#[test]
fn artifact_loader_rejects_stack_types_that_disagree_with_function_signatures() {
    let mut artifact = compile("bad-type.gu", "fn main() -> Int:\n    return 1\n").unwrap();
    artifact.functions[0].code[0].op = Op::Const(Value::Bool(true));
    let error = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap_err();
    assert_eq!(error.code, "R_ARTIFACT_INVALID");
}

#[test]
fn artifact_loader_rejects_an_invalid_entry_signature() {
    let mut artifact = compile("bad-entry.gu", "fn main() -> Int:\n    return 1\n").unwrap();
    artifact.functions[0].parameters = vec![guion_model::Type::Int];
    artifact.functions[0].local_count = 1;
    let error = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap_err();
    assert_eq!(error.code, "R_ARTIFACT_INVALID");
}

#[test]
fn unit_functions_can_be_called_as_statements() {
    let source = "fn announce() -> Unit:\n    print(\"ready\")\n\nfn main() -> Int:\n    announce()\n    return 0\n";
    let artifact = compile("unit.gu", source).expect("Unit calls are legal statements");
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::zero())
    );
    assert_eq!(instance.take_output(), "ready\n");
}

#[test]
fn lists_support_literals_indexing_and_for_loops() {
    let source = "fn sum(values: List[Int]) -> Int:\n    var total = 0\n    for value in values:\n        set total = total + value\n    return total\n\nfn main() -> Int:\n    let values = [2, 3, 4]\n    print(values[1])\n    return sum(values)\n";
    let artifact = compile("lists.gu", source).expect("list program compiles");
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::parse("9").unwrap())
    );
    assert_eq!(instance.take_output(), "3\n");
}

#[test]
fn maps_support_typed_literals_text_key_lookup_and_length() {
    let source = "fn main() -> Int:\n    let prices = {\"pear\": 4, \"apple\": 3}\n    print(prices[\"apple\"])\n    return len(prices)\n";
    let artifact = compile("maps.gu", source).expect("map program compiles");
    let bytes = artifact.to_bytes().expect("map artifact encodes");
    let artifact = guion_model::Artifact::from_bytes(&bytes).expect("map artifact decodes");
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::parse("2").unwrap())
    );
    assert_eq!(instance.take_output(), "3\n");
}

#[test]
fn float64_literals_arithmetic_comparison_and_artifact_execution_work() {
    let source = "fn add(x: Float64, y: Float64) -> Float64:\n    return x + y\n\nfn main() -> Int:\n    let value = add(1.25, 2.75e0)\n    assert(value == 4.0)\n    print(value)\n    return 0\n";
    let artifact = compile("float.gu", source).expect("Float64 source compiles");
    let encoded = artifact.to_bytes().expect("Float64 artifact encodes");
    let artifact = guion_model::Artifact::from_bytes(&encoded).expect("artifact decodes");
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::zero())
    );
    assert_eq!(instance.take_output(), "4\n");
}

#[test]
fn float64_rejects_implicit_integer_mixing_nonfinite_results_and_zero_division() {
    let mixed = compile(
        "mixed-float.gu",
        "fn main() -> Int:\n    let value = 1 + 2.0\n    return 0\n",
    )
    .expect_err("Int and Float64 must not mix implicitly");
    assert_eq!(mixed.code, "E_TYPE_OPERATOR");

    let oversized = compile(
        "overflow-literal.gu",
        "fn main() -> Float64:\n    return 1e309\n",
    )
    .expect_err("non-finite literals must be rejected");
    assert_eq!(oversized.code, "E_FLOAT_LITERAL");

    for (source, expected_code) in [
        ("fn compute() -> Float64:\n    return 1.0 / 0.0\n\nfn main() -> Int:\n    let ignored = compute()\n    return 0\n", "R_DIV_ZERO"),
        ("fn compute() -> Float64:\n    return 1e308 * 1e308\n\nfn main() -> Int:\n    let ignored = compute()\n    return 0\n", "R_FLOAT_NONFINITE"),
    ] {
        let artifact = compile("float-runtime-error.gu", source).unwrap();
        let mut instance = Runtime::new()
            .load(artifact, HostPolicy::default())
            .unwrap();
        assert_eq!(instance.invoke("main", vec![]).unwrap_err().code, expected_code);
    }
}

#[test]
fn json_float_values_convert_explicitly_to_float64() {
    let source = "fn main() -> Int:\n    let document = unwrap(json_parse(\"{\\\"ratio\\\": 1.25}\"))\n    let ratio = json_as_float(json_get(document, \"ratio\"))\n    assert(ratio == 1.25)\n    print(ratio * 4.0)\n    return 0\n";
    let artifact =
        compile("json-float.gu", source).expect("explicit JSON float conversion compiles");
    let encoded = artifact.to_bytes().unwrap();
    let artifact = guion_model::Artifact::from_bytes(&encoded).unwrap();
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::zero())
    );
    assert_eq!(instance.take_output(), "5\n");
}

#[test]
fn map_literals_reject_duplicate_keys_and_missing_lookups_are_traced() {
    let duplicate = compile(
        "duplicate.gu",
        "fn main() -> Int:\n    let values = {\"x\": 1, \"x\": 2}\n    return 0\n",
    )
    .expect_err("duplicate keys must not silently overwrite");
    assert_eq!(duplicate.code, "E_MAP_DUPLICATE_KEY");

    let artifact = compile(
        "missing.gu",
        "fn main() -> Int:\n    let values = {\"x\": 1}\n    return values[\"missing\"]\n",
    )
    .unwrap();
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    let error = instance
        .invoke("main", vec![])
        .expect_err("missing key fails");
    assert_eq!(error.code, "R_MAP_KEY_MISSING");
    assert_eq!(error.trace[0].line, 3);
}

#[test]
fn maps_support_immutable_updates_sorted_key_iteration_and_typed_signatures() {
    let source = "fn total(values: Map[Text, Int]) -> Int:\n    var answer = 0\n    var keys = \"\"\n    for key in values:\n        set answer = answer + values[key]\n        set keys = keys + key\n    print(keys)\n    return answer\n\nfn main() -> Int:\n    let original = {\"pear\": 4, \"apple\": 3}\n    let changed = map_set(original, \"pear\", 8)\n    let extended = map_set(changed, \"zebra\", 4)\n    assert(original[\"pear\"] == 4)\n    assert(not map_has(original, \"zebra\"))\n    assert(changed[\"pear\"] == 8)\n    return total(extended)\n";
    let artifact = compile("map-update.gu", source).expect("map operations compile");
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::parse("15").unwrap())
    );
    assert_eq!(instance.take_output(), "applepearzebra\n");
}

#[test]
fn json_values_support_typed_access_exact_integers_and_stable_round_trip() {
    let source = "fn main() -> Int:\n    let document = unwrap(json_parse(\"{\\\"name\\\":\\\"Guion\\\",\\\"count\\\":123456789012345678901}\"))\n    assert(json_has(document, \"name\"))\n    assert(json_len(document) == 2)\n    let values = unwrap(json_parse(\"[true, null, 3]\"))\n    assert(json_as_bool(json_index(values, 0)))\n    assert(json_is_null(json_index(values, 1)))\n    print(json_as_text(json_get(document, \"name\")))\n    print(json_as_int(json_get(document, \"count\")))\n    print(json_stringify(document))\n    return 0\n";
    let artifact = compile("json.gu", source).expect("JSON helpers compile");
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::zero())
    );
    assert_eq!(
        instance.take_output(),
        "Guion\n123456789012345678901\n{\"count\":123456789012345678901,\"name\":\"Guion\"}\n"
    );
}

#[test]
fn json_values_can_be_constructed_and_updated_from_typed_guion_values() {
    let source = "fn main() -> Int:\n    let row = json_set(json_set(json_object(), \"title\", json_text(\"Crème 東京\")), \"amount\", json_int(123456789012345678901234567890))\n    let rows = json_push(json_array(), row)\n    let document = json_set(json_object(), \"rows\", rows)\n    let updated = json_set(document, \"active\", json_bool(true))\n    assert(json_len(json_get(updated, \"rows\")) == 1)\n    assert(json_as_int(json_get(json_index(json_get(updated, \"rows\"), 0), \"amount\")) == 123456789012345678901234567890)\n    print(json_stringify(updated))\n    return 0\n";
    let artifact = compile("json-builder.gu", source)
        .expect("typed JSON values should be constructible without a Python adapter");
    let artifact = guion_model::Artifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::zero())
    );
    assert_eq!(
        instance.take_output(),
        "{\"active\":true,\"rows\":[{\"amount\":123456789012345678901234567890,\"title\":\"Crème 東京\"}]}\n"
    );
}

#[test]
fn json_construction_rejects_wrong_container_kinds_and_policy_overruns() {
    let wrong_array = compile(
        "json-push-type.gu",
        "fn main() -> Int:\n    let value = json_push(json_object(), json_null())\n    return 0\n",
    )
    .unwrap();
    let mut instance = Runtime::new()
        .load(wrong_array, HostPolicy::default())
        .unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap_err().code,
        "R_JSON_TYPE"
    );

    let wrong_object = compile(
        "json-set-type.gu",
        "fn main() -> Int:\n    let value = json_set(json_array(), \"key\", json_null())\n    return 0\n",
    )
    .unwrap();
    let mut instance = Runtime::new()
        .load(wrong_object, HostPolicy::default())
        .unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap_err().code,
        "R_JSON_TYPE"
    );

    let too_many = compile(
        "json-push-limit.gu",
        "fn main() -> Int:\n    let values = json_push(json_array(), json_int(1))\n    let more = json_push(values, json_int(2))\n    return 0\n",
    )
    .unwrap();
    let policy = HostPolicy {
        max_collection_items: 1,
        ..HostPolicy::default()
    };
    let mut instance = Runtime::new().load(too_many, policy).unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap_err().code,
        "R_COLLECTION_LIMIT"
    );
}

#[test]
fn missing_text_file_is_classified_and_atomic_text_write_replaces_complete_contents() {
    let root = std::env::temp_dir().join(format!("guion-atomic-write-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let source = "fn main() -> Int:\n    let missing = read_text(\"state.json\")\n    print(file_error_code(error(missing)))\n    unwrap(write_text_atomic(\"state.json\", \"{\\\"version\\\":1}\"))\n    unwrap(write_text_atomic(\"state.json\", \"{\\\"version\\\":2}\"))\n    print(unwrap(read_text(\"state.json\")))\n    return 0\n";
    let artifact = compile("atomic-write.gu", source)
        .expect("atomic text write should be available to source programs");
    let mut instance = Runtime::new()
        .load(
            artifact,
            HostPolicy::default().with_file_access(root.clone(), true, true),
        )
        .unwrap();
    instance.invoke("main", vec![]).unwrap();
    assert_eq!(
        instance.take_output(),
        "R_FILE_NOT_FOUND\n{\"version\":2}\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("state.json")).unwrap(),
        "{\"version\":2}"
    );
    assert_eq!(
        std::fs::read_dir(&root).unwrap().count(),
        1,
        "temporary files must be removed after a successful replacement"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn json_parse_returns_inspectable_errors_and_typed_access_fails() {
    let malformed = compile(
        "bad-json.gu",
        "fn main() -> Int:\n    let result = json_parse(\"{\")\n    assert(not is_ok(result))\n    let issue = error(result)\n    print(json_error_code(issue))\n    print(json_error_line(issue))\n    print(json_error_column(issue))\n    return 0\n",
    )
    .unwrap();
    let mut instance = Runtime::new()
        .load(malformed, HostPolicy::default())
        .unwrap();
    instance.invoke("main", vec![]).unwrap();
    assert_eq!(instance.take_output(), "R_JSON_PARSE\n1\n1\n");

    let wrong_type = compile("json-type.gu", "fn main() -> Int:\n    let value = json_as_int(unwrap(json_parse(\"\\\"text\\\"\")))\n    return value\n").unwrap();
    let mut instance = Runtime::new()
        .load(wrong_type, HostPolicy::default())
        .unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap_err().code,
        "R_JSON_TYPE"
    );

    let duplicate = compile(
        "duplicate-json.gu",
        r#"fn main() -> Int:
    let value = json_parse("{\"a\":1,\"\\u0061\":2}")
    assert(not is_ok(value))
    let issue = error(value)
    print(json_error_code(issue))
    print(json_error_line(issue))
    print(json_error_column(issue))
    return 0
"#,
    )
    .unwrap();
    let mut instance = Runtime::new()
        .load(duplicate, HostPolicy::default())
        .unwrap();
    instance.invoke("main", vec![]).unwrap();
    assert_eq!(instance.take_output(), "R_JSON_DUPLICATE_KEY\n1\n8\n");
}

#[test]
fn json_values_obey_integer_and_collection_limits() {
    let huge = compile(
        "json-large-int.gu",
        "fn main() -> Int:\n    let value = unwrap(json_parse(\"12345\"))\n    return json_as_int(value)\n",
    )
    .unwrap();
    let policy = HostPolicy {
        max_integer_digits: 4,
        ..HostPolicy::default()
    };
    let mut instance = Runtime::new().load(huge, policy).unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap_err().code,
        "R_INTEGER_LIMIT"
    );

    let list = compile(
        "json-large-list.gu",
        "fn main() -> Int:\n    let value = unwrap(json_parse(\"[1,2,3]\"))\n    return 0\n",
    )
    .unwrap();
    let policy = HostPolicy {
        max_collection_items: 2,
        ..HostPolicy::default()
    };
    let mut instance = Runtime::new().load(list, policy).unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap_err().code,
        "R_COLLECTION_LIMIT"
    );
}

#[test]
fn host_supplied_json_obeys_depth_size_and_collection_limits() {
    let artifact = compile(
        "host-json-limits.gu",
        "fn identity(value: Json) -> Json:\n    return value\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();

    let oversized_collection = Value::Json(SharedJson::from_serde(serde_json::json!([1, 2, 3])));
    let policy = HostPolicy {
        max_collection_items: 2,
        ..HostPolicy::default()
    };
    let mut instance = Runtime::new().load(artifact.clone(), policy).unwrap();
    assert_eq!(
        instance
            .invoke("identity", vec![oversized_collection])
            .unwrap_err()
            .code,
        "R_COLLECTION_LIMIT"
    );

    let oversized_text = Value::Json(SharedJson::from_serde(serde_json::json!({ "a": "b" })));
    let policy = HostPolicy {
        max_text_bytes: 8,
        ..HostPolicy::default()
    };
    let mut instance = Runtime::new().load(artifact.clone(), policy).unwrap();
    assert_eq!(
        instance
            .invoke("identity", vec![oversized_text])
            .unwrap_err()
            .code,
        "R_TEXT_LIMIT"
    );

    let escaped_text = Value::Json(SharedJson::from_serde(serde_json::json!("\n\u{1}")));
    let policy = HostPolicy {
        max_text_bytes: 9,
        ..HostPolicy::default()
    };
    let mut instance = Runtime::new().load(artifact.clone(), policy).unwrap();
    assert_eq!(
        instance
            .invoke("identity", vec![escaped_text])
            .unwrap_err()
            .code,
        "R_TEXT_LIMIT"
    );

    let mut too_deep = serde_json::Value::Null;
    for _ in 0..65 {
        too_deep = serde_json::json!([too_deep]);
    }
    let too_deep = Value::Json(SharedJson::from_serde(too_deep));
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    assert_eq!(
        instance
            .invoke("identity", vec![too_deep])
            .unwrap_err()
            .code,
        "R_JSON_DEPTH"
    );
}

#[test]
fn runtime_enforces_a_configurable_integer_size_limit() {
    let digits = "9".repeat(5_000);
    let source = format!("fn main() -> Int:\n    print({digits})\n    return 0\n");
    let artifact = compile("large-int.gu", &source).unwrap();
    let policy = HostPolicy {
        max_integer_digits: 256,
        ..HostPolicy::default()
    };
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    let error = instance.invoke("main", vec![]).unwrap_err();
    assert_eq!(error.code, "R_INTEGER_LIMIT");
}

#[test]
fn allocation_limit_is_shared_across_nested_calls_and_resets_per_invocation() {
    let source = "fn churn(rounds: Int) -> Int:\n    var i = 0\n    while i < rounds:\n        let text = \"12345678\" + \"abcdefgh\"\n        set i = i + 1\n    return 0\n\nfn main() -> Int:\n    return churn(20)\n";
    let artifact = compile("allocation-limit.gu", source).unwrap();
    let policy = HostPolicy {
        allocation_limit_bytes: 2048,
        ..HostPolicy::default()
    };
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap_err().code,
        "R_ALLOCATION_LIMIT"
    );
    assert_eq!(
        instance
            .invoke("churn", vec![Value::Int(BigInt::from_usize(1))])
            .unwrap(),
        Value::Int(BigInt::zero())
    );
}

#[test]
fn allocation_limit_is_shared_across_parallel_workers() {
    let source = "fn churn(rounds: Int) -> Int:\n    var i = 0\n    while i < rounds:\n        let text = \"12345678\" + \"abcdefgh\"\n        set i = i + 1\n    return 0\n\nfn work() -> Result[List[Int], TaskError]:\n    return parallel_map([100, 100], churn, 2)\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile("parallel-allocation-limit.gu", source).unwrap();
    let policy = HostPolicy {
        allocation_limit_bytes: 4096,
        ..HostPolicy::default()
    };
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    let Value::ResultErr(error) = instance.invoke("work", vec![]).unwrap() else {
        panic!("shared worker allocation exhaustion should be a TaskError")
    };
    let Value::TaskError(error) = *error else {
        panic!("parallel allocation exhaustion should preserve its runtime code")
    };
    assert_eq!(error.code, "R_ALLOCATION_LIMIT");
}

#[test]
fn allocation_limit_accounts_for_guest_collection_construction() {
    let source = "fn main() -> Int:\n    let values = [true, true, true, true, true, true, true, true]\n    return 0\n";
    let artifact = compile("stack-allocation-limit.gu", source).unwrap();
    let mut constrained = Runtime::new()
        .load(
            artifact.clone(),
            HostPolicy {
                allocation_limit_bytes: 128,
                ..HostPolicy::default()
            },
        )
        .unwrap();
    assert_eq!(
        constrained.invoke("main", vec![]).unwrap_err().code,
        "R_ALLOCATION_LIMIT"
    );

    let mut enough_for_collection = Runtime::new()
        .load(
            artifact,
            HostPolicy {
                allocation_limit_bytes: 1024,
                ..HostPolicy::default()
            },
        )
        .unwrap();
    assert_eq!(
        enough_for_collection.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::zero())
    );
}

#[test]
fn native_and_vm_modes_have_the_same_result_at_the_same_allocation_limit() {
    let source = "fn sum_to(n: Int) -> Int:\n    var total = 0\n    var i = 0\n    while i < n:\n        set total = total + i\n        set i = i + 1\n    return total\n\nfn main() -> Int:\n    return sum_to(1000)\n";
    let artifact = compile("backend-allocation-parity.gu", source).unwrap();
    let policy = HostPolicy {
        allocation_limit_bytes: 1,
        ..HostPolicy::default()
    };
    let mut automatic = Runtime::new()
        .load(artifact.clone(), policy.clone())
        .unwrap();
    let mut vm_only = Runtime::vm_only().load(artifact, policy).unwrap();

    let automatic_result = automatic.invoke("main", vec![]);
    let vm_result = vm_only.invoke("main", vec![]);
    assert!(!automatic.native_function_names().is_empty());
    assert!(vm_only.native_function_names().is_empty());
    assert_eq!(automatic_result, vm_result);
    assert_eq!(
        automatic_result.unwrap(),
        Value::Int(BigInt::parse("499500").unwrap())
    );
}

#[test]
fn allocation_limit_accounts_for_print_rendering() {
    let payload = "a".repeat(900_000);
    let source = format!("fn main() -> Int:\n    print(\"{payload}\")\n    return 0\n");
    let artifact = compile("print-allocation-limit.gu", &source).unwrap();
    let policy = HostPolicy {
        allocation_limit_bytes: 1_500_000,
        ..HostPolicy::default()
    };
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap_err().code,
        "R_ALLOCATION_LIMIT"
    );
}

#[test]
fn parallel_map_runs_pure_functions_on_multiple_workers_and_keeps_input_order() {
    let source = "fn repeat(value: Int) -> Int:\n    var i = 0\n    var total = 0\n    while i < 2000:\n        set total = total + value\n        set i = i + 1\n    return total\n\nfn results() -> Result[List[Int], TaskError]:\n    let values = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]\n    return parallel_map(values, repeat, 4)\n\nfn main() -> Int:\n    let result = results()\n    if is_ok(result):\n        return len(unwrap(result))\n    else:\n        return 0\n";
    let artifact = compile("parallel.gu", source).expect("parallel_map is part of the language");
    let mut instance = Runtime::new()
        .load(
            artifact,
            HostPolicy {
                max_parallelism: 4,
                ..HostPolicy::default()
            },
        )
        .unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::parse("16").unwrap())
    );
    let Value::ResultOk(value) = instance.invoke("results", vec![]).unwrap() else {
        unreachable!()
    };
    let Value::List(results) = *value else {
        unreachable!()
    };
    assert_eq!(
        results.first(),
        Some(&Value::Int(BigInt::parse("2000").unwrap()))
    );
    assert_eq!(
        results.last(),
        Some(&Value::Int(BigInt::parse("32000").unwrap()))
    );
    assert!(
        instance.parallel_threads_used() > 1,
        "CPU work ran on only one worker"
    );
}

#[test]
fn parallel_failures_cancel_pending_work_and_join_running_workers() {
    let source = "fn spin(value: Int) -> Int:\n    while true:\n        let again = value\n    return value\n\nfn work() -> Result[List[Int], TaskError]:\n    return parallel_map([1, 2, 3, 4, 5, 6, 7, 8], spin, 4)\n\nfn main() -> Int:\n    let result = work()\n    if is_ok(result):\n        return 1\n    else:\n        let problem = error(result)\n        print(task_index(problem))\n        print(task_code(problem))\n        return 0\n";
    let artifact = compile("parallel-failure.gu", source).unwrap();
    let policy = HostPolicy {
        instruction_limit: 300,
        max_parallelism: 4,
        ..HostPolicy::default()
    };
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    let Value::ResultErr(value) = instance.invoke("work", vec![]).unwrap() else {
        panic!("parallel failure is returned as Err")
    };
    let Value::TaskError(task_error) = *value else {
        panic!("parallel Err contains TaskError")
    };
    assert_eq!(task_error.code, "R_STEP_LIMIT");
    assert!(task_error.index < BigInt::parse("4").unwrap());
    assert!(!task_error.trace.is_empty());
    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::zero())
    );
    let output = instance.take_output();
    assert!(output.lines().any(|line| line == "R_STEP_LIMIT"));
    assert!(instance.parallel_threads_used() > 0);
}

#[test]
fn file_host_calls_need_a_root_and_cannot_escape_it() {
    let dir = std::env::temp_dir().join(format!("guion-files-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("input.txt"), "hello é").unwrap();
    let source = "fn main() -> Int:\n    let read_result = read_text(\"input.txt\")\n    if is_ok(read_result):\n        let content = unwrap(read_result)\n        unwrap(write_text(\"copy.txt\", content))\n    else:\n        print(file_error_code(error(read_result)))\n    return 0\n";
    let artifact = compile("files.gu", source).unwrap();
    let mut denied = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    denied.invoke("main", vec![]).unwrap();
    assert_eq!(denied.take_output(), "R_HOST_DENIED\n");

    let policy = HostPolicy::default().with_file_access(&dir, true, true);
    let mut allowed = Runtime::new().load(artifact, policy).unwrap();
    allowed.invoke("main", vec![]).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("copy.txt")).unwrap(),
        "hello é"
    );

    let escape = compile(
        "escape.gu",
        "fn main() -> Int:\n    let result = read_text(\"../outside.txt\")\n    assert(not is_ok(result))\n    print(file_error_code(error(result)))\n    return 0\n",
    )
    .unwrap();
    let mut escaped = Runtime::new()
        .load(
            escape,
            HostPolicy::default().with_file_access(&dir, true, false),
        )
        .unwrap();
    escaped.invoke("main", vec![]).unwrap();
    assert_eq!(escaped.take_output(), "R_HOST_DENIED\n");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn named_file_root_aliases_cannot_fall_through_to_another_grant_or_escape() {
    let root = std::env::temp_dir().join(format!("guion-file-alias-{}", std::process::id()));
    let project = root.join("project");
    let standard = root.join("standard");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(project.join("std")).unwrap();
    std::fs::create_dir_all(&standard).unwrap();
    std::fs::write(project.join("std/input.txt"), "shadow").unwrap();
    std::fs::write(standard.join("input.txt"), "trusted").unwrap();
    std::fs::write(project.join("secret.txt"), "private").unwrap();

    let source = "fn main() -> Int:\n    print(unwrap(read_text(\"std/input.txt\")))\n    let escape = read_text(\"std/../secret.txt\")\n    assert(not is_ok(escape))\n    print(file_error_code(error(escape)))\n    return 0\n";
    let artifact = compile("file-root-alias.gu", source).unwrap();
    let policy = HostPolicy::default()
        .with_file_access(&project, true, false)
        .with_file_root_alias("std", &standard);
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    instance.invoke("main", vec![]).unwrap();
    assert_eq!(instance.take_output(), "trusted\nR_HOST_DENIED\n");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn binary_file_reads_preserve_bytes_and_enforce_the_file_limit() {
    let dir = std::env::temp_dir().join(format!("guion-binary-files-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bytes = [0, 0xff, 0x80, b'x'];
    std::fs::write(dir.join("input.bin"), bytes).unwrap();
    let source = "fn main() -> Int:\n    let payload = unwrap(read_bytes(\"input.bin\"))\n    unwrap(write_bytes(\"copy.bin\", payload))\n    return len(unwrap(read_bytes(\"copy.bin\")))\n";
    let artifact = compile("binary-files.gu", source).unwrap();
    let policy = HostPolicy::default().with_file_access(&dir, true, true);
    let mut instance = Runtime::new().load(artifact.clone(), policy).unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::parse("4").unwrap())
    );
    assert_eq!(std::fs::read(dir.join("copy.bin")).unwrap(), bytes);

    std::fs::write(dir.join("too-large.bin"), [0, 1, 2, 3, 4]).unwrap();
    let too_large = compile(
        "binary-limit.gu",
        "fn main() -> Int:\n    return len(unwrap(read_bytes(\"too-large.bin\")))\n",
    )
    .unwrap();
    let mut limited = Runtime::new()
        .load(
            too_large,
            HostPolicy {
                max_file_bytes: 4,
                ..HostPolicy::default().with_file_access(&dir, true, false)
            },
        )
        .unwrap();
    assert_eq!(
        limited.invoke("main", vec![]).unwrap_err().code,
        "R_FILE_LIMIT"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn result_constructors_and_try_propagate_file_errors() {
    let dir = std::env::temp_dir().join(format!("guion-result-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("input.txt"), "safe").unwrap();
    let source = "fn relay(path: Text) -> Result[Text, FileError]:\n    return Ok(read_text(path)?)\n\nfn explicit_ok(text: Text) -> Result[Text, FileError]:\n    return Ok(text)\n\nfn explicit_err(problem: FileError) -> Result[Text, FileError]:\n    return Err(problem)\n\nfn main() -> Int:\n    let success = relay(\"input.txt\")\n    let wrapped = explicit_ok(unwrap(success))\n    print(unwrap(wrapped))\n    let failure = relay(\"missing.txt\")\n    let wrapped_failure = explicit_err(error(failure))\n    print(file_error_code(error(wrapped_failure)))\n    print(file_error_line(error(wrapped_failure)))\n    print(file_error_column(error(wrapped_failure)))\n    return 0\n";
    let artifact = compile("result.gu", source).unwrap();
    let policy = HostPolicy::default().with_file_access(&dir, true, true);
    let mut instance = Runtime::new().load(artifact, policy).unwrap();
    instance.invoke("main", vec![]).unwrap();
    assert_eq!(instance.take_output(), "safe\nR_FILE_NOT_FOUND\n2\n15\n");
    let _ = std::fs::remove_dir_all(dir);

    let invalid_context =
        compile("result-context.gu", "fn main() -> Int:\n    return Ok(1)\n").unwrap_err();
    assert_eq!(invalid_context.code, "E_RESULT_CONSTRUCTOR_CONTEXT");
    let invalid_try = compile(
        "result-try.gu",
        "fn main() -> Int:\n    let text = read_text(\"x\")?\n    return 0\n",
    )
    .unwrap_err();
    assert_eq!(invalid_try.code, "E_RESULT_TRY_FUNCTION");
    let mismatched_error = compile(
        "result-error-type.gu",
        "fn parse(text: Text) -> Result[Int, FileError]:\n    return Ok(json_parse(text)?)\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap_err();
    assert_eq!(mismatched_error.code, "E_RESULT_TRY_ERROR");
}

#[test]
fn instances_support_repeated_invocation_and_reject_use_after_close() {
    let source = "fn greet() -> Int:\n    print(\"hello\")\n    return 7\n\nfn main() -> Int:\n    return greet()\n";
    let artifact = compile("lifecycle.gu", source).unwrap();
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    for _ in 0..2 {
        assert_eq!(
            instance.invoke("main", vec![]).unwrap(),
            Value::Int(BigInt::parse("7").unwrap())
        );
        assert_eq!(instance.take_output(), "hello\n");
    }
    instance.close();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap_err().code,
        "R_INSTANCE_CLOSED"
    );
}

#[test]
fn option_values_are_constructed_checked_unwrapped_and_portable() {
    let source = "fn maybe(value: Int) -> Option[Int]:\n    if value > 0:\n        return Some(value)\n    return None\n\nfn main() -> Int:\n    let present = maybe(42)\n    if is_some(present):\n        return unwrap(present)\n    return 0\n";
    let artifact = compile("option.gu", source).expect("Option source compiles");
    let bytes = artifact.to_bytes().expect("Option artifact serializes");
    let artifact = guion_model::Artifact::from_bytes(&bytes).expect("Option artifact loads");
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .expect("Option artifact verifies");
    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::parse("42").unwrap())
    );
}

#[test]
fn option_unwrap_reports_absence_and_none_needs_an_option_context() {
    let source = "fn maybe() -> Option[Int]:\n    return None\n\nfn main() -> Int:\n    return unwrap(maybe())\n";
    let artifact = compile("option-empty.gu", source).unwrap();
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap_err().code,
        "R_OPTION_UNWRAP"
    );

    let invalid = compile("option-context.gu", "fn main() -> Int:\n    return None\n").unwrap_err();
    assert_eq!(invalid.code, "E_OPTION_CONSTRUCTOR_CONTEXT");
}

/// The native runtime and the portable core run the same instruction loop, so output, return
/// value, and the exact instruction count agree when the native runtime uses its bytecode VM.
#[test]
fn native_vm_matches_shared_portable_core() {
    use guion_vm_core::{PortableHost, VmLimits};
    use std::sync::Arc;

    let programs = [
        include_str!("../../vm-core/tests/fixtures/hello.gu"),
        "fn main() -> Int:\n    var total = 0\n    var i = 1\n    while i <= 10:\n        set total = total + i\n        set i = i + 1\n    print(total)\n    return total\n",
        "fn fib(n: Int) -> Int:\n    if n < 2:\n        return n\n    return fib(n - 1) + fib(n - 2)\n\nfn main() -> Int:\n    print(fib(10))\n    return fib(10)\n",
    ];
    for source in programs {
        let artifact = compile("parity.gu", source).expect("parity fixture compiles");
        let bytes = artifact.to_bytes().expect("artifact serializes");
        let artifact = Artifact::from_bytes(&bytes).expect("artifact decodes");

        let mut portable = PortableHost::new(Arc::new(artifact.clone()), VmLimits::default());
        let expected = portable
            .invoke("main", vec![], 1_000_000)
            .expect("portable core runs the program");
        let expected_output = portable.take_output();

        let policy = |instruction_limit: u64| HostPolicy {
            instruction_limit,
            ..HostPolicy::default()
        };
        let mut native = Runtime::vm_only()
            .load(artifact.clone(), policy(expected.steps))
            .expect("native runtime loads the artifact");
        let value = native
            .invoke("main", vec![])
            .expect("native VM finishes within the portable step count");
        assert_eq!(value, expected.return_value);
        assert_eq!(native.take_output(), expected_output);

        let mut starved = Runtime::vm_only()
            .load(artifact, policy(expected.steps - 1))
            .expect("native runtime loads the artifact");
        let error = starved
            .invoke("main", vec![])
            .expect_err("one fewer instruction exhausts the native budget");
        assert_eq!(error.code, "R_STEP_LIMIT");
    }
}
