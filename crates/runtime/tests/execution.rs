use guion_compiler::{compile, compile_project_with_roots};
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
fn text_print_compiles_natively_and_matches_the_vm() {
    let source = "fn main() -> Int:\n    var total = 0\n    var index = 0\n    while index < 1000:\n        set total = total + index % 3\n        set index = index + 1\n    print(\"a🙂é\")\n    print(\"\")\n    print(\"total \" + json_stringify(json_int(total)))\n    return 0\n";
    let artifact = compile("text-print.gu", source).expect("text print source compiles");
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .expect("native runtime loads the artifact");
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .expect("runtime loads the artifact");

    native.invoke("main", vec![]).expect("native program runs");
    vm.invoke("main", vec![]).expect("VM program runs");
    let output = native.take_output();
    assert_eq!(output, "a🙂é\n\ntotal 999\n");
    assert_eq!(output, vm.take_output());
    assert!(
        native
            .native_function_names()
            .iter()
            .any(|name| name == "main"),
        "{}",
        native.native_compile_profile()
    );
    assert_eq!(native.native_deoptimizations(), 0);
}

/// The register form must not be observable: both VM paths have to agree on the result, the
/// output, and the error with its location and remaining budget at every limit.
#[test]
fn register_form_matches_the_stack_interpreter_at_every_limit() {
    let source = r#"import "std:json"

record Item:
    name: Text
    count: Int

fn pick(flag: Bool) -> Option[Int]:
    if flag:
        return Some(7)
    return None

fn checked(value: Int) -> Result[Int, Text]:
    if value < 0:
        return Err("negative")
    return Ok(value * 2)

fn main() -> Int:
    var counts: Map[Text, Int] = {}
    var seen: List[Text] = []
    var total = 0
    var ratio = 1.5
    var index = 0
    let names = ["a", "bb", "a", "ccc", "bb", "a"]
    while index < len(names):
        let name = names[index]
        if not map_has(counts, name):
            set counts = map_set(counts, name, 0)
            set seen = list_push(seen, name)
        set counts = map_set(counts, name, counts[name] + index)
        set total = total + counts[name] * 2 - 1
        set ratio = ratio * 1.25 - 0.5
        set index = index + 1
    let item = Item("tool", total)
    let shadow = item
    if shadow.name == "tool" and shadow.count != 0 and item.name != "":
        set total = total + shadow.count
    let some = pick(total > 3)
    let none = pick(total < 3)
    if is_some(some) and not is_some(none):
        set total = total + unwrap(some)
    let good = checked(total)
    let bad = checked(0 - total)
    if is_ok(good) and not is_ok(bad):
        set total = total + unwrap(good)
    let record = json_parse("{\"id\": 41, \"name\": \"nine\", \"big\": 123456789012345678901234567890, \"tag\": [1]}")
    if is_ok(record):
        let document = unwrap(record)
        let id = json_int_safe(unwrap(json_get_safe(document, "id")))
        let name = json_text_safe(unwrap(json_get_safe(document, "name")))
        let held = json_get_safe(document, "name")
        let held_text = json_text_safe(unwrap(held))
        let number = json_get_safe(document, "id")
        let held_int = json_int_safe(unwrap(number))
        let huge = json_int_safe(unwrap(json_get_safe(document, "big")))
        let wrong = json_int_safe(unwrap(json_get_safe(document, "name")))
        let missing = json_get_safe(document, "absent")
        if is_ok(id) and is_ok(name) and is_ok(held_text) and is_ok(held_int) and not is_ok(wrong) and not is_ok(missing):
            set total = total + unwrap(id) + unwrap(held_int)
            print(unwrap(name) + unwrap(held_text))
        print(is_ok(huge))
    print(seen[0] + seen[len(seen) - 1])
    print(total)
    print(ratio)
    let last = names[total - total + 5]
    print(last)
    print(counts["missing" + last])
    return total
"#;
    let project_root = std::env::temp_dir().join(format!(
        "guion-register-exact-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&project_root).unwrap();
    let entry = project_root.join("main.gu");
    std::fs::write(&entry, source).unwrap();
    let stdlib_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib");
    let artifact = compile_project_with_roots(
        &entry,
        &project_root,
        Some(&stdlib_root),
        &std::collections::HashMap::new(),
    )
    .expect("source compiles");
    let _ = std::fs::remove_dir_all(&project_root);
    let run = |plain: bool, instructions: u64, bytes: usize| {
        let policy = HostPolicy {
            instruction_limit: instructions,
            allocation_limit_bytes: bytes,
            ..HostPolicy::default()
        };
        let runtime = if plain {
            // Profiled runs do not use the register form.
            Runtime::vm_only().with_runtime_profile()
        } else {
            Runtime::vm_only()
        };
        let mut instance = runtime.load(artifact.clone(), policy).expect("loads");
        let result = instance.invoke("main", vec![]);
        (format!("{result:?}"), instance.take_output())
    };
    let (finished, output) = run(false, 1_000_000, 1 << 24);
    assert!(
        finished.contains("R_MAP_KEY_MISSING"),
        "the program ends on its deliberate error: {finished}"
    );
    assert!(output.contains("ninenine"), "{output}");
    assert_eq!(run(true, 1_000_000, 1 << 24), (finished, output));
    let mut stopped_by_fuel = 0;
    for instructions in 0..1200 {
        let plain = run(true, instructions, 1 << 24);
        stopped_by_fuel += usize::from(plain.0.contains("R_STEP_LIMIT"));
        assert_eq!(plain, run(false, instructions, 1 << 24), "{instructions}");
    }
    assert!(stopped_by_fuel > 200, "{stopped_by_fuel}");
    let mut stopped_by_bytes = 0;
    for bytes in (0..3000).step_by(3) {
        let plain = run(true, 1_000_000, bytes);
        stopped_by_bytes += usize::from(plain.0.contains("R_ALLOCATION_LIMIT"));
        assert_eq!(plain, run(false, 1_000_000, bytes), "{bytes}");
    }
    assert!(stopped_by_bytes > 100, "{stopped_by_bytes}");
}

#[test]
fn csv_rows_over_the_limits_fail_the_same_on_both_vm_paths() {
    let source = "import \"std:csv\"\n\nfn main() -> Int:\n    let reader = unwrap(csv_open_reader(\"rows.csv\"))\n    var rows = 0\n    var done = false\n    while not done:\n        let next = unwrap(csv_next_row(reader))\n        if is_some(next):\n            set rows = rows + len(unwrap(next).fields)\n        else:\n            set done = true\n    return rows\n";
    let root = std::env::temp_dir().join(format!(
        "guion-csv-limits-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let entry = root.join("main.gu");
    std::fs::write(&entry, source).unwrap();
    std::fs::write(root.join("rows.csv"), "a,b\nc,d,e\nlonger,f\n").unwrap();
    let stdlib_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib");
    let artifact = compile_project_with_roots(
        &entry,
        &root,
        Some(&stdlib_root),
        &std::collections::HashMap::new(),
    )
    .expect("source compiles");
    let run = |plain: bool, text: usize, items: usize, digits: usize| {
        let policy = HostPolicy {
            max_text_bytes: text,
            max_collection_items: items,
            max_integer_digits: digits,
            ..HostPolicy::default()
        }
        .with_file_access(root.clone(), true, false);
        let runtime = if plain {
            Runtime::vm_only().with_runtime_profile()
        } else {
            Runtime::vm_only()
        };
        let mut instance = runtime.load(artifact.clone(), policy).expect("loads");
        format!("{:?}", instance.invoke("main", vec![]))
    };
    for (text, items, digits, expected) in [
        (1000, 1000, 100, "7"),
        (3, 1000, 100, "R_TEXT_LIMIT"),
        (1000, 2, 100, "R_COLLECTION_LIMIT"),
        (1000, 1000, 1, "7"),
    ] {
        let register = run(false, text, items, digits);
        assert!(register.contains(expected), "{register}");
        assert_eq!(run(true, text, items, digits), register);
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn integer_division_in_the_register_form_matches_the_stack_interpreter() {
    let source = "fn main() -> Int:\n    let lowest = 0 - 9223372036854775807 - 1\n    let values = [7, 0 - 7, 6, 0 - 6, 0, 1, 0 - 1, lowest, 9223372036854775807]\n    var left = 0\n    while left < len(values):\n        var right = 0\n        while right < len(values):\n            if values[right] != 0:\n                print(values[left] / values[right])\n                print(values[left] // values[right])\n                print(values[left] % values[right])\n            set right = right + 1\n        set left = left + 1\n    print(lowest / (0 - 1))\n    print(lowest // (0 - 1))\n    print(lowest % (0 - 1))\n    print(left % (left - left))\n    return 0\n";
    let artifact = compile("division.gu", source).expect("source compiles");
    let run = |runtime: Runtime| {
        let mut instance = runtime
            .load(artifact.clone(), HostPolicy::default())
            .expect("loads");
        let result = instance.invoke("main", vec![]);
        (format!("{result:?}"), instance.take_output())
    };
    let registers = run(Runtime::vm_only());
    assert!(registers.0.contains("R_DIV_ZERO"), "{}", registers.0);
    assert!(
        registers.1.contains("9223372036854775808"),
        "the lowest integer over -1 does not fit in 64 bits: {}",
        registers.1
    );
    // Profiled runs do not use the register form.
    assert_eq!(run(Runtime::vm_only().with_runtime_profile()), registers);
}

#[test]
fn map_order_is_sorted_by_key_however_it_was_built() {
    let source = "fn main() -> Int:\n    var values: Map[Text, Int] = {}\n    let keys = [\"pear\", \"apple\", \"zebra\", \"mango\", \"é\", \"\", \"Apple\", \"a\", \"ab\"]\n    var index = 0\n    while index < len(keys):\n        set values = map_set(values, keys[index], index)\n        set index = index + 1\n    set values = map_set(values, \"pear\", 40)\n    for name in values:\n        print(\"[\" + name + \"]\")\n        print(values[name])\n    print(values)\n    return 0\n";
    let artifact = compile("map-order.gu", source).expect("source compiles");
    let mut instance = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .expect("loads");
    instance.invoke("main", vec![]).expect("runs");
    let output = instance.take_output();
    let names: Vec<&str> = output
        .lines()
        .filter(|line| line.starts_with('['))
        .collect();
    assert_eq!(
        names,
        ["[]", "[Apple]", "[a]", "[ab]", "[apple]", "[mango]", "[pear]", "[zebra]", "[é]"]
    );
    assert!(output.contains("40"), "{output}");
    let rendered = output.lines().last().unwrap();
    let order =
        ["Apple", "apple", "mango", "pear", "zebra"].map(|key| rendered.find(key).expect(key));
    assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{rendered}");
}

#[test]
fn native_failure_keeps_output_printed_before_it() {
    for (source, expected) in [
        ("fn main() -> Int:\n    print(7)\n    return 1 / 0\n", "7\n"),
        (
            "fn main() -> Int:\n    var index = 0\n    while index < 3:\n        set index = index + 1\n    print(\"before\")\n    return 1 / 0\n",
            "before\n",
        ),
    ] {
        let artifact = compile("print-then-fail.gu", source).expect("source compiles");
        let mut native = Runtime::new()
            .load(artifact.clone(), HostPolicy::default())
            .expect("native runtime loads");
        let mut vm = Runtime::vm_only()
            .load(artifact, HostPolicy::default())
            .expect("VM runtime loads");
        assert_eq!(
            native.invoke("main", vec![]).unwrap_err().code,
            "R_DIV_ZERO"
        );
        assert_eq!(vm.invoke("main", vec![]).unwrap_err().code, "R_DIV_ZERO");
        assert_eq!(native.take_output(), expected);
        assert_eq!(vm.take_output(), expected);
        assert!(
            native
                .native_function_names()
                .iter()
                .any(|name| name == "main"),
            "{}",
            native.native_compile_profile()
        );
    }
}

#[test]
fn text_slice_uses_unicode_scalar_half_open_bounds() {
    let source = "fn slice_text() -> Text:\n    return text_slice(\"a🙂é\", 1, 3)\n\nfn empty_slice() -> Text:\n    return text_slice(\"a🙂é\", 2, 2)\n\nfn main() -> Int:\n    if slice_text() == \"🙂é\" and empty_slice() == \"\":\n        return 0\n    return 1\n";
    let artifact = compile("text-slice.gu", source).expect("text_slice source compiles");
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .expect("native runtime loads the artifact");
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .expect("runtime loads the artifact");

    assert_eq!(
        native.invoke("main", vec![]).expect("native program runs"),
        Value::Int(BigInt::from_i64(0))
    );
    assert_eq!(
        vm.invoke("main", vec![]).expect("VM program runs"),
        Value::Int(BigInt::from_i64(0))
    );
    assert!(
        native
            .native_function_names()
            .iter()
            .any(|name| name == "main"),
        "{}",
        native.native_compile_profile()
    );
    assert_eq!(native.native_deoptimizations(), 0);
}

#[test]
fn native_text_search_matches_scalar_semantics() {
    let source = "fn main() -> Int:\n    assert(text_find(\"a🙂b🙂\", \"🙂\", 0) == 1)\n    assert(text_find(\"a🙂b🙂\", \"🙂\", 2) == 3)\n    assert(text_find(\"a🙂b🙂\", \"x\", 0) == -1)\n    assert(text_find(\"abcabc\", \"bc\", 2) == 4)\n    assert(text_find(\"abc\", \"\", 3) == 3)\n    assert(text_find(\"\", \"a\", 0) == -1)\n    let fields = text_split_native(\"|α||🙂|\", \"|\")\n    assert(len(fields) == 5)\n    assert(fields[1] == \"α\" and fields[2] == \"\" and fields[3] == \"🙂\" and fields[4] == \"\")\n    assert(len(text_split_native(\"aaaa\", \"aa\")) == 3)\n    assert(text_split_native(\"abc\", \"\")[0] == \"abc\")\n    assert(len(text_split_native(\"\", \",\")) == 1)\n    assert(text_replace_native(\"αββ\", \"β\", \"X\") == \"αXX\")\n    assert(text_replace_native(\"aaa\", \"aa\", \"b\") == \"ba\")\n    assert(text_replace_native(\"abc\", \"\", \"x\") == \"abc\")\n    assert(text_trim_native(\"\\u{3000} hi \\u{00a0}\\n\") == \"hi\")\n    assert(text_trim_native(\" \\t\") == \"\")\n    return 0\n\nfn find_past_end() -> Int:\n    return text_find(\"ab\", \"a\", 3)\n";
    let artifact = compile("text-search.gu", source).expect("text search source compiles");
    for runtime in [Runtime::new(), Runtime::vm_only()] {
        let mut instance = runtime
            .load(artifact.clone(), HostPolicy::default())
            .expect("runtime loads the artifact");
        assert_eq!(
            instance.invoke("main", vec![]).expect("program runs"),
            Value::Int(BigInt::from_i64(0))
        );
        assert_eq!(
            instance
                .invoke("find_past_end", vec![])
                .expect_err("a start past the end is rejected")
                .code,
            "R_INDEX_BOUNDS"
        );
    }
}

#[test]
fn native_text_operations_match_the_vm() {
    let source = "fn text_split(value: Text, delimiter: Text) -> List[Text]:\n    return text_split_native(value, delimiter)\n\nfn text_trim(value: Text) -> Text:\n    return text_trim_native(value)\n\nfn text_replace(value: Text, needle: Text, replacement: Text) -> Text:\n    return text_replace_native(value, needle, replacement)\n\nfn text_join(values: List[Text], delimiter: Text) -> Text:\n    return text_join_native(values, delimiter)\n\nfn fields(row: Text) -> Int:\n    let parts = text_split(row, \",\")\n    return len(text_trim(parts[0])) + len(parts) * 100\n\nfn fill(template: Text) -> Text:\n    return text_replace(text_replace(template, \"{{name}}\", \"Ada\"), \"{{x}}\", \"é\")\n\nfn rejoin(row: Text) -> Text:\n    return text_join(text_split(row, \",\"), \"|\")\n\nfn count(text: Text, needle: Text) -> Int:\n    var found = 0\n    var position = text_find(text, needle, 0)\n    while position >= 0:\n        set found = found + 1\n        set position = text_find(text, needle, position + 1)\n    return found\n\nfn find_past_end(text: Text) -> Int:\n    return text_find(text, \"a\", len(text) + 1)\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile("native-text-ops.gu", source).expect("text operations compile");
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .expect("native runtime loads the artifact");
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .expect("runtime loads the artifact");
    let long = "aé🙂b".repeat(3000);
    let cases = [
        ("fields", vec![Value::Text("  α  ,b,,c".into())]),
        (
            "fill",
            vec![Value::Text("{{name}} {{x}}{{x}} {{nam".into())],
        ),
        ("fill", vec![Value::Text("unchanged".into())]),
        ("rejoin", vec![Value::Text("a,,é,".into())]),
        (
            "count",
            vec![Value::Text(long.as_str().into()), Value::Text("🙂".into())],
        ),
        (
            "count",
            vec![Value::Text(long.as_str().into()), Value::Text("éx".into())],
        ),
    ];
    for (function, arguments) in cases {
        assert_eq!(
            native
                .invoke(function, arguments.clone())
                .expect("native call runs"),
            vm.invoke(function, arguments).expect("VM call runs"),
            "{function}"
        );
    }
    let names = native.native_function_names();
    for function in ["fields", "fill", "rejoin", "count"] {
        assert!(
            names.iter().any(|name| name == function),
            "{function} is not native: {}",
            native.native_compile_profile()
        );
    }
    assert_eq!(native.native_deoptimizations(), 0);

    let arguments = vec![Value::Text("abc".into())];
    let native_error = native
        .invoke("find_past_end", arguments.clone())
        .expect_err("a start past the end is rejected");
    let vm_error = vm
        .invoke("find_past_end", arguments)
        .expect_err("a start past the end is rejected");
    assert_eq!(native_error.code, "R_INDEX_BOUNDS");
    assert_eq!(native_error.code, vm_error.code);
    assert_eq!(native.native_deoptimizations(), 1);
}

#[test]
fn native_text_split_and_join_charge_the_same_allocation_as_the_vm() {
    let source = "fn split_join(row: Text) -> Text:\n    return text_join_native(text_split_native(row, \",\"), \"|\")\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile("native-text-split-allocation.gu", source).unwrap();
    let input = vec![Value::Text("a,bb,ccc".into())];
    let outcome = |runtime: Runtime, limit: usize| {
        runtime
            .load(
                artifact.clone(),
                HostPolicy {
                    allocation_limit_bytes: limit,
                    ..HostPolicy::default()
                },
            )
            .unwrap()
            .invoke("split_join", input.clone())
            .map_err(|error| error.code)
    };
    let mut limit = 1;
    while outcome(Runtime::vm_only(), limit).is_err() {
        limit += 1;
    }
    assert_eq!(
        outcome(Runtime::new(), limit),
        Ok(Value::Text("a|bb|ccc".into()))
    );
    assert_eq!(
        outcome(Runtime::new(), limit - 1),
        Err("R_ALLOCATION_LIMIT".to_string())
    );
}

#[test]
fn text_find_seeks_scalar_indices_in_long_unicode_text() {
    let source = "fn main() -> Int:\n    var parts: List[Text] = []\n    var index = 0\n    while index < 3000:\n        set parts = list_push(parts, \"é-ab\")\n        set index = index + 1\n    let text = text_join_native(parts, \"\")\n    var found = 0\n    var position = text_find(text, \"ab\", 0)\n    while position >= 0:\n        assert(position % 4 == 2)\n        set found = found + 1\n        set position = text_find(text, \"ab\", position + 2)\n    assert(text_find(text, \"ab\", 8001) == 8002)\n    assert(text_find(text, \"é\", 11999) == -1)\n    assert(text_find(text, \"x\", 12000) == -1)\n    return found\n\nfn find_past_end() -> Int:\n    var parts: List[Text] = []\n    var index = 0\n    while index < 3000:\n        set parts = list_push(parts, \"é-ab\")\n        set index = index + 1\n    return text_find(text_join_native(parts, \"\"), \"ab\", 12001)\n";
    let artifact = compile("text-find-long.gu", source).expect("long text search compiles");
    for runtime in [Runtime::new(), Runtime::vm_only()] {
        let mut instance = runtime
            .load(artifact.clone(), HostPolicy::default())
            .expect("runtime loads the artifact");
        assert_eq!(
            instance.invoke("main", vec![]).expect("program runs"),
            Value::Int(BigInt::from_i64(3000))
        );
        assert_eq!(
            instance
                .invoke("find_past_end", vec![])
                .expect_err("a start past the end is rejected")
                .code,
            "R_INDEX_BOUNDS"
        );
    }
}

#[test]
fn vm_callers_keep_copy_dominated_native_entries_in_the_vm() {
    // `main` stays in the VM (it pushes onto a local list); `first_is` is native, does
    // constant work, and would otherwise copy the whole list on every call.
    let source = "fn first_is(values: List[Text], expected: Text) -> Bool:\n    return values[0] == expected\n\nfn main() -> Int:\n    var values: List[Text] = []\n    var index = 0\n    while index < 5000:\n        set values = list_push(values, \"x\")\n        set index = index + 1\n    var hits = 0\n    var call = 0\n    while call < 200:\n        if first_is(values, \"x\"):\n            set hits = hits + 1\n        set call = call + 1\n    return hits\n";
    let artifact = compile("entry-cost.gu", source).expect("entry-cost source compiles");
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .expect("native runtime loads the artifact");
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .expect("runtime loads the artifact");
    assert_eq!(
        native.invoke("main", vec![]).expect("native program runs"),
        Value::Int(BigInt::from_i64(200))
    );
    assert_eq!(
        vm.invoke("main", vec![]).expect("VM program runs"),
        Value::Int(BigInt::from_i64(200))
    );
    assert!(
        native
            .native_function_names()
            .iter()
            .any(|name| name == "first_is"),
        "{}",
        native.native_compile_profile()
    );
    assert!(
        native.native_calls() < 50,
        "copy-dominated entries kept entering native code: {}",
        native.native_calls()
    );
}

#[test]
fn list_push_and_in_place_set_keep_item_limits() {
    let source = "fn main() -> Int:\n    var values: List[Text] = []\n    var index = 0\n    while index < 20000:\n        set values = list_push(values, \"x\")\n        set index = index + 1\n    set values = list_set(values, 3, \"y\")\n    return len(values)\n\nfn oversized() -> Int:\n    var values: List[Text] = [\"a\"]\n    set values = list_push(values, \"abcdefgh\")\n    return len(values)\n";
    let artifact = compile("list-push-limits.gu", source).expect("list push source compiles");
    let policy = HostPolicy {
        instruction_limit: 10_000_000,
        ..HostPolicy::default()
    };
    let mut vm = Runtime::vm_only()
        .load(artifact.clone(), policy)
        .expect("runtime loads the artifact");
    assert_eq!(
        vm.invoke("main", vec![]).expect("program runs"),
        Value::Int(BigInt::from_i64(20000))
    );
    let policy = HostPolicy {
        max_text_bytes: 4,
        ..HostPolicy::default()
    };
    let mut limited = Runtime::vm_only()
        .load(artifact, policy)
        .expect("runtime loads the artifact");
    assert_eq!(
        limited
            .invoke("oversized", vec![])
            .expect_err("an oversized pushed text is rejected")
            .code,
        "R_TEXT_LIMIT"
    );
}

#[test]
fn text_slice_rejects_invalid_scalar_ranges() {
    let source = "fn invalid_slice() -> Text:\n    return text_slice(\"abc\", 2, 1)\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile("text-slice-range.gu", source).expect("text_slice source compiles");
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .expect("native runtime loads the artifact");
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .expect("runtime loads the artifact");

    let native_error = native
        .invoke("invalid_slice", vec![])
        .expect_err("a reversed scalar range is rejected");
    let vm_error = vm
        .invoke("invalid_slice", vec![])
        .expect_err("a reversed scalar range is rejected");

    assert_eq!(native_error.code, vm_error.code);
    assert_eq!(native_error.code, "R_INDEX_BOUNDS");
    assert_eq!(native.native_deoptimizations(), 1);
    let site = &native.native_deoptimization_sites()[0];
    assert_eq!(site["function"], "invalid_slice");
    assert_eq!(site["operation"], "TextSlice");
    assert_eq!(site["line"], 2);
}

#[test]
fn native_text_slice_preserves_allocation_charges_and_full_slice_reuse() {
    let source = "fn full_slice() -> Text:\n    return text_slice(\"a🙂é\", 0, 3)\n\nfn partial_slice() -> Text:\n    return text_slice(\"a🙂é\", 1, 3)\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile("native-text-slice-allocation.gu", source).unwrap();
    // The full slice reuses the shared literal and allocates nothing; the partial slice
    // allocates a new text, one byte more than the limit allows.
    let partial_allocation = std::mem::size_of::<String>() + "🙂é".len();
    let policy = HostPolicy {
        allocation_limit_bytes: partial_allocation - 1,
        ..HostPolicy::default()
    };
    let mut native = Runtime::new()
        .load(artifact.clone(), policy.clone())
        .unwrap();
    let mut vm = Runtime::vm_only().load(artifact, policy).unwrap();

    let expected = Value::Text("a🙂é".into());
    assert_eq!(native.invoke("full_slice", vec![]).unwrap(), expected);
    assert_eq!(vm.invoke("full_slice", vec![]).unwrap(), expected);

    let native_error = native.invoke("partial_slice", vec![]).unwrap_err();
    let vm_error = vm.invoke("partial_slice", vec![]).unwrap_err();
    assert_eq!(native_error.code, vm_error.code);
    assert_eq!(native_error.code, "R_ALLOCATION_LIMIT");
    assert!(native
        .native_deoptimization_sites()
        .as_array()
        .unwrap()
        .iter()
        .any(|site| { site["function"] == "partial_slice" && site["operation"] == "TextSlice" }));
}

#[test]
fn native_record_call_results_survive_later_native_calls() {
    let source = "record InstantLike:\n    value: Int\n\nfn make_instant() -> InstantLike:\n    return InstantLike(123)\n\nfn format_instant(value: InstantLike) -> Text:\n    let ignored = value.value\n    return text_slice(\"2026-10-30T00:00:00Z\", 0, 10)\n\nfn main() -> Int:\n    let instant = make_instant()\n    let formatted = format_instant(instant)\n    if formatted == \"2026-10-30\":\n        return instant.value\n    return 0\n";
    let artifact = compile("native-record-call-lifetime.gu", source)
        .expect("native record result lifetime source compiles");
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .expect("native runtime loads the artifact");
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .expect("VM runtime loads the artifact");

    assert_eq!(
        native.invoke("main", vec![]).expect("native program runs"),
        Value::Int(BigInt::from_i64(123))
    );
    assert_eq!(
        vm.invoke("main", vec![]).expect("VM program runs"),
        Value::Int(BigInt::from_i64(123))
    );
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

#[test]
fn vm_result_option_load_fast_paths_preserve_results_and_report_hits() {
    let source = "fn make_result(value: Int) -> Result[Int, Text]:\n    if value > 0:\n        return Ok(value)\n    return Err(\"no\")\n\nfn make_option(value: Int) -> Option[Int]:\n    if value > 0:\n        return Some(value)\n    return None\n\nfn main() -> Int:\n    let result = make_result(42)\n    print(is_ok(result))\n    if is_ok(result):\n        print(unwrap(result))\n    let option = make_option(7)\n    print(is_some(option))\n    if is_some(option):\n        print(unwrap(option))\n    let failed = make_result(-1)\n    print(is_ok(failed))\n    let absent = make_option(0)\n    print(is_some(absent))\n    return 0\n";
    let artifact =
        compile("result-option-load-fast-path.gu", source).expect("Result/Option source compiles");
    let mut instance = Runtime::vm_only()
        .with_runtime_profile()
        .load(artifact.clone(), HostPolicy::default())
        .expect("runtime loads");

    instance.invoke("main", vec![]).expect("program runs");

    let expected_output = "true\n42\ntrue\n7\nfalse\nfalse\n";
    assert_eq!(instance.take_output(), expected_output);
    let profile = instance.runtime_profile().expect("profile enabled");
    assert_eq!(
        profile["vm_interpreter"]["result_option_probe_fast_paths"].as_u64(),
        Some(6)
    );
    assert_eq!(
        profile["vm_interpreter"]["result_option_unwrap_fast_paths"].as_u64(),
        Some(2)
    );
    for opcode in ["IsOk", "IsSome", "Unwrap"] {
        assert!(
            profile["opcode_transition_counts"][format!("main::Load->{opcode}")]
                .as_u64()
                .unwrap()
                > 0
        );
    }
    let exact_instruction_limit = profile["vm_interpreter"]["instructions"]
        .as_u64()
        .expect("profile reports executed instructions");
    let mut exact = Runtime::vm_only()
        .load(
            artifact.clone(),
            HostPolicy {
                instruction_limit: exact_instruction_limit,
                ..HostPolicy::default()
            },
        )
        .expect("runtime loads with exact fuel");
    exact
        .invoke("main", vec![])
        .expect("exact fuel is sufficient");
    assert_eq!(exact.take_output(), expected_output);

    let mut one_short = Runtime::vm_only()
        .load(
            artifact,
            HostPolicy {
                instruction_limit: exact_instruction_limit - 1,
                ..HostPolicy::default()
            },
        )
        .expect("runtime loads with one less fuel unit");
    assert_eq!(
        one_short
            .invoke("main", vec![])
            .expect_err("one less fuel unit fails")
            .code,
        "R_STEP_LIMIT"
    );
}

#[test]
fn vm_record_field_load_fast_path_preserves_values_and_fuel() {
    let source = "record Pair:\n    left: Int\n    right: Text\n\nfn main() -> Int:\n    let pair = Pair(42, \"guion\")\n    print(pair.left)\n    print(pair.right)\n    return 0\n";
    let artifact =
        compile("record-field-load-fast-path.gu", source).expect("record field source compiles");
    let mut instance = Runtime::vm_only()
        .with_runtime_profile()
        .load(artifact.clone(), HostPolicy::default())
        .expect("runtime loads");

    instance.invoke("main", vec![]).expect("program runs");

    let expected_output = "42\nguion\n";
    assert_eq!(instance.take_output(), expected_output);
    let profile = instance.runtime_profile().expect("profile enabled");
    assert_eq!(
        profile["vm_interpreter"]["record_field_load_fast_paths"].as_u64(),
        Some(2)
    );
    assert_eq!(
        profile["opcode_transition_counts"]["main::Load->GetField"].as_u64(),
        Some(2)
    );

    let exact_instruction_limit = profile["vm_interpreter"]["instructions"]
        .as_u64()
        .expect("profile reports executed instructions");
    let mut exact = Runtime::vm_only()
        .load(
            artifact.clone(),
            HostPolicy {
                instruction_limit: exact_instruction_limit,
                ..HostPolicy::default()
            },
        )
        .expect("runtime loads with exact fuel");
    exact
        .invoke("main", vec![])
        .expect("exact fuel is sufficient");
    assert_eq!(exact.take_output(), expected_output);

    let mut one_short = Runtime::vm_only()
        .load(
            artifact,
            HostPolicy {
                instruction_limit: exact_instruction_limit - 1,
                ..HostPolicy::default()
            },
        )
        .expect("runtime loads with one less fuel unit");
    assert_eq!(
        one_short
            .invoke("main", vec![])
            .expect_err("one less fuel unit fails")
            .code,
        "R_STEP_LIMIT"
    );
}

#[test]
fn vm_text_chars_constant_concat_fast_path_preserves_output_and_fuel() {
    let source = "fn main() -> Int:\n    let at = \"🙂026-10-29Z\"\n    let characters = text_chars(at)\n    let date = characters[0] + characters[1] + characters[2] + characters[3] + characters[4] + characters[5] + characters[6] + characters[7] + characters[8] + characters[9]\n    print(date)\n    return 0\n";
    let artifact = compile("text-chars-constant-concat.gu", source)
        .expect("text character concatenation fixture compiles");
    let mut instance = Runtime::vm_only()
        .with_runtime_profile()
        .load(artifact.clone(), HostPolicy::default())
        .expect("runtime loads");

    instance.invoke("main", vec![]).expect("program runs");
    assert_eq!(instance.take_output(), "🙂026-10-29\n");
    let profile = instance.runtime_profile().expect("profile enabled");
    assert_eq!(
        profile["vm_interpreter"]["text_chars_concat_fast_paths"].as_u64(),
        Some(1)
    );
    let exact_instruction_limit = profile["vm_interpreter"]["instructions"]
        .as_u64()
        .expect("profile reports executed instructions");
    let mut exact = Runtime::vm_only()
        .load(
            artifact.clone(),
            HostPolicy {
                instruction_limit: exact_instruction_limit,
                ..HostPolicy::default()
            },
        )
        .expect("runtime loads with exact fuel");
    exact
        .invoke("main", vec![])
        .expect("exact fuel is sufficient");
    assert_eq!(exact.take_output(), "🙂026-10-29\n");

    let mut one_short = Runtime::vm_only()
        .load(
            artifact,
            HostPolicy {
                instruction_limit: exact_instruction_limit - 1,
                ..HostPolicy::default()
            },
        )
        .expect("runtime loads with one less fuel unit");
    assert_eq!(
        one_short
            .invoke("main", vec![])
            .expect_err("one less fuel unit fails")
            .code,
        "R_STEP_LIMIT"
    );

    let short_source = "fn main() -> Int:\n    let at = \"abc\"\n    let characters = text_chars(at)\n    print(characters[0] + characters[1] + characters[2] + characters[3])\n    return 0\n";
    let short_artifact = compile("text-chars-short-fallback.gu", short_source)
        .expect("short text fallback fixture compiles");
    let mut short = Runtime::vm_only()
        .with_runtime_profile()
        .load(short_artifact, HostPolicy::default())
        .expect("runtime loads");
    let error = short
        .invoke("main", vec![])
        .expect_err("short input preserves the original list index error");
    assert_eq!(error.code, "R_INDEX_BOUNDS");
    assert!(error.message.contains("list index 3"));
    assert_eq!(
        short.runtime_profile().unwrap()["vm_interpreter"]["text_chars_concat_fast_paths"].as_u64(),
        Some(0)
    );

    let unicode_source = "fn main() -> Int:\n    let at = \"🙂123456789\"\n    let characters = text_chars(at)\n    print(characters[0] + characters[1] + characters[2] + characters[3] + characters[4] + characters[5] + characters[6] + characters[7] + characters[8] + characters[9])\n    return 0\n";
    let unicode_artifact = compile("text-chars-unicode-limit.gu", unicode_source)
        .expect("Unicode text character fixture compiles");
    let mut unicode = Runtime::vm_only()
        .with_runtime_profile()
        .load(
            unicode_artifact,
            HostPolicy {
                max_collection_items: 10,
                ..HostPolicy::default()
            },
        )
        .expect("runtime loads with a scalar collection limit");
    unicode
        .invoke("main", vec![])
        .expect("ten Unicode scalars fit the collection limit despite their byte length");
    assert_eq!(unicode.take_output(), "🙂123456789\n");
    assert_eq!(
        unicode.runtime_profile().unwrap()["vm_interpreter"]["text_chars_concat_fast_paths"]
            .as_u64(),
        Some(1)
    );

    let oversized_source = "fn main() -> Int:\n    let at = \"abc\"\n    let characters = text_chars(at)\n    print(characters[0] + characters[1] + characters[2])\n    return 0\n";
    let oversized_artifact = compile("text-chars-collection-limit.gu", oversized_source)
        .expect("collection limit fixture compiles");
    let mut oversized = Runtime::vm_only()
        .with_runtime_profile()
        .load(
            oversized_artifact,
            HostPolicy {
                max_collection_items: 2,
                ..HostPolicy::default()
            },
        )
        .expect("runtime loads with a smaller collection limit");
    assert_eq!(
        oversized
            .invoke("main", vec![])
            .expect_err("TextChars still checks the full scalar collection limit")
            .code,
        "R_COLLECTION_LIMIT"
    );
    assert_eq!(
        oversized.runtime_profile().unwrap()["vm_interpreter"]["text_chars_concat_fast_paths"]
            .as_u64(),
        Some(0)
    );
}

#[test]
fn vm_text_chars_concat_fast_path_preserves_allocation_limit_outcomes() {
    let fast_source = "fn main() -> Int:\n    let at = \"abcdef\"\n    let characters = text_chars(at)\n    let date = characters[0] + characters[1] + characters[2]\n    print(date)\n    return 0\n";
    let fallback_source = "fn main() -> Int:\n    let at = \"abcdef\"\n    let characters = text_chars(at)\n    let date = characters[0] + characters[1] + characters[2] + \"\"\n    print(date)\n    return 0\n";
    let fast_artifact = compile("text-chars-allocation-fast.gu", fast_source)
        .expect("fast-path allocation fixture compiles");
    let fallback_artifact = compile("text-chars-allocation-fallback.gu", fallback_source)
        .expect("fallback allocation fixture compiles");

    // The fallback's extra `+ ""` shifts its charges after byte 505, so limits between 506 and
    // 587 legitimately stop the two programs at different points.
    for allocation_limit_bytes in [0, 100, 256, 505, 1024, 4096] {
        let run = |artifact: &guion_model::Artifact| {
            let mut instance = Runtime::vm_only()
                .load(
                    artifact.clone(),
                    HostPolicy {
                        allocation_limit_bytes,
                        ..HostPolicy::default()
                    },
                )
                .expect("runtime loads with allocation limit");
            match instance.invoke("main", vec![]) {
                Ok(_) => Ok(instance.take_output()),
                Err(error) => Err(error),
            }
        };

        assert_eq!(
            run(&fast_artifact),
            run(&fallback_artifact),
            "fast path differs from the ordinary bytecode path at an allocation limit of {allocation_limit_bytes} bytes"
        );
    }
}

#[test]
fn vm_constant_list_index_load_fast_path_preserves_values_and_fuel() {
    let source = "fn main() -> Int:\n    let characters = text_chars(\"guion\")\n    print(characters[0])\n    print(characters[4])\n    return 0\n";
    let artifact = compile("constant-list-index-load-fast-path.gu", source)
        .expect("constant list index source compiles");
    let mut instance = Runtime::vm_only()
        .with_runtime_profile()
        .load(artifact.clone(), HostPolicy::default())
        .expect("runtime loads");

    instance.invoke("main", vec![]).expect("program runs");

    let expected_output = "g\nn\n";
    assert_eq!(instance.take_output(), expected_output);
    let profile = instance.runtime_profile().expect("profile enabled");
    assert_eq!(
        profile["vm_interpreter"]["list_index_load_fast_paths"].as_u64(),
        Some(2)
    );
    assert_eq!(
        profile["opcode_transition_counts"]["main::Load->Const"].as_u64(),
        Some(2)
    );
    assert_eq!(
        profile["opcode_transition_counts"]["main::Const->Index"].as_u64(),
        Some(2)
    );

    let exact_instruction_limit = profile["vm_interpreter"]["instructions"]
        .as_u64()
        .expect("profile reports executed instructions");
    let mut exact = Runtime::vm_only()
        .load(
            artifact.clone(),
            HostPolicy {
                instruction_limit: exact_instruction_limit,
                ..HostPolicy::default()
            },
        )
        .expect("runtime loads with exact fuel");
    exact
        .invoke("main", vec![])
        .expect("exact fuel is sufficient");
    assert_eq!(exact.take_output(), expected_output);

    let mut one_short = Runtime::vm_only()
        .load(
            artifact,
            HostPolicy {
                instruction_limit: exact_instruction_limit - 1,
                ..HostPolicy::default()
            },
        )
        .expect("runtime loads with one less fuel unit");
    assert_eq!(
        one_short
            .invoke("main", vec![])
            .expect_err("one less fuel unit fails")
            .code,
        "R_STEP_LIMIT"
    );
}

#[test]
fn vm_profile_reports_dynamic_adjacent_opcode_transitions_by_function() {
    let source = "fn main() -> Int:\n    let total = 1 + 2\n    return total\n";
    let artifact = compile("opcode-transition-profile.gu", source)
        .expect("opcode transition fixture compiles");
    let mut instance = Runtime::vm_only()
        .with_runtime_profile()
        .load(artifact, HostPolicy::default())
        .expect("runtime loads the artifact");

    instance.invoke("main", vec![]).expect("program runs");

    let profile = instance.runtime_profile().expect("profile is enabled");
    let transitions = &profile["opcode_transition_counts"];
    assert_eq!(transitions["main::Const->Const"].as_u64(), Some(1));
    assert_eq!(transitions["main::Const->Add"].as_u64(), Some(1));
    assert_eq!(transitions["main::Add->Store"].as_u64(), Some(1));
    assert_eq!(transitions["main::Store->Load"].as_u64(), Some(1));
    assert_eq!(transitions["main::Load->Return"].as_u64(), Some(1));
}

#[test]
fn vm_profile_reports_exact_opcode_counts_for_a_literal_return() {
    let artifact = compile("opcode-profile.gu", "fn main() -> Int:\n    return 7\n")
        .expect("opcode profile fixture compiles");
    let mut instance = Runtime::vm_only()
        .with_runtime_profile()
        .load(artifact, HostPolicy::default())
        .expect("runtime loads the artifact");

    instance.invoke("main", vec![]).expect("program runs");

    let profile = instance.runtime_profile().expect("profile is enabled");
    assert_eq!(profile["opcode_executions"]["Const"].as_u64(), Some(1));
    assert_eq!(profile["opcode_executions"]["Return"].as_u64(), Some(1));
    assert_eq!(profile["vm_interpreter"]["instructions"].as_u64(), Some(2));
    assert_eq!(profile["vm_interpreter"]["frames"].as_u64(), Some(1));
    assert_eq!(
        profile["vm_interpreter"]["sampled_frames"].as_u64(),
        Some(1)
    );
    assert_eq!(
        profile["vm_interpreter"]["sampled_instructions"].as_u64(),
        Some(2)
    );
}

#[test]
fn vm_control_flow_preserves_opcode_profile_and_exact_fuel() {
    let source = "fn identity(value: Int) -> Int:\n    return value\n\nfn main() -> Int:\n    if true:\n        return identity(7)\n    return 0\n";
    let artifact = compile("vm-control-flow-fastpath.gu", source)
        .expect("control-flow fastpath fixture compiles");
    let mut profiled = Runtime::vm_only()
        .with_runtime_profile()
        .load(artifact.clone(), HostPolicy::default())
        .expect("runtime loads the artifact");

    assert_eq!(
        profiled.invoke("main", vec![]).expect("program runs"),
        Value::Int(BigInt::from_i64(7))
    );
    let profile = profiled.runtime_profile().expect("profile is enabled");
    assert_eq!(profile["opcode_executions"]["Call"].as_u64(), Some(1));
    assert_eq!(profile["opcode_executions"]["Return"].as_u64(), Some(2));
    assert_eq!(
        profile["opcode_executions"]["JumpIfFalse"].as_u64(),
        Some(1)
    );
    assert_eq!(
        profile["dynamic_guest_conditional_branch_executions"].as_u64(),
        Some(1)
    );
    let exact_instruction_limit = profile["vm_interpreter"]["instructions"]
        .as_u64()
        .expect("profile reports executed instructions");

    let mut exact = Runtime::vm_only()
        .load(
            artifact.clone(),
            HostPolicy {
                instruction_limit: exact_instruction_limit,
                ..HostPolicy::default()
            },
        )
        .expect("runtime loads with exact fuel");
    assert_eq!(
        exact
            .invoke("main", vec![])
            .expect("exact fuel is sufficient"),
        Value::Int(BigInt::from_i64(7))
    );

    let mut one_short = Runtime::vm_only()
        .load(
            artifact,
            HostPolicy {
                instruction_limit: exact_instruction_limit - 1,
                ..HostPolicy::default()
            },
        )
        .expect("runtime loads with one less fuel unit");
    assert_eq!(
        one_short
            .invoke("main", vec![])
            .expect_err("one less fuel unit fails")
            .code,
        "R_STEP_LIMIT"
    );
}

#[test]
fn vm_profile_attributes_calls_to_the_called_function() {
    let source = "fn twice(value: Int) -> Int:\n    return value + value\n\nfn main() -> Int:\n    return twice(4)\n";
    let artifact = compile("function-call-profile.gu", source).expect("call profile compiles");
    let mut instance = Runtime::vm_only()
        .with_runtime_profile()
        .load(artifact, HostPolicy::default())
        .expect("runtime loads the artifact");

    assert_eq!(
        instance.invoke("main", vec![]).expect("program runs"),
        Value::Int(BigInt::from_i64(8))
    );
    let profile = instance.runtime_profile().expect("profile is enabled");
    assert_eq!(profile["function_call_counts"]["twice"].as_u64(), Some(1));
}

#[test]
fn vm_profile_attributes_sampled_guest_time_by_function_and_separates_host_time() {
    let source = r#"fn safe_text(value: Json) -> Result[Text, JsonError]:
    return json_try_text(value)

fn main() -> Int:
    let value = unwrap(json_parse("\"hello\""))
    let text = unwrap(safe_text(value))
    var index = 0
    while index < 3:
        set index = index + 1
    print(text)
    return index
"#;
    let artifact = compile("function-time-profile.gu", source)
        .expect("function-time profile fixture compiles");
    let mut instance = Runtime::vm_only()
        .with_runtime_profile()
        .load(artifact, HostPolicy::default())
        .expect("runtime loads the artifact");

    assert_eq!(
        instance.invoke("main", vec![]).expect("program runs"),
        Value::Int(BigInt::from_i64(3))
    );
    assert_eq!(instance.take_output(), "hello\n");

    let profile = instance.runtime_profile().expect("profile is enabled");
    let main = &profile["vm_interpreter"]["sampled_functions"]["main"];
    assert!(main["sampled_frames"].as_u64().unwrap() > 0);
    let wall = main["sampled_wall_nanoseconds"].as_u64().unwrap();
    let host = main["sampled_host_nanoseconds"].as_u64().unwrap();
    let child = main["sampled_child_nanoseconds"].as_u64().unwrap();
    let guest = main["sampled_guest_nanoseconds"].as_u64().unwrap();
    assert!(host > 0, "JSON host operations should be timed separately");
    assert_eq!(child, 0, "inlined forwarders have no child frames");
    assert!(
        guest > 0,
        "VM execution should retain exclusive sampled time"
    );
    assert_eq!(wall, host + child + guest);
    assert_eq!(main["sampled_loop_backedges"].as_u64(), Some(3));
    assert_eq!(
        profile["function_loop_backedge_counts"]["main"].as_u64(),
        Some(3)
    );
}

#[test]
fn sampled_runtime_profile_keeps_attribution_without_exact_opcode_maps() {
    let source = "fn main() -> Int:\n    let value = unwrap(json_parse(\"\\\"sample\\\"\"))\n    print(unwrap(json_try_text(value)))\n    return 0\n";
    let artifact = compile("sampled-profile.gu", source).expect("profile fixture compiles");
    let mut instance = Runtime::vm_only()
        .with_sampled_runtime_profile()
        .load(artifact, HostPolicy::default())
        .expect("runtime loads the artifact");

    assert_eq!(
        instance.invoke("main", vec![]).expect("program runs"),
        Value::Int(BigInt::from_i64(0))
    );
    assert_eq!(instance.take_output(), "sample\n");

    let profile = instance.runtime_profile().expect("profile is enabled");
    assert_eq!(profile["profile_detail"].as_str(), Some("sampled"));
    assert!(
        profile["vm_interpreter"]["sampled_functions"]["main"]["sampled_frames"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(profile["vm_interpreter"]["instructions"].is_null());
    assert!(
        profile["vm_interpreter"]["sampled_instructions"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(profile["opcode_executions"].as_object().unwrap().len(), 0);
    assert_eq!(
        profile["function_call_counts"].as_object().unwrap().len(),
        0
    );
    assert_eq!(profile["host_operations"]["timings_are_sampled"], true);
    assert!(
        profile["host_operations"]["counts"]["JsonParse"]
            .as_u64()
            .unwrap()
            > 0
    );
}

#[test]
fn inlined_json_forwarders_run_as_direct_opcodes() {
    let source = "fn safe_text(value: Json) -> Result[Text, JsonError]:\n    return json_try_text(value)\n\nfn main() -> Int:\n    let value = unwrap(json_parse(\"\\\"hello\\\"\"))\n    let result = safe_text(value)\n    assert(is_ok(result))\n    return 0\n";
    let artifact = compile("fast-json-forwarder.gu", source).expect("forwarder compiles");
    let mut instance = Runtime::vm_only()
        .with_runtime_profile()
        .load(artifact, HostPolicy::default())
        .expect("runtime loads the artifact");

    assert_eq!(
        instance.invoke("main", vec![]).expect("program runs"),
        Value::Int(BigInt::from_i64(0))
    );
    let profile = instance.runtime_profile().expect("profile is enabled");
    assert!(profile["function_call_counts"]["safe_text"].is_null());
    assert_eq!(
        profile["opcode_executions"]["JsonTryText"].as_u64(),
        Some(1)
    );
    assert_eq!(profile["vm_interpreter"]["frames"].as_u64(), Some(1));
}

#[test]
fn inlined_json_forwarder_needs_no_call_depth() {
    let source = "fn safe_text(value: Json) -> Result[Text, JsonError]:\n    return json_try_text(value)\n\nfn main() -> Int:\n    let value = unwrap(json_parse(\"\\\"hello\\\"\"))\n    let result = safe_text(value)\n    return 0\n";
    let artifact = compile("fast-json-forwarder-depth.gu", source).expect("forwarder compiles");
    let policy = HostPolicy {
        call_depth_limit: 1,
        ..HostPolicy::default()
    };
    let mut instance = Runtime::vm_only()
        .load(artifact, policy)
        .expect("runtime loads the artifact");

    assert_eq!(
        instance
            .invoke("main", vec![])
            .expect("the inlined helper needs no extra call depth"),
        Value::Int(BigInt::from_i64(0))
    );
}

#[test]
fn inlined_time_host_forwarders_run_as_direct_opcodes() {
    let source = "import \"std:time\"\nimport \"std:types\"\n\nfn parse_alias(value: Text) -> Result[Instant, FileError]:\n    return parse_timestamp(value)\n\nfn format_alias(value: Instant) -> Text:\n    return format_utc(value)\n\nfn main() -> Int:\n    let instant = unwrap(parse_alias(\"2026-10-28T12:34:56Z\"))\n    print(format_alias(instant))\n    return 0\n";
    let project_root =
        std::env::temp_dir().join(format!("guion-time-forwarder-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&project_root);
    std::fs::create_dir_all(&project_root).expect("temporary project directory is created");
    let entry = project_root.join("main.gu");
    std::fs::write(&entry, source).expect("temporary project source is written");
    let stdlib_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib");
    let artifact = compile_project_with_roots(
        &entry,
        &project_root,
        Some(&stdlib_root),
        &std::collections::HashMap::new(),
    )
    .expect("time forwarders compile");
    let mut instance = Runtime::vm_only()
        .with_runtime_profile()
        .load(artifact, HostPolicy::default())
        .expect("runtime loads the artifact");

    assert_eq!(
        instance.invoke("main", vec![]).expect("program runs"),
        Value::Int(BigInt::from_i64(0))
    );
    assert_eq!(instance.take_output(), "2026-10-28T12:34:56Z\n");
    let profile = instance.runtime_profile().expect("profile is enabled");
    assert!(profile["function_call_counts"]["parse_timestamp"].is_null());
    assert!(profile["function_call_counts"]["format_utc"].is_null());
    assert_eq!(
        profile["opcode_executions"]["TimeParseTimestamp"].as_u64(),
        Some(1)
    );
    assert_eq!(
        profile["opcode_executions"]["TimeFormatUtc"].as_u64(),
        Some(1)
    );
    assert_eq!(profile["vm_interpreter"]["frames"].as_u64(), Some(3));
    std::fs::remove_dir_all(&project_root).expect("temporary project is removed");
}

#[test]
fn tiny_option_forwarders_preserve_constructors_and_allocation_charges() {
    let source = "fn some_value(value: Int) -> Option[Int]:\n    return Some(value)\n\nfn no_value() -> Option[Int]:\n    return None\n\nfn main() -> Int:\n    let present = some_value(7)\n    let absent = no_value()\n    assert(is_some(present))\n    assert(not is_some(absent))\n    return 0\n";
    let artifact = compile("fast-option-forwarder.gu", source).expect("option forwarders compile");
    let mut instance = Runtime::vm_only()
        .with_runtime_profile()
        .load(artifact, HostPolicy::default())
        .expect("runtime loads the artifact");

    assert_eq!(
        instance.invoke("main", vec![]).expect("program runs"),
        Value::Int(BigInt::from_i64(0))
    );
    let profile = instance.runtime_profile().expect("profile is enabled");
    assert_eq!(
        profile["function_call_counts"]["some_value"].as_u64(),
        Some(1)
    );
    assert_eq!(
        profile["function_call_counts"]["no_value"].as_u64(),
        Some(1)
    );
    assert_eq!(profile["opcode_executions"]["MakeSome"].as_u64(), Some(1));
    assert_eq!(profile["opcode_executions"]["MakeNone"].as_u64(), Some(1));
    assert!(
        profile["estimated_guest_allocation_budget_charges"]["estimated_gross_bytes"]
            .as_u64()
            .unwrap()
            >= std::mem::size_of::<Value>() as u64
    );
}

#[test]
fn runtime_profile_separates_json_parse_metrics_and_host_time() {
    let source = "fn main() -> Int:\n    let parsed = unwrap(json_parse(\"{\\\"value\\\":7}\"))\n    return json_as_int(json_get(parsed, \"value\"))\n";
    let artifact = compile("json-phase-profile.gu", source).expect("JSON profile fixture compiles");
    let mut instance = Runtime::vm_only()
        .with_runtime_profile()
        .load(artifact, HostPolicy::default())
        .expect("runtime loads the artifact");

    instance.invoke("main", vec![]).expect("program runs");

    let profile = instance.runtime_profile().expect("profile is enabled");
    assert_eq!(profile["opcode_executions"]["JsonParse"].as_u64(), Some(1));
    assert_eq!(
        profile["host_operations"]["counts"]["JsonParse"].as_u64(),
        Some(1)
    );
    assert!(
        profile["host_operations"]["inclusive_nanoseconds"]["JsonParse"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(
        profile["json_phase_nanoseconds"]["parse_and_reserved_key_handling"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(
        profile["json_phase_nanoseconds"]["post_parse_metrics_traversal"]
            .as_u64()
            .unwrap()
            > 0
    );
}

#[test]
fn enabling_runtime_profile_preserves_instruction_limit_failures() {
    let artifact = compile("profile-limits.gu", "fn main() -> Int:\n    return 7\n")
        .expect("limit profile fixture compiles");
    let policy = HostPolicy {
        instruction_limit: 1,
        ..HostPolicy::default()
    };

    let mut ordinary = Runtime::vm_only()
        .load(artifact.clone(), policy.clone())
        .expect("ordinary runtime loads");
    let ordinary_error = ordinary
        .invoke("main", vec![])
        .expect_err("one step is insufficient");
    let mut profiled = Runtime::vm_only()
        .with_runtime_profile()
        .load(artifact, policy)
        .expect("profiled runtime loads");
    let profiled_error = profiled
        .invoke("main", vec![])
        .expect_err("one step is insufficient");

    assert_eq!(ordinary_error.code, profiled_error.code);
    assert_eq!(ordinary_error.message, profiled_error.message);
    assert!(profiled.runtime_profile().is_some());
}

#[test]
fn nested_map_update_reuses_unique_parent_and_child_ownership() {
    let source = "fn main() -> Int:\n    var groups: Map[Text, Map[Text, Int]] = {\"s\": {}}\n    set groups = map_set(groups, \"s\", map_set(groups[\"s\"], \"d\", 2))\n    print(groups[\"s\"][\"d\"])\n    return 0\n";
    let artifact = compile("nested-map-update.gu", source).expect("nested update compiles");
    let mut instance = Runtime::vm_only()
        .with_runtime_profile()
        .load(artifact, HostPolicy::default())
        .expect("runtime loads the artifact");

    instance.invoke("main", vec![]).expect("program runs");

    assert_eq!(instance.take_output(), "2\n");
    let profile = instance.runtime_profile().expect("profile is enabled");
    assert_eq!(profile["map_set"]["copy_on_write_copies"].as_u64(), Some(0));
    assert!(profile["map_set"]["in_place_updates"].as_u64().unwrap() >= 1);
}

#[test]
fn nested_map_update_keeps_existing_parent_alias_unchanged() {
    let source = "fn main() -> Int:\n    var groups: Map[Text, Map[Text, Int]] = {\"s\": {\"d\": 1}}\n    let original = groups\n    set groups = map_set(groups, \"s\", map_set(groups[\"s\"], \"d\", 2))\n    print(original[\"s\"][\"d\"])\n    print(groups[\"s\"][\"d\"])\n    return 0\n";
    let artifact = compile("nested-map-alias.gu", source).expect("alias fixture compiles");
    let mut instance = Runtime::vm_only()
        .with_runtime_profile()
        .load(artifact, HostPolicy::default())
        .expect("runtime loads the artifact");

    instance.invoke("main", vec![]).expect("program runs");

    assert_eq!(instance.take_output(), "1\n2\n");
    let profile = instance.runtime_profile().expect("profile is enabled");
    assert!(profile["map_set"]["copy_on_write_copies"].as_u64().unwrap() > 0);
}

#[test]
fn child_map_assignment_then_parent_writeback_avoids_the_intermediate_copy() {
    let source = "fn main() -> Int:\n    var groups: Map[Text, Map[Text, Int]] = {\"s\": {\"d\": 1}}\n    var by_date: Map[Text, Int] = groups[\"s\"]\n    if map_has(by_date, \"d\"):\n        set by_date = map_set(by_date, \"d\", 2)\n    else:\n        set by_date = map_set(by_date, \"d\", 3)\n    set groups = map_set(groups, \"s\", by_date)\n    print(groups[\"s\"][\"d\"])\n    return 0\n";
    let artifact = compile("nested-map-writeback.gu", source).expect("nested writeback compiles");
    let mut instance = Runtime::vm_only()
        .with_runtime_profile()
        .load(artifact, HostPolicy::default())
        .expect("runtime loads the artifact");

    instance.invoke("main", vec![]).expect("program runs");

    assert_eq!(instance.take_output(), "2\n");
    let profile = instance.runtime_profile().expect("profile is enabled");
    assert_eq!(profile["map_set"]["operations"].as_u64(), Some(2));
    assert_eq!(profile["map_set"]["copy_on_write_copies"].as_u64(), Some(0));
}

#[test]
fn child_map_assignment_keeps_intermediate_parent_observations_immutable() {
    let source = "fn main() -> Int:\n    var groups: Map[Text, Map[Text, Int]] = {\"s\": {\"d\": 1}}\n    var by_date: Map[Text, Int] = groups[\"s\"]\n    set by_date = map_set(by_date, \"d\", 2)\n    print(groups[\"s\"][\"d\"])\n    set groups = map_set(groups, \"s\", by_date)\n    print(groups[\"s\"][\"d\"])\n    return 0\n";
    let artifact = compile("nested-map-writeback-observed.gu", source)
        .expect("observed nested writeback compiles");
    let mut instance = Runtime::vm_only()
        .with_runtime_profile()
        .load(artifact, HostPolicy::default())
        .expect("runtime loads the artifact");

    instance.invoke("main", vec![]).expect("program runs");

    assert_eq!(instance.take_output(), "1\n2\n");
    let profile = instance.runtime_profile().expect("profile is enabled");
    assert!(profile["map_set"]["copy_on_write_copies"].as_u64().unwrap() > 0);
}

#[test]
fn child_map_assignment_keeps_shared_parent_aliases_immutable() {
    let source = "fn main() -> Int:\n    var groups: Map[Text, Map[Text, Int]] = {\"s\": {\"d\": 1}}\n    let original = groups\n    var by_date: Map[Text, Int] = groups[\"s\"]\n    set by_date = map_set(by_date, \"d\", 2)\n    set groups = map_set(groups, \"s\", by_date)\n    print(original[\"s\"][\"d\"])\n    print(groups[\"s\"][\"d\"])\n    return 0\n";
    let artifact = compile("nested-map-writeback-alias.gu", source)
        .expect("aliased nested writeback compiles");
    let mut instance = Runtime::vm_only()
        .with_runtime_profile()
        .load(artifact, HostPolicy::default())
        .expect("runtime loads the artifact");

    instance.invoke("main", vec![]).expect("program runs");

    assert_eq!(instance.take_output(), "1\n2\n");
    let profile = instance.runtime_profile().expect("profile is enabled");
    assert!(profile["map_set"]["copy_on_write_copies"].as_u64().unwrap() > 0);
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
fn bounded_byte_construction_primitives_require_format_34_or_newer() {
    let source = "fn main() -> Int:\n    let raw = bytes_from_ints([0, 255])\n    let utf8 = bytes_from_text(\"é\")\n    let joined = bytes_join([raw, utf8])\n    print(len(joined))\n    print(float64_bits(1.0))\n    return 0\n";
    let artifact =
        compile("artifact-byte-primitives.gu", source).expect("byte construction source compiles");
    assert_eq!(artifact.format_version, Artifact::CURRENT_VERSION);
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
        main_instructions <= 477,
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
fn map_record_updates_preserve_map_and_record_aliases() {
    let source = "record Summary:\n    count: Int\n    oldest: Int\n    newest: Int\n\nfn main() -> Int:\n    var groups: Map[Text, Summary] = {}\n    set groups = map_set(groups, \"svc:date\", Summary(1, 10, 10))\n    let previous = groups\n    set groups = map_record_set(groups, \"svc:date\", \"count\", 2)\n    set groups = map_record_set(groups, \"svc:date\", \"newest\", 20)\n    assert(map_record_get(previous, \"svc:date\", \"count\") == 1)\n    assert(map_record_get(previous, \"svc:date\", \"newest\") == 10)\n    assert(map_record_get(groups, \"svc:date\", \"count\") == 2)\n    assert(map_record_get(groups, \"svc:date\", \"newest\") == 20)\n    return len(previous) + len(groups)\n";
    let artifact = compile("map-record-update-alias.gu", source)
        .expect("typed map record update source compiles");
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .unwrap();

    let expected = Value::Int(BigInt::from_usize(2));
    assert_eq!(native.invoke("main", vec![]).unwrap(), expected);
    assert_eq!(vm.invoke("main", vec![]).unwrap(), expected);
}

#[test]
fn same_local_map_record_updates_reuse_unique_ownership() {
    let source = "record Summary:\n    count: Int\n\nfn main() -> Int:\n    var groups: Map[Text, Summary] = {}\n    set groups = map_set(groups, \"svc:date\", Summary(0))\n    var index = 0\n    while index < 100:\n        let count = map_record_get(groups, \"svc:date\", \"count\")\n        set groups = map_record_set(groups, \"svc:date\", \"count\", count + 1)\n        set index = index + 1\n    return map_record_get(groups, \"svc:date\", \"count\")\n";
    let artifact = compile("map-record-update-budget.gu", source)
        .expect("typed map record update source compiles");
    assert!(
        artifact.functions[0].code.windows(2).any(|window| {
            matches!(window[0].op, Op::MapRecordSet(_, _, false))
                && matches!(window[1].op, Op::Store(_))
        }),
        "mutable record update should store back to its map local: {:?}",
        artifact.functions[0].code
    );
    let mut instance = Runtime::new()
        .with_runtime_profile()
        .load(artifact, HostPolicy::default())
        .unwrap();

    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::from_usize(100))
    );
    let update_profile = instance.runtime_profile().unwrap()["map_record_update"].clone();
    assert_eq!(update_profile["operations"], 100);
    assert_eq!(update_profile["in_place_updates"], 100, "{update_profile}");
    assert_eq!(
        update_profile["copy_on_write_copies"], 0,
        "{update_profile}"
    );
}

#[test]
fn nested_map_record_updates_preserve_shared_aliases() {
    let source = "record Summary:\n    count: Int\n\nfn main() -> Int:\n    var groups: Map[Text, Map[Text, Summary]] = {}\n    var by_date: Map[Text, Summary] = {}\n    set by_date = map_set(by_date, \"2026-10-29\", Summary(1))\n    set groups = map_set(groups, \"svc\", by_date)\n    let previous = groups\n    set groups = map_record_set(groups, \"svc\", \"2026-10-29\", \"count\", 2)\n    assert(map_record_get(previous, \"svc\", \"2026-10-29\", \"count\") == 1)\n    assert(map_record_get(groups, \"svc\", \"2026-10-29\", \"count\") == 2)\n    return 0\n";
    let artifact = compile("nested-map-record-alias.gu", source)
        .expect("nested typed map record update source compiles");
    let mut instance = Runtime::new()
        .with_runtime_profile()
        .load(artifact, HostPolicy::default())
        .unwrap();

    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::from_usize(0))
    );
    let profile = instance.runtime_profile().unwrap()["map_record_update"].clone();
    assert_eq!(profile["operations"], 1);
    assert_eq!(profile["in_place_updates"], 0);
    assert_eq!(profile["copy_on_write_copies"], 1);
    assert!(profile["estimated_map_copy_bytes"].as_u64().unwrap() > 0);
}

#[test]
fn native_map_set_accepts_native_layout_record_values() {
    let source = "record Summary:\n    count: Int\n    oldest: Int\n    newest: Int\n\nfn main() -> Int:\n    let service = text_slice(\"worker-service\", 0, 6)\n    var by_date: Map[Text, Summary] = {}\n    set by_date = map_set(by_date, service, Summary(2, 10, 20))\n    return 0\n";
    let artifact = compile("native-record-map-set.gu", source)
        .expect("native record map insertion source compiles");
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .expect("native runtime loads the artifact");
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .expect("VM runtime loads the artifact");

    let native_value = native.invoke("main", vec![]).expect("native program runs");
    let vm_value = vm.invoke("main", vec![]).expect("VM program runs");
    assert!(
        native
            .native_function_names()
            .iter()
            .any(|name| name == "main"),
        "{}",
        native.native_compile_profile()
    );
    assert_eq!(native_value, vm_value);
    assert_eq!(native.native_deoptimizations(), 0);
}

#[test]
fn same_local_nested_map_record_updates_reuse_unique_ownership() {
    let source = "record Summary:\n    count: Int\n\nfn main() -> Int:\n    var by_date: Map[Text, Summary] = {}\n    set by_date = map_set(by_date, \"2026-10-29\", Summary(0))\n    var groups: Map[Text, Map[Text, Summary]] = {}\n    set groups = map_set(groups, \"svc\", by_date)\n    set by_date = {}\n    var index = 0\n    while index < 100:\n        let count = map_record_get(groups, \"svc\", \"2026-10-29\", \"count\")\n        set groups = map_record_set(groups, \"svc\", \"2026-10-29\", \"count\", count + 1)\n        set index = index + 1\n    return map_record_get(groups, \"svc\", \"2026-10-29\", \"count\")\n";
    let artifact = compile("nested-map-record-update-unique.gu", source)
        .expect("nested typed map record update source compiles");
    assert!(artifact.functions[0].code.windows(2).any(|window| {
        matches!(window[0].op, Op::MapRecordSet(_, _, true)) && matches!(window[1].op, Op::Store(_))
    }));
    let mut instance = Runtime::new()
        .with_runtime_profile()
        .load(artifact, HostPolicy::default())
        .unwrap();

    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::from_usize(100))
    );
    let profile = instance.runtime_profile().unwrap()["map_record_update"].clone();
    assert_eq!(profile["operations"], 100);
    assert_eq!(profile["in_place_updates"], 100, "{profile}");
    assert_eq!(profile["copy_on_write_copies"], 0, "{profile}");
}

#[test]
fn map_record_update_requires_an_existing_key_and_valid_typed_field() {
    let missing_key_source = "record Summary:\n    count: Int\n\nfn main() -> Int:\n    var groups: Map[Text, Summary] = {}\n    set groups = map_set(groups, \"present\", Summary(1))\n    set groups = map_record_set(groups, \"missing\", \"count\", 2)\n    return 0\n";
    let artifact = compile("map-record-update-missing.gu", missing_key_source).unwrap();
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap_err().code,
        "R_MAP_KEY"
    );

    let invalid_field_source = "record Summary:\n    count: Int\n\nfn main() -> Int:\n    var groups: Map[Text, Summary] = {}\n    set groups = map_record_set(groups, \"key\", \"missing\", 1)\n    return 0\n";
    assert_eq!(
        compile("map-record-update-invalid-field.gu", invalid_field_source)
            .unwrap_err()
            .code,
        "E_FIELD_UNKNOWN"
    );

    let invalid_value_source = "record Summary:\n    count: Int\n\nfn main() -> Int:\n    var groups: Map[Text, Summary] = {}\n    set groups = map_record_set(groups, \"key\", \"count\", \"two\")\n    return 0\n";
    assert_eq!(
        compile("map-record-update-invalid-value.gu", invalid_value_source)
            .unwrap_err()
            .code,
        "E_TYPE_ARGUMENT"
    );
}

#[test]
fn map_set_existing_key_preserves_nonlocal_aliases() {
    let source = "fn main() -> Int:\n    let original: Map[Text, Int] = {\"one\": 1}\n    let updated = map_set(original, \"one\", 2)\n    assert(original[\"one\"] == 1)\n    assert(updated[\"one\"] == 2)\n    return len(original) + len(updated)\n";
    let artifact = compile("map-update-existing-alias.gu", source).unwrap();
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();

    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::from_usize(2))
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
fn mutable_json_object_set_is_local_and_preserves_aliases() {
    let source = "fn main() -> Int:\n    var value = json_object()\n    set value = json_set(value, \"left\", json_text(\"old\"))\n    let previous = value\n    set value = json_set(value, \"right\", json_text(\"new\"))\n    assert(json_has(previous, \"right\") == false)\n    assert(json_stringify(value) == \"{\\\"left\\\":\\\"old\\\",\\\"right\\\":\\\"new\\\"}\")\n    return 0\n";
    let artifact = compile("json-set-local.gu", source).unwrap();
    assert!(artifact.functions[0]
        .code
        .iter()
        .any(|instruction| matches!(instruction.op, Op::JsonSetLocal(_))));
    assert_eq!(artifact.format_version, Artifact::CURRENT_VERSION);
    let artifact = Artifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let mut legacy = artifact.clone();
    legacy.format_version = 40;
    assert_eq!(legacy.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");
    let mut legacy_bytes = artifact.to_bytes().unwrap();
    legacy_bytes[4..8].copy_from_slice(&40u32.to_be_bytes());
    assert_eq!(
        Artifact::from_bytes(&legacy_bytes).unwrap_err().code,
        "E_ARTIFACT_VERSION"
    );
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();

    instance.invoke("main", vec![]).unwrap();
}

#[test]
fn json_stringify_pairs_writes_sorted_escaped_rows_and_empty_arrays() {
    let source = r#"fn main() -> Int:
    let groups: Map[Text, List[Text]] = {"é\"group": ["line\nbreak", "quote\""], "alpha": [], "beta": ["café"]}
    let empty: Map[Text, List[Text]] = {}
    print(json_stringify_pairs_native(groups, "na\nme", "ru\"le"))
    print(json_stringify_pairs_native(empty, "na\nme", "ru\"le"))
    return 0
"#;
    let artifact =
        compile("json-stringify-pairs.gu", source).expect("JSON pair serializer compiles");
    assert_eq!(artifact.format_version, Artifact::CURRENT_VERSION);
    assert!(artifact.functions.iter().any(|function| {
        function
            .code
            .iter()
            .any(|instruction| matches!(instruction.op, Op::JsonStringifyPairs))
    }));
    let artifact = Artifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let mut legacy = artifact.clone();
    legacy.format_version = 43;
    assert_eq!(legacy.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");

    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    instance.invoke("main", vec![]).unwrap();

    assert_eq!(
        instance.take_output(),
        r#"[{"na\nme":"beta","ru\"le":"café"},{"na\nme":"é\"group","ru\"le":"line\nbreak"},{"na\nme":"é\"group","ru\"le":"quote\""}]
[]
"#
    );
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
fn native_args_returns_the_configured_text_values() {
    let source = "fn read_arg() -> Text:\n    return args()[1]\n\nfn main() -> Int:\n    return len(args())\n";
    let artifact = compile("native-args-list.gu", source).expect("args source compiles");
    let policy = HostPolicy::default().with_arguments(vec!["alpha".into(), "β".into()]);
    let mut native = Runtime::new()
        .load(artifact.clone(), policy.clone())
        .expect("native runtime loads");
    let mut vm = Runtime::vm_only()
        .load(artifact, policy)
        .expect("VM runtime loads");

    assert_eq!(
        native.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::from_i64(2))
    );
    let native_value = native.invoke("read_arg", vec![]).unwrap();
    assert_eq!(native_value, vm.invoke("read_arg", vec![]).unwrap());
    assert_eq!(native_value, Value::Text("β".into()));
    assert!(
        native
            .native_function_names()
            .iter()
            .any(|name| name == "main"),
        "{}",
        native.native_compile_profile()
    );
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "read_arg"));
    assert_eq!(native.native_deoptimizations(), 0);
}

#[test]
fn native_args_preserves_vm_limit_errors() {
    let source =
        "fn count_args() -> Int:\n    return len(args())\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile("native-args-limits.gu", source).expect("args source compiles");
    let cases = [
        HostPolicy {
            max_collection_items: 1,
            ..HostPolicy::default()
        },
        HostPolicy {
            allocation_limit_bytes: 0,
            ..HostPolicy::default()
        },
        HostPolicy {
            max_text_bytes: 1,
            ..HostPolicy::default()
        },
    ];

    for policy in cases {
        let policy = policy.with_arguments(vec!["alpha".into(), "β".into()]);
        let mut native = Runtime::new()
            .load(artifact.clone(), policy.clone())
            .expect("native runtime loads");
        let mut vm = Runtime::vm_only()
            .load(artifact.clone(), policy)
            .expect("VM runtime loads");

        let native_error = native
            .invoke("count_args", vec![])
            .expect_err("the guest limit rejects the arguments");
        let vm_error = vm
            .invoke("count_args", vec![])
            .expect_err("the VM limit rejects the arguments");
        assert_eq!(native_error, vm_error);
        assert!(
            native
                .native_function_names()
                .iter()
                .any(|name| name == "count_args"),
            "{}",
            native.native_compile_profile()
        );
        assert!(native.native_deoptimizations() > 0);
    }
}

#[test]
fn native_text_equality_works_with_argument_list_values() {
    let source = "fn main() -> Int:\n    if args()[0] == \"yes\":\n        return 1\n    if args()[0] != \"no\":\n        return 2\n    return 0\n";
    let artifact =
        compile("native-text-equality.gu", source).expect("text equality with arguments compiles");

    for (argument, expected) in [("yes", 1), ("no", 0), ("other", 2)] {
        let policy = HostPolicy::default().with_arguments(vec![argument.into()]);
        let mut native = Runtime::new()
            .load(artifact.clone(), policy.clone())
            .expect("native runtime loads");
        let mut vm = Runtime::vm_only()
            .load(artifact.clone(), policy)
            .expect("VM runtime loads");

        let expected = Value::Int(BigInt::from_i64(expected));
        assert_eq!(native.invoke("main", vec![]).unwrap(), expected);
        assert_eq!(
            native.invoke("main", vec![]).unwrap(),
            vm.invoke("main", vec![]).unwrap()
        );
        assert!(
            native
                .native_function_names()
                .iter()
                .any(|name| name == "main"),
            "{}",
            native.native_compile_profile()
        );
        assert_eq!(native.native_deoptimizations(), 0);
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
        ir_instructions < 505,
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
        .with_runtime_profile()
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
    let deoptimization_sites = native.native_deoptimization_sites();
    assert_eq!(deoptimization_sites.as_array().unwrap().len(), 1);
    assert_eq!(deoptimization_sites[0]["function"], "quotient");
    assert_eq!(deoptimization_sites[0]["instruction"], 2);
    assert_eq!(deoptimization_sites[0]["operation"], "FloorDivide");
    assert_eq!(deoptimization_sites[0]["line"], 2);
    assert_eq!(deoptimization_sites[0]["count"], 1);
    assert_eq!(
        native.runtime_profile().unwrap()["vm_interpreter"]["instructions"].as_u64(),
        Some(8),
        "the VM should execute the non-native caller and only the callee instructions after division"
    );
}

#[test]
fn native_side_exit_resumes_exact_vm_locals_stack_and_fuel() {
    let source = "fn carry_state(value: Int) -> Int:\n    let saved = value + 1\n    return saved + value\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile("native-side-exit-state.gu", source).unwrap();
    let argument = || vec![Value::Int(BigInt::parse("5000000000000000000").unwrap())];
    let mut native = Runtime::new()
        .with_runtime_profile()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();

    let native_result = native.invoke("carry_state", argument()).unwrap();
    assert_eq!(
        native_result,
        Value::Int(BigInt::parse("10000000000000000001").unwrap())
    );
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "carry_state"));
    assert_eq!(native.native_deoptimizations(), 1);
    assert_eq!(
        native.runtime_profile().unwrap()["vm_interpreter"]["instructions"].as_u64(),
        Some(2),
        "VM should execute only the Add and Return at and after the side-exit IP"
    );

    let mut vm = Runtime::vm_only()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    assert_eq!(native_result, vm.invoke("carry_state", argument()).unwrap());

    let mut exact = Runtime::new()
        .load(
            artifact.clone(),
            HostPolicy {
                instruction_limit: 8,
                ..HostPolicy::default()
            },
        )
        .unwrap();
    assert_eq!(
        exact.invoke("carry_state", argument()).unwrap(),
        native_result
    );
    let mut one_short = Runtime::new()
        .load(
            artifact,
            HostPolicy {
                instruction_limit: 7,
                ..HostPolicy::default()
            },
        )
        .unwrap();
    assert_eq!(
        one_short
            .invoke("carry_state", argument())
            .unwrap_err()
            .code,
        "R_STEP_LIMIT"
    );
}

#[test]
fn native_side_exit_resumes_callers_and_commits_pending_output_once() {
    let source = "fn carry_state(value: Int) -> Int:\n    let saved = value + 1\n    return saved + value\n\nfn wrapper(value: Int) -> Int:\n    print(value)\n    return carry_state(value) + 5\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile("native-side-exit-call-chain.gu", source).unwrap();
    let argument = || vec![Value::Int(BigInt::parse("5000000000000000000").unwrap())];
    let mut native = Runtime::new()
        .with_runtime_profile()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let native_result = native.invoke("wrapper", argument()).unwrap();
    let native_output = native.take_output();

    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .unwrap();
    let vm_result = vm.invoke("wrapper", argument()).unwrap();
    let vm_output = vm.take_output();

    assert_eq!(native_result, vm_result);
    assert_eq!(
        native_result,
        Value::Int(BigInt::parse("10000000000000000006").unwrap())
    );
    assert_eq!(native_output, "5000000000000000000\n");
    assert_eq!(native_output, vm_output);
    assert_eq!(
        native.runtime_profile().unwrap()["vm_interpreter"]["instructions"].as_u64(),
        Some(5),
        "the resumed child and caller should execute only their remaining instructions"
    );
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
        assert_eq!(
            instance.invoke("main", vec![]).unwrap_err().code,
            expected_code
        );
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
        .with_runtime_profile()
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
fn native_integer_negation_overflow_resumes_at_the_faulting_instruction() {
    let source =
        "fn negate(value: Int) -> Int:\n    return -value\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile("native-int-negate-overflow.gu", source).unwrap();
    let argument = || vec![Value::Int(BigInt::parse("-9223372036854775808").unwrap())];
    let mut native = Runtime::new()
        .with_runtime_profile()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let result = native.invoke("negate", argument()).unwrap();

    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .unwrap();
    assert_eq!(result, vm.invoke("negate", argument()).unwrap());
    assert_eq!(
        result,
        Value::Int(BigInt::parse("9223372036854775808").unwrap())
    );
    assert_eq!(native.native_deoptimizations(), 1);
    assert_eq!(
        native.runtime_profile().unwrap()["vm_interpreter"]["instructions"].as_u64(),
        Some(2)
    );
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
#[cfg(not(feature = "native-value-bridge"))]
#[test]
fn native_json_result_functions_compile_without_enabling_the_general_bridge() {
    let source = "fn accepts_text(value: Json) -> Bool:\n    return is_ok(json_try_text(value))\n\nfn main() -> Int:\n    let parsed = json_parse(\"\\\"guion\\\"\")\n    if is_ok(parsed):\n        if accepts_text(unwrap(parsed)):\n            return 0\n    return 1\n";
    let artifact = compile("native-json-host-bridge-opt-in.gu", source)
        .expect("mixed-value bridge fixture compiles");
    let text_json = Value::Json(SharedJson::from_serde(serde_json::json!("guion")));
    let mut runtime = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .expect("runtime loads");
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .expect("VM runtime loads");

    assert_eq!(
        runtime
            .invoke("accepts_text", vec![text_json.clone()])
            .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        runtime
            .invoke("accepts_text", vec![text_json.clone()])
            .unwrap(),
        vm.invoke("accepts_text", vec![text_json]).unwrap()
    );
    assert_eq!(runtime.native_compilations(), 1);
    assert!(runtime
        .native_function_names()
        .iter()
        .any(|name| name == "accepts_text"));
    let profile = runtime.native_compile_profile();
    let rejected = profile["rejected_functions"]
        .as_array()
        .expect("native profile reports functions rejected from compilation");
    assert!(!rejected
        .iter()
        .any(|function| { function["name"].as_str() == Some("accepts_text") }));
}

#[test]
fn native_backend_keeps_result_and_option_chains_in_one_tier_by_default() {
    let source = "fn make_result(value: Int, message: Text) -> Result[Int, Text]:\n    if value < 0:\n        return Err(message)\n    return Ok(value)\n\nfn make_option(value: Int) -> Option[Int]:\n    if value < 0:\n        return None\n    return Some(value)\n\nfn run_result(value: Int, message: Text) -> Int:\n    let result = make_result(value, message)\n    if is_ok(result):\n        return unwrap(result)\n    return -1\n\nfn run_option(value: Int) -> Int:\n    let option = make_option(value)\n    if is_some(option):\n        return unwrap(option)\n    return -1\n\nfn main() -> Int:\n    return run_option(22) + run_option(-1)\n";
    let artifact = compile("native-result-option-tier.gu", source)
        .expect("Result/Option execution-tier fixture compiles");

    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .expect("native runtime loads");
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .expect("VM runtime loads");

    let expected = Value::Int(BigInt::from_i64(21));
    assert_eq!(native.invoke("main", vec![]).unwrap(), expected);
    assert_eq!(vm.invoke("main", vec![]).unwrap(), expected);
    let result_arguments = vec![
        Value::Int(BigInt::from_i64(11)),
        Value::Text("negative".into()),
    ];
    assert_eq!(
        native
            .invoke("run_result", result_arguments.clone())
            .unwrap(),
        Value::Int(BigInt::from_i64(11))
    );
    assert_eq!(
        vm.invoke("run_result", result_arguments.clone()).unwrap(),
        Value::Int(BigInt::from_i64(11))
    );
    let error_arguments = vec![
        Value::Int(BigInt::from_i64(-1)),
        Value::Text("negative".into()),
    ];
    assert_eq!(
        native
            .invoke("run_result", error_arguments.clone())
            .unwrap(),
        Value::Int(BigInt::from_i64(-1))
    );
    assert_eq!(
        vm.invoke("run_result", error_arguments).unwrap(),
        Value::Int(BigInt::from_i64(-1))
    );
    let compiled = native.native_function_names();
    for name in [
        "make_result",
        "make_option",
        "run_result",
        "run_option",
        "main",
    ] {
        assert!(
            compiled.iter().any(|compiled_name| compiled_name == name),
            "{name} should stay in the native execution tier; compiled: {compiled:?}; profile: {}",
            native.native_compile_profile()
        );
    }
}

#[test]
fn vm_parent_keeps_host_callback_call_chain_in_the_vm_tier() {
    let source = "fn parse_ok(source: Text) -> Bool:\n    return is_ok(json_parse(source))\n\nfn main() -> Int:\n    print(\"vm parent\")\n    if parse_ok(\"{\\\"value\\\":1}\"):\n        return 0\n    return 1\n";
    let artifact = compile("vm-native-host-callback-boundary.gu", source)
        .expect("VM/native tier boundary fixture compiles");
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .expect("native runtime loads");
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .expect("VM runtime loads");

    let native_value = native.invoke("main", vec![]).unwrap();
    let native_output = native.take_output();
    let vm_value = vm.invoke("main", vec![]).unwrap();
    let vm_output = vm.take_output();
    assert_eq!(native_value, vm_value);
    assert_eq!(native_output, vm_output);
    assert_eq!(native_output, "vm parent\n");
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "parse_ok"));
    assert!(!native
        .native_function_names()
        .iter()
        .any(|name| name == "main"));
    assert_eq!(
        native.native_calls(),
        0,
        "VM child calls should not enter a native helper arena"
    );
}

#[test]
fn native_root_entry_can_execute_a_json_host_callback() {
    let source = "fn main() -> Int:\n    let parsed = json_parse(\"{\\\"value\\\":1}\")\n    if is_ok(parsed):\n        return 0\n    return 1\n";
    let artifact =
        compile("native-root-json-callback.gu", source).expect("root callback fixture compiles");
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .expect("native runtime loads");
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .expect("VM runtime loads");

    assert_eq!(
        native.invoke("main", vec![]).unwrap(),
        vm.invoke("main", vec![]).unwrap()
    );
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "main"));
    assert_eq!(
        native.native_calls(),
        1,
        "the root JIT entry should execute the callback-bearing function"
    );
}

#[test]
fn native_backend_keeps_result_error_text_literals_in_the_native_tier() {
    let source = "fn checked_result(value: Int) -> Result[Int, Text]:\n    if value % 7 == 0:\n        return Err(\"rejected\")\n    return Ok(value)\n\nfn main() -> Int:\n    let failure = checked_result(7)\n    let success = checked_result(8)\n    var index = 1\n    var checksum = 0\n    while index <= 600:\n        let current = checked_result(index)\n        if is_ok(current):\n            set checksum = checksum + unwrap(current)\n        set index = index + 1\n    if not is_ok(failure) and is_ok(success):\n        return checksum\n    return -1\n";
    let artifact = compile("native-result-error-text-literal.gu", source)
        .expect("Result error literal fixture compiles");
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .expect("native runtime loads");
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .expect("VM runtime loads");

    let expected_error = Value::ResultErr(Box::new(Value::Text("rejected".into())));
    assert_eq!(
        native
            .invoke("checked_result", vec![Value::Int(BigInt::from_i64(7))])
            .unwrap(),
        expected_error
    );
    assert_eq!(
        native.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::from_i64(154715))
    );
    assert_eq!(
        vm.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::from_i64(154715))
    );
    let compiled = native.native_function_names();
    assert!(compiled.iter().any(|name| name == "checked_result"));
    assert!(compiled.iter().any(|name| name == "main"));
}

#[test]
fn native_backend_keeps_json_result_tag_checks_in_the_native_tier_by_default() {
    let source = "fn accepts_text(value: Json) -> Bool:\n    return is_ok(json_try_text(value))\n\nfn has_text_field(value: Json) -> Bool:\n    let field = json_try_get(value, \"name\")\n    if is_ok(field):\n        return is_ok(json_try_text(unwrap(field)))\n    return false\n\nfn accepts_integer(value: Json) -> Bool:\n    return is_ok(json_try_int(value))\n\nfn get_text_field(value: Json) -> Result[Text, JsonError]:\n    let field = json_try_get(value, \"name\")\n    if is_ok(field):\n        return json_try_text(unwrap(field))\n    return Err(error(field))\n\nfn parse_integer(value: Json) -> Result[Int, JsonError]:\n    return json_try_int(value)\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile("native-json-result-tier.gu", source)
        .expect("JSON Result execution-tier fixture compiles");
    let text_json = Value::Json(SharedJson::from_serde(serde_json::json!("guion")));
    let number_json = Value::Json(SharedJson::from_serde(serde_json::json!(42)));
    let object_json = Value::Json(SharedJson::from_serde(serde_json::json!({"name": "guion"})));
    let missing_json = Value::Json(SharedJson::from_serde(serde_json::json!({})));

    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .expect("native runtime loads");
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .expect("VM runtime loads");
    for (function, argument, expected) in [
        ("accepts_text", text_json.clone(), true),
        ("accepts_text", number_json.clone(), false),
        ("has_text_field", object_json.clone(), true),
        ("has_text_field", missing_json.clone(), false),
        ("accepts_integer", number_json.clone(), true),
        ("accepts_integer", text_json.clone(), false),
    ] {
        let arguments = vec![argument];
        assert_eq!(
            native.invoke(function, arguments.clone()).unwrap(),
            Value::Bool(expected)
        );
        assert_eq!(
            vm.invoke(function, arguments).unwrap(),
            Value::Bool(expected)
        );
    }
    let expected_text = Value::ResultOk(Box::new(Value::Text("guion".into())));
    assert_eq!(
        native
            .invoke("get_text_field", vec![object_json.clone()])
            .unwrap(),
        expected_text
    );
    assert_eq!(
        native
            .invoke("get_text_field", vec![missing_json.clone()])
            .unwrap(),
        Value::ResultErr(Box::new(Value::JsonError(Box::new(
            guion_model::JsonErrorValue {
                code: "E_JSON_MISSING".into(),
                message: "JSON object has no key `name`".into(),
                line: 0,
                column: 0,
            }
        ))))
    );
    assert_eq!(
        native
            .invoke("parse_integer", vec![number_json.clone()])
            .unwrap(),
        Value::ResultOk(Box::new(Value::Int(BigInt::from_i64(42))))
    );
    assert_eq!(
        native
            .invoke("parse_integer", vec![text_json.clone()])
            .unwrap(),
        Value::ResultErr(Box::new(Value::JsonError(Box::new(
            guion_model::JsonErrorValue {
                code: "E_JSON_TYPE".into(),
                message: "expected a JSON integer".into(),
                line: 0,
                column: 0,
            }
        ))))
    );
    for (function, argument) in [
        ("get_text_field", object_json.clone()),
        ("get_text_field", missing_json.clone()),
        ("parse_integer", number_json.clone()),
        ("parse_integer", text_json.clone()),
    ] {
        assert_eq!(
            native.invoke(function, vec![argument.clone()]).unwrap(),
            vm.invoke(function, vec![argument]).unwrap()
        );
    }
    let compiled = native.native_function_names();
    for name in [
        "accepts_text",
        "has_text_field",
        "accepts_integer",
        "get_text_field",
        "parse_integer",
    ] {
        assert!(
            compiled.iter().any(|compiled_name| compiled_name == name),
            "{name} should stay in the native tier; compiled: {compiled:?}; profile: {}",
            native.native_compile_profile()
        );
    }
}

#[test]
fn native_backend_reads_json_fields_from_mixed_records_by_default() {
    let source = "record Document:\n    body: Json\n\nfn has_name(document: Document) -> Bool:\n    return is_ok(json_try_get(document.body, \"name\"))\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile("native-mixed-record-field.gu", source)
        .expect("mixed record native field fixture compiles");
    let document = |value| {
        Value::Record(
            "Document".into(),
            std::sync::Arc::new(vec![Value::Json(SharedJson::from_serde(value))]),
        )
    };
    let present = document(serde_json::json!({"name": "Guion"}));
    let missing = document(serde_json::json!({}));

    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .expect("native runtime loads");
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .expect("VM runtime loads");
    for (argument, expected) in [(present, true), (missing, false)] {
        let arguments = vec![argument];
        assert_eq!(
            native.invoke("has_name", arguments.clone()).unwrap(),
            Value::Bool(expected)
        );
        assert_eq!(
            vm.invoke("has_name", arguments).unwrap(),
            Value::Bool(expected)
        );
    }
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "has_name"));
}

#[test]
fn native_json_result_helpers_deopt_without_changing_policy_errors() {
    let source = "fn accepts_text(value: Json) -> Bool:\n    return is_ok(json_try_text(value))\n\nfn get_long_key(value: Json) -> Result[Json, JsonError]:\n    return json_try_get(value, \"long\")\n\nfn parse_integer(value: Json) -> Result[Int, JsonError]:\n    return json_try_int(value)\n\nfn parse_text(source: Text) -> Result[Json, JsonError]:\n    return json_parse(source)\n\nfn parse_long_literal() -> Result[Json, JsonError]:\n    return json_parse(\"{\\\"k\\\":1}\")\n\nfn main() -> Int:\n    let parsed = json_parse(\"123\")\n    let value = unwrap(parsed)\n    let converted = parse_integer(value)\n    return 0\n";
    let artifact = compile("native-json-result-limits.gu", source)
        .expect("JSON Result policy fixture compiles");

    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .expect("native runtime loads");
    let mut vm = Runtime::vm_only()
        .load(artifact.clone(), HostPolicy::default())
        .expect("VM runtime loads");
    for (source, expected_code) in [
        ("{\"k\":1}", None),
        ("{", Some("R_JSON_PARSE")),
        ("{\"k\":1,\"k\":2}", Some("R_JSON_DUPLICATE_KEY")),
    ] {
        let arguments = vec![Value::Text(source.into())];
        let native_value = native.invoke("parse_text", arguments.clone()).unwrap();
        let vm_value = vm.invoke("parse_text", arguments).unwrap();
        assert_eq!(native_value, vm_value);
        if let Some(expected_code) = expected_code {
            let Value::ResultErr(error) = native_value else {
                panic!("invalid JSON should be returned as a Result error")
            };
            let Value::JsonError(error) = *error else {
                panic!("JSON parse error should preserve JsonError")
            };
            assert_eq!(error.code, expected_code);
        } else {
            assert!(matches!(native_value, Value::ResultOk(_)));
        }
    }
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "parse_text"));
    assert_eq!(native.native_deoptimizations(), 0);

    let text = Value::Json(SharedJson::from_serde(serde_json::json!("x")));
    let allocation_policy = HostPolicy {
        allocation_limit_bytes: 1,
        ..HostPolicy::default()
    };
    let mut native = Runtime::new()
        .load(artifact.clone(), allocation_policy.clone())
        .expect("native runtime loads");
    let mut vm = Runtime::vm_only()
        .load(artifact.clone(), allocation_policy)
        .expect("VM runtime loads");
    let native_error = native
        .invoke("accepts_text", vec![text.clone()])
        .expect_err("text conversion must honor the allocation limit");
    let vm_error = vm
        .invoke("accepts_text", vec![text])
        .expect_err("VM text conversion must honor the allocation limit");
    assert_eq!(native_error, vm_error);
    assert_eq!(native_error.code, "R_ALLOCATION_LIMIT");
    let source = Value::Text("{}".into());
    let native_error = native
        .invoke("parse_text", vec![source.clone()])
        .expect_err("JSON parse must honor the allocation limit");
    let vm_error = vm
        .invoke("parse_text", vec![source])
        .expect_err("VM JSON parse must honor the allocation limit");
    assert_eq!(native_error, vm_error);
    assert_eq!(native_error.code, "R_ALLOCATION_LIMIT");

    let object = Value::Json(SharedJson::from_serde(serde_json::json!({"k": 1})));
    let key_policy = HostPolicy {
        max_text_bytes: 3,
        ..HostPolicy::default()
    };
    let mut native = Runtime::new()
        .load(artifact.clone(), key_policy.clone())
        .expect("native runtime loads");
    let mut vm = Runtime::vm_only()
        .load(artifact.clone(), key_policy)
        .expect("VM runtime loads");
    let native_error = native
        .invoke("get_long_key", vec![object.clone()])
        .expect_err("JSON key must honor the text limit");
    let vm_error = vm
        .invoke("get_long_key", vec![object])
        .expect_err("VM JSON key must honor the text limit");
    assert_eq!(native_error, vm_error);
    assert_eq!(native_error.code, "R_TEXT_LIMIT");
    let native_error = native
        .invoke("parse_long_literal", vec![])
        .expect_err("JSON parse input must honor the text limit");
    let vm_error = vm
        .invoke("parse_long_literal", vec![])
        .expect_err("VM JSON parse input must honor the text limit");
    assert_eq!(native_error, vm_error);
    assert_eq!(native_error.code, "R_TEXT_LIMIT");

    let integer_policy = HostPolicy {
        max_integer_digits: 2,
        ..HostPolicy::default()
    };
    let mut native = Runtime::new()
        .load(artifact.clone(), integer_policy.clone())
        .expect("native runtime loads");
    let mut vm = Runtime::vm_only()
        .load(artifact, integer_policy)
        .expect("VM runtime loads");
    let native_error = native
        .invoke("main", vec![])
        .expect_err("JSON integer conversion must honor the digit limit");
    let vm_error = vm
        .invoke("main", vec![])
        .expect_err("VM JSON integer conversion must honor the digit limit");
    assert_eq!(native_error, vm_error);
    assert_eq!(native_error.code, "R_INTEGER_LIMIT");
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "main"));
}

#[test]
#[cfg(feature = "native-value-bridge")]
fn native_backend_indexes_typed_json_handle_lists() {
    let source = "import \"std:json\"\n\nfn main() -> Int:\n    let document = unwrap(json_parse(\"{\\\"count\\\":7}\"))\n    let rows: List[Json] = [document]\n    let field = unwrap(json_get_safe(rows[0], \"count\"))\n    let count = json_int_safe(field)\n    if is_ok(count):\n        return unwrap(count)\n    return -1\n";
    let project_root = std::env::temp_dir().join(format!(
        "guion-native-json-handle-list-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&project_root).expect("temporary project directory is created");
    let entry = project_root.join("main.gu");
    std::fs::write(&entry, source).expect("temporary project source is written");
    let stdlib_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib");
    let artifact = compile_project_with_roots(
        &entry,
        &project_root,
        Some(&stdlib_root),
        &std::collections::HashMap::new(),
    )
    .expect("typed JSON handle-list fixture compiles");
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .expect("native runtime loads");
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .expect("VM runtime loads");

    let expected = Value::Int(BigInt::from_i64(7));
    assert_eq!(native.invoke("main", vec![]).unwrap(), expected);
    assert_eq!(vm.invoke("main", vec![]).unwrap(), expected);
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "main"));
    let _ = std::fs::remove_dir_all(project_root);
}

#[cfg(feature = "native-value-bridge")]
#[test]
fn native_mixed_value_handles_preserve_vm_allocation_limits() {
    let source = r#"record Payload:
    text: Text

fn payload_text(payload: Payload) -> Text:
    return payload.text

fn has_field(document: Json, key: Text) -> Bool:
    return is_ok(json_try_get(document, key))

fn main() -> Int:
    let parsed = json_parse("{\"value\":\"guion\"}")
    let document = unwrap(parsed)
    let payload = Payload("guion")
    var cursor = 0
    var found = 0
    while cursor < 100:
        if has_field(document, "value") and payload_text(payload) == "guion":
            set found = found + 1
        set cursor = cursor + 1
    return found
"#;
    let artifact = compile("native-json-handle-budget.gu", source).unwrap();

    for allocation_limit_bytes in [4096, 8192, 16384, 32768] {
        let policy = HostPolicy {
            allocation_limit_bytes,
            ..HostPolicy::default()
        };
        let mut native = Runtime::new()
            .load(artifact.clone(), policy.clone())
            .unwrap();
        let mut vm = Runtime::vm_only().load(artifact.clone(), policy).unwrap();

        match (native.invoke("main", vec![]), vm.invoke("main", vec![])) {
            (Ok(native), Ok(vm)) => assert_eq!(native, vm),
            (Err(native), Err(vm)) => {
                assert_eq!(native.code, vm.code);
                assert_eq!(native.trace, vm.trace);
            }
            (native, vm) => panic!(
                "native mixed-value handles changed the allocation outcome at {allocation_limit_bytes} bytes: {native:?} vs {vm:?}"
            ),
        }
        assert!(
            native
                .native_function_names()
                .iter()
                .any(|name| name == "has_field")
                && native
                    .native_function_names()
                    .iter()
                    .any(|name| name == "payload_text"),
            "JSON and record helpers must exercise native mixed-value handles"
        );
    }

    let profiled_policy = HostPolicy {
        allocation_limit_bytes: 1_000_000,
        ..HostPolicy::default()
    };
    let mut native = Runtime::new()
        .with_runtime_profile()
        .load(artifact.clone(), profiled_policy.clone())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .with_runtime_profile()
        .load(artifact, profiled_policy)
        .unwrap();
    let expected = Value::Int(BigInt::parse("100").unwrap());
    assert_eq!(native.invoke("main", vec![]).unwrap(), expected);
    assert_eq!(vm.invoke("main", vec![]).unwrap(), expected);
    let native_profile = native.runtime_profile().unwrap();
    let vm_profile = vm.runtime_profile().unwrap();
    let native_allocated = native_profile["estimated_guest_allocation_budget_charges"]
        ["estimated_gross_bytes"]
        .as_u64()
        .unwrap();
    let vm_allocated = vm_profile["estimated_guest_allocation_budget_charges"]
        ["estimated_gross_bytes"]
        .as_u64()
        .unwrap();
    assert_eq!(
        native_allocated, vm_allocated,
        "native arena handles must not be charged as guest allocations"
    );
}

#[cfg(feature = "native-value-bridge")]
#[test]
fn native_backend_bridges_json_values_through_a_checked_host_call() {
    let source = "fn accepts_text(value: Json) -> Bool:\n    return is_ok(json_try_text(value))\n\nfn main() -> Int:\n    return 0\n";
    let artifact =
        compile("native-json-host-bridge.gu", source).expect("mixed-value bridge fixture compiles");
    let text_json = Value::Json(SharedJson::from_serde(serde_json::json!("guion")));
    let number_json = Value::Json(SharedJson::from_serde(serde_json::json!(42)));

    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .expect("native runtime loads");
    assert_eq!(
        native
            .invoke("accepts_text", vec![text_json.clone()])
            .expect("native host call runs"),
        Value::Bool(true)
    );
    assert_eq!(
        native
            .invoke("accepts_text", vec![number_json.clone()])
            .expect("recoverable JSON type error runs"),
        Value::Bool(false)
    );
    assert!(
        native
            .native_function_names()
            .iter()
            .any(|name| name == "accepts_text"),
        "mixed-value helper should be compiled natively: {:?}",
        native.native_function_names()
    );
    assert!(native.native_calls() >= 2);

    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .expect("VM runtime loads");
    assert_eq!(
        vm.invoke("accepts_text", vec![text_json])
            .expect("VM host call runs"),
        Value::Bool(true)
    );
    assert_eq!(
        vm.invoke("accepts_text", vec![number_json])
            .expect("VM recoverable JSON type error runs"),
        Value::Bool(false)
    );
}

#[test]
fn native_backend_executes_jsonl_next_by_default() {
    let root = std::env::temp_dir().join(format!(
        "guion-native-jsonl-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let events = root.join("events.jsonl");
    std::fs::write(&events, "{\"kind\":\"ok\"}\nnot json\n").unwrap();
    let source = "import \"std:jsonl\"\n\nfn main() -> Int:\n    let arguments = args()\n    if len(arguments) != 1:\n        return 8\n    let opened = jsonl_open(arguments[0])\n    if not is_ok(opened):\n        return 1\n    let reader = unwrap(opened)\n    let next = jsonl_next(reader)\n    if not is_ok(next):\n        return 2\n    let maybe_line = unwrap(next)\n    if not is_some(maybe_line):\n        return 3\n    let line = unwrap(maybe_line)\n    if line.raw != \"{\\\"kind\\\":\\\"ok\\\"}\":\n        return 4\n    if not is_some(line.value):\n        return 5\n    let parsed = unwrap(line.value)\n    let kind = json_try_get(parsed, \"kind\")\n    if not is_ok(kind):\n        return 6\n    if not is_ok(json_try_text(unwrap(kind))):\n        return 7\n    let second = unwrap(jsonl_next(reader))\n    if not is_some(second):\n        return 9\n    if unwrap(second).raw != \"not json\":\n        return 10\n    if is_some(unwrap(second).value):\n        return 11\n    let eof = unwrap(jsonl_next(reader))\n    if is_some(eof):\n        return 12\n    return 0\n";
    let entry = root.join("main.gu");
    std::fs::write(&entry, source).unwrap();
    let stdlib_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib");
    let artifact = compile_project_with_roots(
        &entry,
        &root,
        Some(&stdlib_root),
        &std::collections::HashMap::new(),
    )
    .expect("JSONL bridge fixture compiles");
    let policy = HostPolicy::default()
        .with_file_access(root.clone(), true, false)
        .with_arguments(vec!["events.jsonl".into()]);
    let mut native = Runtime::new()
        .load(artifact.clone(), policy.clone())
        .expect("native runtime loads");
    assert_eq!(
        native.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::from_i64(0))
    );
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "jsonl_next"));
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "fs_next_line"));
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "main"));
    assert!(
        native.native_calls() > 0,
        "the root JSONL call chain should enter compiled native code"
    );
    assert_eq!(
        native.native_deoptimizations(),
        0,
        "{}",
        native.native_compile_profile()
    );

    let mut vm = Runtime::vm_only()
        .load(artifact, policy)
        .expect("VM runtime loads");
    assert_eq!(
        vm.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::from_i64(0))
    );
    let _ = std::fs::remove_dir_all(root);
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
    let mut vm = Runtime::vm_only().load(artifact.clone(), limited).unwrap();
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
    // The overflowing `increment` native entry is counted once; its resumed
    // VM continuation is not a second native invocation.
    assert_eq!(instance.native_calls(), 9);

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
        (
            "fn compute() -> Float64:\n    return 1.0 / 0.0\n\nfn main() -> Int:\n    let ignored = compute()\n    return 0\n",
            "R_DIV_ZERO",
        ),
        (
            "fn compute() -> Float64:\n    return 1e308 * 1e308\n\nfn main() -> Int:\n    let ignored = compute()\n    return 0\n",
            "R_FLOAT_NONFINITE",
        ),
    ] {
        let artifact = compile("float-runtime-error.gu", source).unwrap();
        let mut instance = Runtime::new()
            .load(artifact, HostPolicy::default())
            .unwrap();
        assert_eq!(
            instance.invoke("main", vec![]).unwrap_err().code,
            expected_code
        );
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
    // Each round's concatenation allocates a fresh 16-byte text; the literals are shared.
    let policy = HostPolicy {
        allocation_limit_bytes: 512,
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
    // The literal is shared and allocates nothing; rendering it for output does.
    let policy = HostPolicy {
        allocation_limit_bytes: 800_000,
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

#[test]
fn fused_json_field_accessors_match_the_two_step_form() {
    let source = r#"import "std:json"

fn describe_text(value: Json, key: Text) -> Text:
    let fused = json_get_text_safe(value, key)
    let stepped = json_text_safe_chain(value, key)
    if is_ok(fused) and is_ok(stepped):
        return "ok:" + unwrap(fused) + ":" + unwrap(stepped)
    if not is_ok(fused) and not is_ok(stepped):
        return "err:" + json_error_code(error(fused)) + ":" + json_error_code(error(stepped))
    return "mismatch"

fn json_text_safe_chain(value: Json, key: Text) -> Result[Text, JsonError]:
    let field = json_get_safe(value, key)
    if not is_ok(field):
        return Err(error(field))
    return json_text_safe(unwrap(field))

fn describe_int(value: Json, key: Text) -> Text:
    let fused = json_get_int_safe(value, key)
    let field = json_get_safe(value, key)
    if is_ok(fused):
        return "ok:" + json_stringify(json_int(unwrap(fused)))
    if is_ok(field):
        return "type:" + json_error_code(error(fused)) + ":" + json_error_code(error(json_int_safe(unwrap(field))))
    return "missing:" + json_error_code(error(fused)) + ":" + json_error_code(error(field))

fn main() -> Int:
    let document = unwrap(json_parse("{\"name\":\"ada\",\"count\":7}"))
    print(describe_text(document, "name"))
    print(describe_text(document, "count"))
    print(describe_text(document, "absent"))
    print(describe_int(document, "count"))
    print(describe_int(document, "name"))
    print(describe_int(document, "absent"))
    return 0
"#;
    let project_root = std::env::temp_dir().join(format!(
        "guion-fused-json-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&project_root).unwrap();
    let entry = project_root.join("main.gu");
    std::fs::write(&entry, source).unwrap();
    let stdlib_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib");
    let artifact = compile_project_with_roots(
        &entry,
        &project_root,
        Some(&stdlib_root),
        &std::collections::HashMap::new(),
    )
    .expect("fused accessor fixture compiles");
    let _ = std::fs::remove_dir_all(&project_root);
    let mut instance = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    instance.invoke("main", vec![]).unwrap();
    let output = instance.take_output();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 6, "{output}");
    assert_eq!(lines[0], "ok:ada:ada");
    assert!(lines[1].starts_with("err:"), "{output}");
    assert!(lines[2].starts_with("err:"), "{output}");
    assert_eq!(lines[3], "ok:7");
    assert!(lines[4].starts_with("type:"), "{output}");
    assert!(lines[5].starts_with("missing:"), "{output}");
    for line in &lines[1..3] {
        let parts: Vec<&str> = line.split(':').collect();
        assert_eq!(
            parts[1], parts[2],
            "fused and stepped error codes differ: {line}"
        );
    }
    for line in &lines[4..6] {
        let parts: Vec<&str> = line.split(':').collect();
        assert_eq!(
            parts[1], parts[2],
            "fused and stepped error codes differ: {line}"
        );
    }
}

#[test]
fn option_and_result_constructors_outside_a_matching_function_are_source_diagnostics() {
    let option = "record Box:\n    value: Option[Int]\n\nfn main() -> Int:\n    let boxed = Box(Some(1))\n    return 0\n";
    let invalid = compile("option-nested.gu", option).unwrap_err();
    assert_eq!(invalid.code, "E_OPTION_CONSTRUCTOR_CONTEXT");
    assert_eq!(invalid.span.line, 5);

    let result = "fn wrap() -> Option[Result[Int, Text]]:\n    return Some(Ok(1))\n\nfn main() -> Int:\n    return 0\n";
    let invalid = compile("result-nested.gu", result).unwrap_err();
    assert_eq!(invalid.code, "E_RESULT_CONSTRUCTOR_CONTEXT");
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

#[test]
fn native_print_err_preserves_stream_order_and_output_limit_fallback() {
    let source =
        "fn main() -> Int:\n    print(1)\n    eprint(\"problem\")\n    print(2)\n    return 0\n";
    let artifact = compile("native-print-err.gu", source).unwrap();
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
    assert_eq!(native.take_program_stderr(), vm.take_program_stderr());
    assert_eq!(native.effect_report(), vm.effect_report());
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "main"));

    let limited = HostPolicy {
        output_limit_bytes: 4,
        ..HostPolicy::default()
    };
    let mut native = Runtime::new()
        .load(artifact.clone(), limited.clone())
        .unwrap();
    let mut vm = Runtime::vm_only().load(artifact.clone(), limited).unwrap();
    let native_error = native.invoke("main", vec![]).unwrap_err();
    let vm_error = vm.invoke("main", vec![]).unwrap_err();
    assert_eq!(native_error.code, vm_error.code);
    assert_eq!(native_error.code, "R_OUTPUT_LIMIT");
    let native_output = native.take_output();
    let vm_output = vm.take_output();
    let native_stderr = native.take_program_stderr();
    let vm_stderr = vm.take_program_stderr();
    assert_eq!(native_output, vm_output);
    assert_eq!(native_output, "1\n");
    assert_eq!(native_stderr, vm_stderr);
    assert_eq!(native_stderr, "");
    assert_eq!(native.effect_report(), vm.effect_report());

    let mixed_limit = HostPolicy {
        output_limit_bytes: 11,
        ..HostPolicy::default()
    };
    let mut native = Runtime::new()
        .load(artifact.clone(), mixed_limit.clone())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact.clone(), mixed_limit)
        .unwrap();
    let native_error = native.invoke("main", vec![]).unwrap_err();
    let vm_error = vm.invoke("main", vec![]).unwrap_err();
    assert_eq!(native_error.code, vm_error.code);
    assert_eq!(native_error.code, "R_OUTPUT_LIMIT");
    let native_output = native.take_output();
    let vm_output = vm.take_output();
    let native_stderr = native.take_program_stderr();
    let vm_stderr = vm.take_program_stderr();
    assert_eq!(native_output, vm_output);
    assert_eq!(native_output, "1\n");
    assert_eq!(native_stderr, vm_stderr);
    assert_eq!(native_stderr, "problem\n");
    assert_eq!(native.effect_report(), vm.effect_report());
}

#[test]
fn native_print_err_replays_allocation_limit_errors_through_the_vm() {
    let artifact = compile(
        "native-print-err-allocation.gu",
        "fn main() -> Int:\n    eprint(\"problem\")\n    return 0\n",
    )
    .unwrap();
    let limited = HostPolicy {
        allocation_limit_bytes: 0,
        ..HostPolicy::default()
    };
    let mut native = Runtime::new()
        .load(artifact.clone(), limited.clone())
        .unwrap();
    let mut vm = Runtime::vm_only().load(artifact, limited).unwrap();

    let native_error = native.invoke("main", vec![]).unwrap_err();
    let vm_error = vm.invoke("main", vec![]).unwrap_err();
    assert_eq!(native_error.code, vm_error.code);
    assert_eq!(native_error.code, "R_ALLOCATION_LIMIT");
    assert_eq!(native.take_output(), vm.take_output());
    assert_eq!(native.take_program_stderr(), vm.take_program_stderr());
    assert_eq!(native.effect_report(), vm.effect_report());
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "main"));
}

#[test]
fn native_rejection_profile_names_the_first_unsupported_instruction() {
    let artifact = compile(
        "native-rejection-instruction.gu",
        "fn join() -> Json:\n    return json_bool(true)\n\nfn main() -> Int:\n    if join() == json_bool(true):\n        return 0\n    return 1\n",
    )
    .unwrap();
    let mut runtime = Runtime::new()
        .load(artifact, HostPolicy::default())
        .unwrap();
    assert_eq!(
        runtime.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::from_i64(0))
    );

    let profile = runtime.native_compile_profile();
    let rejected = profile["rejected_functions"]
        .as_array()
        .expect("native profile reports rejected functions");
    let main = rejected
        .iter()
        .find(|function| function["name"] == "join")
        .expect("join is rejected from native compilation");
    let blocker = main["blocking_instruction"]
        .as_str()
        .expect("type-flow rejection names its blocking bytecode instruction");
    assert!(
        blocker.contains("JsonBool"),
        "unexpected blocker: {blocker}"
    );
}

#[test]
#[cfg(not(feature = "native-value-bridge"))]
fn native_loops_reaching_value_callbacks_stay_in_vm_for_arena_lifetime() {
    let source = "fn parses(value: Text) -> Bool:\n    return is_ok(json_parse(value))\n\nfn main() -> Int:\n    var index = 0\n    while index < 10:\n        if parses(\"{\\\"value\\\":1}\"):\n            set index = index + 1\n        else:\n            return -1\n    return index\n";
    let artifact =
        compile("native-loop-arena-lifetime.gu", source).expect("callback loop fixture compiles");
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
    assert_eq!(
        native.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::from_usize(10))
    );
    assert!(!native
        .native_function_names()
        .iter()
        .any(|name| name == "main"));
    let compile_profile = native.native_compile_profile();
    let rejected = compile_profile["rejected_functions"]
        .as_array()
        .expect("native compile profile lists rejected functions");
    let main = rejected
        .iter()
        .find(|function| function["name"] == "main")
        .expect("callback loop is rejected from native compilation");
    assert_eq!(main["reason_code"], "native_loop_value_arena_retention");
    assert!(main["blocking_instruction"]
        .as_str()
        .unwrap()
        .contains("loop backedge"));
}

#[test]
#[cfg(not(feature = "native-value-bridge"))]
fn native_json_value_creation_loops_stay_in_vm_for_arena_lifetime() {
    let source = "fn main() -> Int:\n    var index = 0\n    var report = json_object()\n    while index < 100:\n        set report = json_set(report, \"index\", json_int(index))\n        set index = index + 1\n    return index\n";
    let artifact = compile("native-json-allocation-loop-arena.gu", source)
        .expect("JSON allocation loop fixture compiles");
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .unwrap();

    let native_value = native.invoke("main", vec![]).unwrap();
    assert_eq!(native_value, vm.invoke("main", vec![]).unwrap());
    assert_eq!(native_value, Value::Int(BigInt::from_usize(100)));
    assert!(!native
        .native_function_names()
        .iter()
        .any(|name| name == "main"));
    let compile_profile = native.native_compile_profile();
    let main = compile_profile["rejected_functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|function| function["name"] == "main")
        .expect("JSON allocation loop is rejected from native compilation");
    assert_eq!(main["reason_code"], "native_loop_value_arena_retention");
}

#[test]
fn native_typed_map_initialization_preserves_allocation_limits() {
    let artifact = compile(
        "native-typed-map-init.gu",
        "fn main() -> Int:\n    let allowed: Map[Text, Bool] = {}\n    return 0\n",
    )
    .unwrap();
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let expected = Value::Int(BigInt::from_i64(0));
    assert_eq!(native.invoke("main", vec![]).unwrap(), expected);
    assert_eq!(vm.invoke("main", vec![]).unwrap(), expected);
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "main"));

    let limited = HostPolicy {
        allocation_limit_bytes: 0,
        ..HostPolicy::default()
    };
    let mut native = Runtime::new()
        .load(artifact.clone(), limited.clone())
        .unwrap();
    let mut vm = Runtime::vm_only().load(artifact, limited).unwrap();
    let native_error = native.invoke("main", vec![]).unwrap_err();
    let vm_error = vm.invoke("main", vec![]).unwrap_err();
    assert_eq!(native_error.code, vm_error.code);
    assert_eq!(native_error.code, "R_ALLOCATION_LIMIT");
}

#[test]
fn native_json_array_initialization_preserves_allocation_limits() {
    let artifact = compile(
        "native-json-array-init.gu",
        "fn main() -> Int:\n    let invalid = json_array()\n    return 0\n",
    )
    .unwrap();
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let expected = Value::Int(BigInt::from_i64(0));
    assert_eq!(native.invoke("main", vec![]).unwrap(), expected);
    assert_eq!(vm.invoke("main", vec![]).unwrap(), expected);
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "main"));

    let limited = HostPolicy {
        allocation_limit_bytes: 0,
        ..HostPolicy::default()
    };
    let mut native = Runtime::new()
        .load(artifact.clone(), limited.clone())
        .unwrap();
    let mut vm = Runtime::vm_only().load(artifact, limited).unwrap();
    let native_error = native.invoke("main", vec![]).unwrap_err();
    let vm_error = vm.invoke("main", vec![]).unwrap_err();
    assert_eq!(native_error.code, vm_error.code);
    assert_eq!(native_error.code, "R_ALLOCATION_LIMIT");
}

#[test]
fn native_map_keys_produces_text_list_with_vm_quota_parity() {
    let artifact = compile(
        "native-map-keys.gu",
        "fn key_count(values: Map[Text, Bool]) -> Int:\n    var count = 0\n    for key in values:\n        set count = count + 1\n    return count\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let values = Value::Map(std::sync::Arc::new(guion_model::TextMap::from([
        ("first".to_owned(), Value::Bool(true)),
        ("second".to_owned(), Value::Bool(false)),
    ])));
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let expected = Value::Int(BigInt::from_i64(2));
    assert_eq!(
        native.invoke("key_count", vec![values.clone()]).unwrap(),
        expected
    );
    assert_eq!(
        vm.invoke("key_count", vec![values.clone()]).unwrap(),
        expected
    );
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "key_count"));

    let limited = HostPolicy {
        allocation_limit_bytes: 0,
        ..HostPolicy::default()
    };
    let mut native = Runtime::new()
        .load(artifact.clone(), limited.clone())
        .unwrap();
    let mut vm = Runtime::vm_only().load(artifact, limited).unwrap();
    let native_error = native
        .invoke("key_count", vec![values.clone()])
        .unwrap_err();
    let vm_error = vm.invoke("key_count", vec![values]).unwrap_err();
    assert_eq!(native_error.code, vm_error.code);
    assert_eq!(native_error.code, "R_ALLOCATION_LIMIT");
}

#[test]
fn native_json_index_preserves_result_and_vm_errors() {
    let artifact = compile(
        "native-json-index.gu",
        "fn json_item(value: Json, index: Int) -> Json:\n    return json_index(value, index)\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let input = Value::Json(SharedJson::from_serde(serde_json::json!([
        "first", "second"
    ])));
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let args = vec![input.clone(), Value::Int(BigInt::from_i64(1))];
    let native_value = native.invoke("json_item", args.clone()).unwrap();
    let vm_value = vm.invoke("json_item", args).unwrap();
    assert_eq!(native_value, vm_value);
    assert_eq!(
        native_value,
        Value::Json(SharedJson::from_serde(serde_json::json!("second")))
    );
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "json_item"));

    for index in [-1, 2] {
        let args = vec![input.clone(), Value::Int(BigInt::from_i64(index))];
        let native_error = native.invoke("json_item", args.clone()).unwrap_err();
        let vm_error = vm.invoke("json_item", args).unwrap_err();
        assert_eq!(native_error.code, vm_error.code);
        assert_eq!(native_error.code, "R_JSON_INDEX");
    }
}

#[test]
fn native_json_get_preserves_values_and_vm_errors() {
    let artifact = compile(
        "native-json-get.gu",
        "fn json_field(value: Json, key: Text) -> Json:\n    return json_get(value, key)\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let input = Value::Json(SharedJson::from_serde(serde_json::json!({
        "nested": { "answer": 42 }
    })));
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let args = vec![input.clone(), Value::Text("nested".into())];
    let native_value = native.invoke("json_field", args.clone()).unwrap();
    let vm_value = vm.invoke("json_field", args).unwrap();
    assert_eq!(native_value, vm_value);
    assert_eq!(
        native_value,
        Value::Json(SharedJson::from_serde(serde_json::json!({ "answer": 42 })))
    );
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "json_field"));

    for (value, key, expected_code) in [
        (
            Value::Json(SharedJson::from_serde(serde_json::json!([]))),
            "nested",
            "R_JSON_TYPE",
        ),
        (input, "missing", "R_JSON_KEY"),
    ] {
        let args = vec![value, Value::Text(key.into())];
        let native_error = native.invoke("json_field", args.clone()).unwrap_err();
        let vm_error = vm.invoke("json_field", args).unwrap_err();
        assert_eq!(native_error.code, vm_error.code);
        assert_eq!(native_error.code, expected_code);
    }
}

#[test]
fn native_json_stringify_preserves_text_and_vm_limits() {
    let artifact = compile(
        "native-json-stringify.gu",
        "fn stringify(value: Json) -> Text:\n    return json_stringify(value)\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let input = Value::Json(SharedJson::from_serde(serde_json::json!({
        "nested": ["東京", 42]
    })));
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let args = vec![input.clone()];
    let native_value = native.invoke("stringify", args.clone()).unwrap();
    let vm_value = vm.invoke("stringify", args).unwrap();
    assert_eq!(native_value, vm_value);
    assert_eq!(
        native_value,
        Value::Text(r#"{"nested":["東京",42]}"#.into())
    );
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "stringify"));

    let limited = HostPolicy {
        max_text_bytes: 4,
        ..HostPolicy::default()
    };
    let mut native = Runtime::new()
        .load(artifact.clone(), limited.clone())
        .unwrap();
    let mut vm = Runtime::vm_only().load(artifact.clone(), limited).unwrap();
    let native_error = native.invoke("stringify", vec![input.clone()]).unwrap_err();
    let vm_error = vm.invoke("stringify", vec![input.clone()]).unwrap_err();
    assert_eq!(native_error.code, vm_error.code);
    assert_eq!(native_error.code, "R_TEXT_LIMIT");

    let limited = HostPolicy {
        allocation_limit_bytes: 47,
        ..HostPolicy::default()
    };
    let mut native = Runtime::new()
        .load(artifact.clone(), limited.clone())
        .unwrap();
    let mut vm = Runtime::vm_only().load(artifact.clone(), limited).unwrap();
    let native_error = native.invoke("stringify", vec![input.clone()]).unwrap_err();
    let vm_error = vm.invoke("stringify", vec![input.clone()]).unwrap_err();
    assert_eq!(native_error.code, vm_error.code);
    assert_eq!(native_error.code, "R_ALLOCATION_LIMIT");
}

#[test]
fn native_json_object_initialization_preserves_allocation_limits() {
    let artifact = compile(
        "native-json-object-init.gu",
        "fn empty_object() -> Json:\n    return json_object()\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let expected = Value::Json(SharedJson::from_serde(serde_json::json!({})));
    assert_eq!(native.invoke("empty_object", vec![]).unwrap(), expected);
    assert_eq!(vm.invoke("empty_object", vec![]).unwrap(), expected);
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "empty_object"));

    let limited = HostPolicy {
        allocation_limit_bytes: 0,
        ..HostPolicy::default()
    };
    let mut native = Runtime::new()
        .load(artifact.clone(), limited.clone())
        .unwrap();
    let mut vm = Runtime::vm_only().load(artifact, limited).unwrap();
    let native_error = native.invoke("empty_object", vec![]).unwrap_err();
    let vm_error = vm.invoke("empty_object", vec![]).unwrap_err();
    assert_eq!(native_error.code, vm_error.code);
    assert_eq!(native_error.code, "R_ALLOCATION_LIMIT");
}

#[test]
fn native_json_int_preserves_integer_and_allocation_limits() {
    let artifact = compile(
        "native-json-int.gu",
        "fn encode() -> Json:\n    return json_int(42)\n\nfn oversized() -> Json:\n    return json_int(123)\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let expected = Value::Json(SharedJson::from_serde(serde_json::json!(42)));
    assert_eq!(native.invoke("encode", vec![]).unwrap(), expected);
    assert_eq!(vm.invoke("encode", vec![]).unwrap(), expected);
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "encode"));

    let limited = HostPolicy {
        max_integer_digits: 2,
        ..HostPolicy::default()
    };
    let mut native = Runtime::new()
        .load(artifact.clone(), limited.clone())
        .unwrap();
    let mut vm = Runtime::vm_only().load(artifact.clone(), limited).unwrap();
    let native_error = native.invoke("oversized", vec![]).unwrap_err();
    let vm_error = vm.invoke("oversized", vec![]).unwrap_err();
    assert_eq!(native_error.code, vm_error.code);
    assert_eq!(native_error.code, "R_INTEGER_LIMIT");

    let limited = HostPolicy {
        allocation_limit_bytes: 0,
        ..HostPolicy::default()
    };
    let mut native = Runtime::new()
        .load(artifact.clone(), limited.clone())
        .unwrap();
    let mut vm = Runtime::vm_only().load(artifact, limited).unwrap();
    let native_error = native.invoke("encode", vec![]).unwrap_err();
    let vm_error = vm.invoke("encode", vec![]).unwrap_err();
    assert_eq!(native_error.code, vm_error.code);
    assert_eq!(native_error.code, "R_ALLOCATION_LIMIT");
}

#[test]
fn native_json_text_preserves_unicode_and_vm_limits() {
    let artifact = compile(
        "native-json-text.gu",
        "fn encode() -> Json:\n    return json_text(\"snow🙂\")\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let expected = Value::Json(SharedJson::from_serde(serde_json::json!("snow🙂")));
    assert_eq!(native.invoke("encode", vec![]).unwrap(), expected);
    assert_eq!(vm.invoke("encode", vec![]).unwrap(), expected);
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "encode"));

    let limited = HostPolicy {
        max_text_bytes: 4,
        ..HostPolicy::default()
    };
    let mut native = Runtime::new()
        .load(artifact.clone(), limited.clone())
        .unwrap();
    let mut vm = Runtime::vm_only().load(artifact.clone(), limited).unwrap();
    let native_error = native.invoke("encode", vec![]).unwrap_err();
    let vm_error = vm.invoke("encode", vec![]).unwrap_err();
    assert_eq!(native_error.code, vm_error.code);
    assert_eq!(native_error.code, "R_TEXT_LIMIT");

    let limited = HostPolicy {
        allocation_limit_bytes: 0,
        ..HostPolicy::default()
    };
    let mut native = Runtime::new()
        .load(artifact.clone(), limited.clone())
        .unwrap();
    let mut vm = Runtime::vm_only().load(artifact, limited).unwrap();
    let native_error = native.invoke("encode", vec![]).unwrap_err();
    let vm_error = vm.invoke("encode", vec![]).unwrap_err();
    assert_eq!(native_error.code, vm_error.code);
    assert_eq!(native_error.code, "R_ALLOCATION_LIMIT");
}

#[test]
fn native_time_parse_timestamp_preserves_results_and_limits() {
    let root = std::env::temp_dir().join(format!(
        "guion-native-time-parse-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let entry = root.join("main.gu");
    std::fs::write(
        &entry,
        "import \"std:time\"\nimport \"std:types\"\n\nfn parse_value(value: Text) -> Result[Instant, FileError]:\n    return parse_timestamp(value)\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let stdlib_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib");
    let artifact = compile_project_with_roots(
        &entry,
        &root,
        Some(&stdlib_root),
        &std::collections::HashMap::new(),
    )
    .unwrap();
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    for text in ["2024-01-01T00:00:00Z", "not-a-timestamp"] {
        let args = vec![Value::Text(text.into())];
        assert_eq!(
            native.invoke("parse_value", args.clone()).unwrap(),
            vm.invoke("parse_value", args).unwrap()
        );
    }
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "parse_timestamp"));
    assert!(native.native_calls() > 0);

    let limited = HostPolicy {
        max_text_bytes: 4,
        ..HostPolicy::default()
    };
    let mut native = Runtime::new()
        .load(artifact.clone(), limited.clone())
        .unwrap();
    let mut vm = Runtime::vm_only().load(artifact, limited).unwrap();
    let args = vec![Value::Text("2024-01-01T00:00:00Z".into())];
    let native_error = native.invoke("parse_value", args.clone()).unwrap_err();
    let vm_error = vm.invoke("parse_value", args).unwrap_err();
    assert_eq!(native_error.code, vm_error.code);
    assert_eq!(native_error.code, "R_TEXT_LIMIT");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_time_format_utc_preserves_record_inputs_and_limits() {
    let source = "import \"std:time\"\nimport \"std:types\"\n\nfn format_value(value: Instant) -> Text:\n    return format_utc(value)\n\nfn parse_and_format(value: Text) -> Text:\n    return format_utc(unwrap(parse_timestamp(value)))\n\nfn parse_format_and_read(value: Text) -> Int:\n    let instant = unwrap(parse_timestamp(value))\n    let formatted = format_utc(instant)\n    if formatted == value:\n        return instant.unix_microseconds\n    return -1\n\nfn main() -> Int:\n    return 0\n";
    let root = std::env::temp_dir().join(format!(
        "guion-native-time-format-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let entry = root.join("main.gu");
    std::fs::write(&entry, source).unwrap();
    let stdlib_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib");
    let artifact = compile_project_with_roots(
        &entry,
        &root,
        Some(&stdlib_root),
        &std::collections::HashMap::new(),
    )
    .unwrap();
    let instant = Value::Record(
        "Instant".into(),
        std::sync::Arc::new(vec![Value::Int(BigInt::from_i64(1_704_067_200_000_000))]),
    );
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let native_value = native
        .invoke("format_value", vec![instant.clone()])
        .unwrap();
    assert_eq!(
        native_value,
        vm.invoke("format_value", vec![instant]).unwrap()
    );
    assert_eq!(native_value, Value::Text("2024-01-01T00:00:00Z".into()));
    let args = vec![Value::Text("2024-01-01T00:00:00Z".into())];
    assert_eq!(
        native.invoke("parse_and_format", args.clone()).unwrap(),
        vm.invoke("parse_and_format", args).unwrap()
    );
    let args = vec![Value::Text("2024-01-01T00:00:00Z".into())];
    assert_eq!(
        native
            .invoke("parse_format_and_read", args.clone())
            .expect("native parsed record remains usable after formatting"),
        Value::Int(BigInt::from_i64(1_704_067_200_000_000))
    );
    assert_eq!(
        vm.invoke("parse_format_and_read", args).unwrap(),
        Value::Int(BigInt::from_i64(1_704_067_200_000_000))
    );
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "parse_format_and_read"));
    assert_eq!(native.native_deoptimizations(), 0);
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "format_utc"));
    assert!(native.native_calls() > 0);

    let limited = HostPolicy {
        max_text_bytes: 10,
        ..HostPolicy::default()
    };
    let mut native = Runtime::new()
        .load(artifact.clone(), limited.clone())
        .unwrap();
    let mut vm = Runtime::vm_only().load(artifact, limited).unwrap();
    let instant = Value::Record(
        "Instant".into(),
        std::sync::Arc::new(vec![Value::Int(BigInt::from_i64(1_704_067_200_000_000))]),
    );
    let native_error = native
        .invoke("format_value", vec![instant.clone()])
        .unwrap_err();
    let vm_error = vm.invoke("format_value", vec![instant]).unwrap_err();
    assert_eq!(native_error.code, vm_error.code);
    assert_eq!(native_error.code, "R_TEXT_LIMIT");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_record_from_map_bridge_preserves_primitive_list_fields() {
    let source = "record Frame:\n    values: List[Int]\n\nfn inspect(frames: Map[Text, Frame]) -> Int:\n    let frame = frames[\"current\"]\n    let values = frame.values\n    return values[1] + len(values)\n\nfn main() -> Int:\n    return 0\n";
    let artifact = compile("native-record-map-list-bridge.gu", source)
        .expect("record map bridge source compiles");
    let frame = Value::Record(
        "Frame".into(),
        std::sync::Arc::new(vec![Value::List(std::sync::Arc::new(vec![
            Value::Int(BigInt::from_i64(4)),
            Value::Int(BigInt::from_i64(7)),
        ]))]),
    );
    let frames = Value::Map(std::sync::Arc::new(guion_model::TextMap::from([(
        "current".to_owned(),
        frame,
    )])));
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .expect("native runtime loads the artifact");
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .expect("VM runtime loads the artifact");
    let arguments = vec![frames];

    assert_eq!(
        native.invoke("inspect", arguments.clone()).unwrap(),
        Value::Int(BigInt::from_i64(9))
    );
    assert_eq!(
        vm.invoke("inspect", arguments).unwrap(),
        Value::Int(BigInt::from_i64(9))
    );
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "inspect"));
    assert_eq!(native.native_deoptimizations(), 0);
}

#[test]
fn native_write_text_atomic_side_exits_after_compiling_prefix() {
    let root = std::env::temp_dir().join(format!(
        "guion-native-atomic-write-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let native_root = root.join("native");
    let vm_root = root.join("vm");
    std::fs::create_dir_all(&native_root).unwrap();
    std::fs::create_dir_all(&vm_root).unwrap();
    let artifact = compile(
        "native-write-text-atomic.gu",
        "fn main() -> Int:\n    let saved = write_text_atomic(\"state.txt\", \"written\")\n    return 0\n",
    )
    .unwrap();
    let mut native = Runtime::new()
        .load(
            artifact.clone(),
            HostPolicy::default().with_file_access(&native_root, true, true),
        )
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(
            artifact,
            HostPolicy::default().with_file_access(&vm_root, true, true),
        )
        .unwrap();
    assert_eq!(
        native.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::from_i64(0))
    );
    assert_eq!(
        vm.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::from_i64(0))
    );
    assert_eq!(
        std::fs::read_to_string(native_root.join("state.txt")).unwrap(),
        "written"
    );
    assert_eq!(
        std::fs::read_to_string(vm_root.join("state.txt")).unwrap(),
        "written"
    );
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "main"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_file_open_and_close_preserve_capability_effects() {
    let root = std::env::temp_dir().join(format!(
        "guion-native-file-open-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("events.txt"), "event\n").unwrap();
    let entry = root.join("main.gu");
    std::fs::write(
        &entry,
        "import \"std:fs\"\n\nfn cycle(path: Text) -> Bool:\n    let opened = fs_open_lines(path)\n    if not is_ok(opened):\n        return false\n    let reader = unwrap(opened)\n    return is_ok(fs_close_reader(reader))\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let stdlib_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib");
    let artifact = compile_project_with_roots(
        &entry,
        &root,
        Some(&stdlib_root),
        &std::collections::HashMap::new(),
    )
    .unwrap();
    let policy = HostPolicy::default().with_file_access(&root, true, false);
    let mut native = Runtime::new()
        .load(artifact.clone(), policy.clone())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact.clone(), policy.clone())
        .unwrap();
    let args = vec![Value::Text("events.txt".into())];
    assert_eq!(
        native.invoke("cycle", args.clone()).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(vm.invoke("cycle", args).unwrap(), Value::Bool(true));
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "cycle"));
    assert!(native.native_calls() > 0);
    assert_eq!(native.effect_report(), vm.effect_report());
    assert_eq!(
        native.effect_report(),
        &[EffectRecord {
            kind: EffectKind::FileRead,
            target: "events.txt".into(),
            target_truncated: false,
            outcome: EffectOutcome::Returned,
        }]
    );

    let mut native = Runtime::new()
        .load(artifact.clone(), policy.clone())
        .unwrap();
    let mut vm = Runtime::vm_only().load(artifact, policy).unwrap();
    let denied_args = vec![Value::Text("../outside.txt".into())];
    assert_eq!(
        native.invoke("cycle", denied_args.clone()).unwrap(),
        Value::Bool(false)
    );
    assert_eq!(vm.invoke("cycle", denied_args).unwrap(), Value::Bool(false));
    assert!(native.effect_report().is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_json_set_local_resumes_with_copy_on_write_aliases() {
    let artifact = compile(
        "native-json-set-local.gu",
        "fn update(report: Json, key: Text, value: Json) -> Text:\n    let original = report\n    var copy = report\n    set copy = json_set(copy, key, value)\n    return json_stringify(original) + \"|\" + json_stringify(copy)\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let input = Value::Json(SharedJson::from_serde(serde_json::json!({})));
    let value = Value::Json(SharedJson::from_serde(serde_json::json!(42)));
    let args = vec![input, Value::Text("answer".into()), value];
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .unwrap();
    let native_value = native.invoke("update", args.clone()).unwrap();
    let vm_value = vm.invoke("update", args).unwrap();
    assert_eq!(native_value, vm_value);
    assert_eq!(native_value, Value::Text("{}|{\"answer\":42}".into()));
    assert_eq!(native.native_deoptimizations(), 0);
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "update"));
}

#[test]
fn native_json_set_local_updates_unique_values_without_deoptimizing() {
    let artifact = compile(
        "native-json-set-local-unique.gu",
        "fn update() -> Text:\n    var object = json_object()\n    set object = json_set(object, \"answer\", json_int(42))\n    set object = json_set(object, \"ready\", json_text(\"yes\"))\n    return json_stringify(object)\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .unwrap();

    let native_value = native.invoke("update", Vec::new()).unwrap();
    let vm_value = vm.invoke("update", Vec::new()).unwrap();
    assert_eq!(native_value, vm_value);
    assert_eq!(
        native_value,
        Value::Text(r#"{"answer":42,"ready":"yes"}"#.into())
    );
    assert_eq!(native.native_deoptimizations(), 0);
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "update"));
}

#[test]
fn native_json_push_local_resumes_with_copy_on_write_aliases() {
    let artifact = compile(
        "native-json-push-local.gu",
        "fn append(report: Json, value: Json) -> Text:\n    let original = report\n    var copy = report\n    set copy = json_push(copy, value)\n    return json_stringify(original) + \"|\" + json_stringify(copy)\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let report = Value::Json(SharedJson::from_serde(serde_json::json!([])));
    let item = Value::Json(SharedJson::from_serde(serde_json::json!(42)));
    let args = vec![report, item];
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .unwrap();
    let native_value = native.invoke("append", args.clone()).unwrap();
    let vm_value = vm.invoke("append", args).unwrap();
    assert_eq!(native_value, vm_value);
    assert_eq!(native_value, Value::Text("[]|[42]".into()));
    assert_eq!(native.native_deoptimizations(), 0);
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "append"));
}

#[test]
fn native_json_push_local_updates_unique_values_without_deoptimizing() {
    let artifact = compile(
        "native-json-push-local-unique.gu",
        "fn append() -> Text:\n    var array = json_array()\n    set array = json_push(array, json_int(1))\n    set array = json_push(array, json_int(2))\n    return json_stringify(array)\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .unwrap();

    let native_value = native.invoke("append", Vec::new()).unwrap();
    let vm_value = vm.invoke("append", Vec::new()).unwrap();
    assert_eq!(native_value, vm_value);
    assert_eq!(native_value, Value::Text("[1,2]".into()));
    assert_eq!(native.native_deoptimizations(), 0);
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "append"));
}

#[test]
fn native_text_add_preserves_unicode_and_vm_limits() {
    let artifact = compile(
        "native-text-add.gu",
        "fn concatenate(left: Text, right: Text) -> Text:\n    return left + right\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let args = vec![Value::Text("a🙂".into()), Value::Text("é".into())];
    let native_value = native.invoke("concatenate", args.clone()).unwrap();
    let vm_value = vm.invoke("concatenate", args.clone()).unwrap();
    assert_eq!(native_value, vm_value);
    assert_eq!(native_value, Value::Text("a🙂é".into()));
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "concatenate"));

    let limited = HostPolicy {
        max_text_bytes: 6,
        ..HostPolicy::default()
    };
    let mut native = Runtime::new()
        .load(artifact.clone(), limited.clone())
        .unwrap();
    let mut vm = Runtime::vm_only().load(artifact, limited).unwrap();
    let native_error = native.invoke("concatenate", args.clone()).unwrap_err();
    let vm_error = vm.invoke("concatenate", args).unwrap_err();
    assert_eq!(native_error.code, vm_error.code);
    assert_eq!(native_error.code, "R_TEXT_LIMIT");
}

#[test]
fn native_unicode_collapse_preserves_text_limits_and_no_change_path() {
    let root = std::env::temp_dir().join(format!(
        "guion-native-unicode-collapse-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let entry = root.join("main.gu");
    std::fs::write(
        &entry,
        "import \"std:unicode\"\n\nfn collapse() -> Text:\n    return unicode_collapse_whitespace(\"  alpha\\t beta  \")\n\nfn unchanged() -> Text:\n    return unicode_collapse_whitespace(\"alpha\")\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let stdlib_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib");
    let artifact = compile_project_with_roots(
        &entry,
        &root,
        Some(&stdlib_root),
        &std::collections::HashMap::new(),
    )
    .unwrap();
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    assert_eq!(
        native.invoke("collapse", vec![]).unwrap(),
        Value::Text("alpha beta".into())
    );
    assert_eq!(
        native.invoke("collapse", vec![]).unwrap(),
        vm.invoke("collapse", vec![]).unwrap()
    );
    assert_eq!(
        native.invoke("unchanged", vec![]).unwrap(),
        Value::Text("alpha".into())
    );
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "collapse"));
    assert!(native.native_calls() > 0);

    let limited = HostPolicy {
        max_text_bytes: 5,
        ..HostPolicy::default()
    };
    let mut native = Runtime::new()
        .load(artifact.clone(), limited.clone())
        .unwrap();
    let mut vm = Runtime::vm_only().load(artifact, limited).unwrap();
    let native_error = native.invoke("collapse", vec![]).unwrap_err();
    let vm_error = vm.invoke("collapse", vec![]).unwrap_err();
    assert_eq!(native_error.code, vm_error.code);
    assert_eq!(native_error.code, "R_TEXT_LIMIT");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_map_index_preserves_values_and_vm_errors() {
    let artifact = compile(
        "native-map-index.gu",
        "fn value_at(values: Map[Text, Text], key: Text) -> Text:\n    return values[key]\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let values = Value::Map(std::sync::Arc::new(guion_model::TextMap::from([(
        "answer".to_owned(),
        Value::Text("forty-two".into()),
    )])));
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .unwrap();
    let args = vec![values.clone(), Value::Text("answer".into())];
    let native_value = native.invoke("value_at", args.clone()).unwrap();
    let vm_value = vm.invoke("value_at", args).unwrap();
    assert_eq!(native_value, vm_value);
    assert_eq!(native_value, Value::Text("forty-two".into()));
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "value_at"));

    let args = vec![values, Value::Text("missing".into())];
    let native_error = native.invoke("value_at", args.clone()).unwrap_err();
    let vm_error = vm.invoke("value_at", args).unwrap_err();
    assert_eq!(native_error.code, vm_error.code);
    assert_eq!(native_error.code, "R_MAP_KEY_MISSING");
}

#[test]
fn native_map_has_preserves_membership_results() {
    let artifact = compile(
        "native-map-has.gu",
        "fn contains(values: Map[Text, Bool], key: Text) -> Bool:\n    return map_has(values, key)\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let values = Value::Map(std::sync::Arc::new(guion_model::TextMap::from([(
        "service-a".to_owned(),
        Value::Bool(true),
    )])));
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .unwrap();
    for (key, expected) in [("service-a", true), ("service-missing", false)] {
        let args = vec![values.clone(), Value::Text(key.into())];
        let native_value = native.invoke("contains", args.clone()).unwrap();
        let vm_value = vm.invoke("contains", args).unwrap();
        assert_eq!(native_value, vm_value);
        assert_eq!(native_value, Value::Bool(expected));
    }
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "contains"));
}

#[test]
fn native_map_set_preserves_copy_on_write_and_vm_limits() {
    let artifact = compile(
        "native-map-set.gu",
        "fn update(values: Map[Text, Text], key: Text, value: Text) -> Map[Text, Text]:\n    var copy = values\n    set copy = map_set(copy, key, value)\n    return copy\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let values = Value::Map(std::sync::Arc::new(guion_model::TextMap::from([(
        "answer".to_owned(),
        Value::Text("old".into()),
    )])));
    let args = vec![
        values.clone(),
        Value::Text("answer".into()),
        Value::Text("new".into()),
    ];
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let native_value = native.invoke("update", args.clone()).unwrap();
    let vm_value = vm.invoke("update", args).unwrap();
    assert_eq!(native_value, vm_value);
    assert_eq!(
        native_value,
        Value::Map(std::sync::Arc::new(guion_model::TextMap::from([(
            "answer".to_owned(),
            Value::Text("new".into())
        ),])))
    );
    assert_eq!(
        values,
        Value::Map(std::sync::Arc::new(guion_model::TextMap::from([(
            "answer".to_owned(),
            Value::Text("old".into())
        ),])))
    );
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "update"));

    let limited = HostPolicy {
        max_collection_items: 0,
        ..HostPolicy::default()
    };
    let mut native = Runtime::new()
        .load(artifact.clone(), limited.clone())
        .unwrap();
    let mut vm = Runtime::vm_only().load(artifact, limited).unwrap();
    let args = vec![
        Value::Map(std::sync::Arc::new(guion_model::TextMap::new())),
        Value::Text("new".into()),
        Value::Text("value".into()),
    ];
    let native_error = native.invoke("update", args.clone()).unwrap_err();
    let vm_error = vm.invoke("update", args).unwrap_err();
    assert_eq!(native_error.code, vm_error.code);
    assert_eq!(native_error.code, "R_COLLECTION_LIMIT");
}

#[test]
fn native_text_chars_preserves_unicode_and_vm_limits() {
    let artifact = compile(
        "native-text-chars.gu",
        "fn text_char_count(value: Text) -> Int:\n    return len(text_chars(value))\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let input = Value::Text("a🙂é".into());
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let expected = Value::Int(BigInt::from_i64(3));
    assert_eq!(
        native
            .invoke("text_char_count", vec![input.clone()])
            .unwrap(),
        expected
    );
    assert_eq!(
        vm.invoke("text_char_count", vec![input.clone()]).unwrap(),
        expected
    );
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "text_char_count"));

    for policy in [
        HostPolicy {
            allocation_limit_bytes: 0,
            ..HostPolicy::default()
        },
        HostPolicy {
            max_collection_items: 2,
            ..HostPolicy::default()
        },
    ] {
        let mut native = Runtime::new()
            .load(artifact.clone(), policy.clone())
            .unwrap();
        let mut vm = Runtime::vm_only().load(artifact.clone(), policy).unwrap();
        let native_error = native
            .invoke("text_char_count", vec![input.clone()])
            .unwrap_err();
        let vm_error = vm
            .invoke("text_char_count", vec![input.clone()])
            .unwrap_err();
        assert_eq!(native_error.code, vm_error.code);
    }
}

#[test]
fn native_text_chars_index_charges_the_cloned_scalar() {
    let artifact = compile(
        "native-text-chars-index.gu",
        "fn first(value: Text) -> Text:\n    return text_chars(value)[0]\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let policy = HostPolicy {
        allocation_limit_bytes: 177,
        ..HostPolicy::default()
    };
    let input = vec![Value::Text("a🙂".into())];
    let sufficient = HostPolicy {
        allocation_limit_bytes: 178,
        ..HostPolicy::default()
    };
    assert_eq!(
        Runtime::new()
            .load(artifact.clone(), sufficient.clone())
            .unwrap()
            .invoke("first", input.clone())
            .unwrap(),
        Runtime::vm_only()
            .load(artifact.clone(), sufficient)
            .unwrap()
            .invoke("first", input.clone())
            .unwrap()
    );
    let mut native = Runtime::new()
        .load(artifact.clone(), policy.clone())
        .unwrap();
    let mut vm = Runtime::vm_only().load(artifact, policy).unwrap();

    let native_result = native.invoke("first", input.clone());
    let vm_result = vm.invoke("first", input);
    match (native_result, vm_result) {
        (Err(native), Err(vm)) => {
            assert_eq!(native.code, vm.code);
            assert_eq!(native.code, "R_ALLOCATION_LIMIT");
        }
        (native, vm) => panic!("native/VM allocation outcomes differ: {native:?} vs {vm:?}"),
    }
    assert!(native
        .native_function_names()
        .iter()
        .any(|name| name == "first"));
}

#[cfg(feature = "native-value-bridge")]
#[test]
fn native_loop_safepoints_retire_dead_handles_without_changing_results() {
    let source = "fn main() -> Int:\n    var index = 0\n    var total = 0\n    var report = json_object()\n    while index < 12000:\n        set report = json_set(report, \"last\", json_int(index))\n        set total = total + index\n        set index = index + 1\n    return total\n";
    let artifact = compile("native-loop-safepoint.gu", source).expect("safepoint fixture compiles");
    let mut native = Runtime::new()
        .load(artifact.clone(), HostPolicy::default())
        .unwrap();
    let mut vm = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .unwrap();
    let native_value = native.invoke("main", vec![]).unwrap();
    assert_eq!(native_value, vm.invoke("main", vec![]).unwrap());
    assert_eq!(native_value, Value::Int(BigInt::from_usize(71_994_000)));
    assert!(
        native
            .native_function_names()
            .iter()
            .any(|name| name == "main"),
        "{}",
        native.native_compile_profile()
    );
}
