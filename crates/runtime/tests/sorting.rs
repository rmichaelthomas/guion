//! General `sort` and `sort_by` (artifact format 65): stable, natural orders for Text, Int and
//! Float64, key evaluated once per item, native comparisons charged to the instruction budget.

use guion_compiler::compile;
use guion_model::{Artifact, Op};
use guion_runtime::{HostPolicy, Runtime};

fn run_with(source: &str, policy: HostPolicy) -> Result<String, (String, String)> {
    let artifact = compile("sorting.gu", source)
        .unwrap_or_else(|error| panic!("{} {}", error.code, error.message));
    run_artifact(artifact, policy)
}

fn run_artifact(artifact: Artifact, policy: HostPolicy) -> Result<String, (String, String)> {
    let mut instance = Runtime::vm_only()
        .load(artifact, policy)
        .expect("artifact loads");
    match instance.invoke("main", vec![]) {
        Ok(_) => Ok(instance.take_output()),
        Err(error) => Err((error.code.to_string(), error.message)),
    }
}

fn run(source: &str) -> String {
    run_with(source, HostPolicy::default()).unwrap_or_else(|(code, message)| {
        panic!("{code}: {message}");
    })
}

#[test]
fn sort_orders_text_int_and_float64_lists() {
    let out = run(r#"fn main() -> Unit:
    print(sort(["bb", "ccc", "a", "B", ""]))
    print(sort([3, -1, 2, 10, -20, 2]))
    print(sort([2.5, -1.5, 10.0, 0.25]))
    print(sort([100000000000000000000, -100000000000000000000, 5]))
"#);
    assert_eq!(
        out,
        "[, B, a, bb, ccc]\n[-20, -1, 2, 2, 3, 10]\n[-1.5, 0.25, 2.5, 10]\n[-100000000000000000000, 5, 100000000000000000000]\n"
    );
}

#[test]
fn sort_handles_empty_and_one_element_lists() {
    let out = run(r#"fn text_key(value: Text) -> Text:
    return value

fn main() -> Unit:
    let none_text: List[Text] = []
    let none_int: List[Int] = []
    let none_float: List[Float64] = []
    print(len(sort(none_text)))
    print(len(sort(none_int)))
    print(len(sort(none_float)))
    print(sort(["only"]))
    print(sort([7]))
    print(sort([1.5]))
    print(len(sort_by(none_text, text_key)))
    print(sort_by(["one"], text_key))
"#);
    assert_eq!(out, "0\n0\n0\n[only]\n[7]\n[1.5]\n0\n[one]\n");
}

#[test]
fn sort_does_not_change_its_input() {
    let out = run(r#"fn main() -> Unit:
    let original = [3, 1, 2]
    let sorted = sort(original)
    print(original)
    print(sorted)
"#);
    assert_eq!(out, "[3, 1, 2]\n[1, 2, 3]\n");
}

#[test]
fn sort_by_orders_by_text_and_by_len_from_shuffled_input() {
    let out = run(r#"fn same(word: Text) -> Text:
    return word

fn length(word: Text) -> Int:
    return len(word)

fn main() -> Unit:
    let words = ["bb", "a", "ccc"]
    print(sort_by(words, same))
    print(sort_by(words, length))
    print(sort_by(["ccc", "bb", "a"], same))
    print(sort_by(["ccc", "a", "bb"], length))
    print(sort_by(["bb", "ccc", "a"], length))
    print(words)
"#);
    assert_eq!(
        out,
        "[a, bb, ccc]\n[a, bb, ccc]\n[a, bb, ccc]\n[a, bb, ccc]\n[a, bb, ccc]\n[bb, a, ccc]\n"
    );
}

#[test]
fn sort_by_is_stable_for_every_key_type() {
    let out = run(r#"record Item:
    name: Text
    rank: Int
    weight: Float64
    order: Int

fn by_name(item: Item) -> Text:
    return item.name

fn by_rank(item: Item) -> Int:
    return item.rank

fn by_weight(item: Item) -> Float64:
    return item.weight

fn orders(items: List[Item]) -> Text:
    var out = ""
    for item in items:
        set out = out + int_to_text(item.order)
    return out

fn main() -> Unit:
    var items: List[Item] = []
    var index = 0
    while index < 40:
        set items = list_push(items, Item("n" + int_to_text(index % 3), index % 4, 1.5 * int_to_float(index % 2), index))
        set index = index + 1
    print(orders(sort_by(items, by_name)))
    print(orders(sort_by(items, by_rank)))
    print(orders(sort_by(items, by_weight)))
"#);
    let expected = |key: &dyn Fn(usize) -> usize| {
        let mut order: Vec<usize> = (0..40).collect();
        order.sort_by_key(|index| key(*index));
        order
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join("")
    };
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], expected(&|index| index % 3));
    assert_eq!(lines[1], expected(&|index| index % 4));
    assert_eq!(lines[2], expected(&|index| index % 2));
}

#[test]
fn sort_by_evaluates_the_key_once_per_item() {
    // The key function counts its calls through the sort order it produces: a key that
    // returned a different value on a second call would break the order, and the number of
    // key evaluations is bounded by the instruction budget below (a comparison-time key would
    // cost log n calls per item).
    let source = r#"fn spend(value: Int) -> Int:
    var total = 0
    var step = 0
    while step < 20:
        set total = total + value
        set step = step + 1
    return total

fn main() -> Unit:
    var values: List[Int] = []
    var index = 0
    while index < 2000:
        set values = list_push(values, (index * 7919) % 2000)
        set index = index + 1
    let sorted = sort_by(values, spend)
    print(sorted[0])
    print(sorted[1999])
"#;
    // 2000 items x about 130 instructions of key work = 260k; log2(2000) = 11 evaluations per
    // item would be about 2.9M and cannot fit.
    let policy = HostPolicy {
        instruction_limit: 1_000_000,
        ..HostPolicy::default()
    };
    assert_eq!(run_with(source, policy).unwrap(), "0\n1999\n");
}

#[test]
fn sort_float64_zero_signs_and_stability() {
    let out = run(r#"record Cell:
    id: Int
    value: Float64

fn by_value(cell: Cell) -> Float64:
    return cell.value

fn ids(cells: List[Cell]) -> Text:
    var out = ""
    for cell in cells:
        set out = out + int_to_text(cell.id)
    return out

fn main() -> Unit:
    print(sort([0.0, -0.0, 1.0, -1.0, 0.0, -0.0]))
    let cells = [Cell(1, 0.0), Cell(2, -0.0), Cell(3, 0.0), Cell(4, -0.0), Cell(5, -1.0), Cell(6, 1.0)]
    print(ids(sort_by(cells, by_value)))
"#);
    assert_eq!(out, "[-1, -0, -0, 0, 0, 1]\n524136\n");
}

#[test]
fn sort_text_order_equals_sorted_map_key_order() {
    let out = run(r#"fn main() -> Unit:
    let keys = ["\u{10000}", "b", "\u{FFFF}", "a", "\u{E000}", "\u{7F}", "ab", "\u{10FFFF}", "a\u{10000}", "a\u{FFFF}", ""]
    var table: Map[Text, Int] = {}
    var index = 0
    for key in keys:
        set table = map_set(table, key, index)
        set index = index + 1
    var in_map_order: List[Text] = []
    for key in table:
        set in_map_order = list_push(in_map_order, key)
    let sorted = sort(keys)
    assert(len(sorted) == len(in_map_order))
    var position = 0
    while position < len(sorted):
        assert(sorted[position] == in_map_order[position])
        set position = position + 1
    print(sorted[0] == "")
    print(sorted[1])
    print(sorted[2])
    print(sorted[3])
    print(sorted[4])
    print(sorted[5])
    print(sorted[6] == "\u{7F}")
    print(sorted[7] == "\u{E000}")
    print(sorted[8] == "\u{FFFF}")
    print(sorted[9] == "\u{10000}")
    print(sorted[10] == "\u{10FFFF}")
"#);
    // "ab" sorts before "a\u{FFFF}": 'b' (U+0062) is below U+FFFF, as scalar order requires.
    assert_eq!(
        out,
        "true\na\nab\na\u{FFFF}\na\u{10000}\nb\ntrue\ntrue\ntrue\ntrue\ntrue\n"
    );
}

#[test]
fn sort_of_100000_text_keys_fits_the_default_instruction_budget() {
    let root = std::env::temp_dir().join(format!("guion-sort-100k-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    // 100,000 distinct keys in a scrambled order, one per line.
    let mut data = String::new();
    for index in 0..100_000u64 {
        let scrambled = (index * 48_271) % 100_003;
        data.push_str(&format!("key{scrambled:06}\n"));
    }
    std::fs::write(root.join("keys.txt"), &data).unwrap();
    let source = r#"import "std:text"

fn main() -> Unit:
    let loaded = read_text("keys.txt")
    let parts = text_split(unwrap(loaded), "\n")
    let sorted = sort(parts)
    print(len(sorted))
    print(sorted[0])
    print(sorted[1])
    print(sorted[len(sorted) - 1])
"#;
    let entry = root.join("main.gu");
    std::fs::write(&entry, source).unwrap();
    let stdlib = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib");
    let artifact = guion_compiler::compile_project_with_roots(
        &entry,
        &root,
        Some(&stdlib),
        &std::collections::HashMap::new(),
    )
    .expect("program compiles");
    let policy = HostPolicy::default().with_file_access(&root, true, false);
    assert_eq!(policy.instruction_limit, 2_000_000, "the default budget");
    let out = run_artifact(artifact, policy).expect("100,000 keys sort inside the default budget");
    let _ = std::fs::remove_dir_all(&root);
    let mut expected: Vec<String> = data.split('\n').map(str::to_string).collect();
    expected.sort();
    assert_eq!(
        out,
        format!(
            "{}\n{}\n{}\n{}\n",
            expected.len(),
            expected[0],
            expected[1],
            expected[expected.len() - 1]
        )
    );
}

#[test]
fn sort_charges_native_comparisons_to_the_instruction_budget() {
    let source = r#"fn main() -> Unit:
    var values: List[Int] = []
    var index = 0
    while index < 4000:
        set values = list_push(values, (index * 7919) % 4000)
        set index = index + 1
    let sorted = sort(values)
    print(sorted[0])
"#;
    // About 4000 x 8 instructions to build, 4000 x log2(4000) comparisons to sort: a ceiling
    // of 50,000 must stop inside the sort, not before the loop ends.
    let policy = HostPolicy {
        instruction_limit: 50_000,
        ..HostPolicy::default()
    };
    let (code, message) = run_with(source, policy).unwrap_err();
    assert_eq!(code, "R_STEP_LIMIT");
    assert!(message.contains("instructions used"), "{message}");
}

fn expect_check_error(source: &str, code: &str, needle: &str) {
    let error = compile("sorting.gu", source).expect_err("the sort is rejected at check time");
    assert_eq!(error.code, code, "{}", error.message);
    assert!(
        error.message.contains(needle),
        "`{}` should contain `{needle}`",
        error.message
    );
}

#[test]
fn sort_by_rejects_bool_unit_and_other_key_types_with_e_sort_key() {
    expect_check_error(
        "fn flag(value: Int) -> Bool:\n    return value > 1\n\nfn main() -> Int:\n    return len(sort_by([3, 1, 2], flag))\n",
        "E_SORT_KEY",
        "Text, Int or Float64",
    );
    expect_check_error(
        "fn nothing(value: Int) -> Unit:\n    print(value)\n\nfn main() -> Int:\n    return len(sort_by([3, 1, 2], nothing))\n",
        "E_SORT_KEY",
        "Text, Int or Float64",
    );
    expect_check_error(
        "fn pair(value: Int) -> List[Int]:\n    return [value]\n\nfn main() -> Int:\n    return len(sort_by([3, 1, 2], pair))\n",
        "E_SORT_KEY",
        "Text, Int or Float64",
    );
    // The key must take the list's item type.
    expect_check_error(
        "fn by_text(value: Text) -> Int:\n    return len(value)\n\nfn main() -> Int:\n    return len(sort_by([3, 1, 2], by_text))\n",
        "E_SORT_KEY",
        "(T) -> K",
    );
}

#[test]
fn sort_rejects_lists_that_have_no_natural_order() {
    expect_check_error(
        "fn main() -> Int:\n    return len(sort([true, false]))\n",
        "E_SORT_KEY",
        "Text, Int or Float64",
    );
    expect_check_error(
        "fn main() -> Int:\n    return len(sort(3))\n",
        "E_TYPE_ARGUMENT",
        "List",
    );
}

#[test]
fn sort_by_key_function_must_be_pure() {
    expect_check_error(
        "fn loud(value: Int) -> Int:\n    print(value)\n    return value\n\nfn main() -> Int:\n    return len(sort_by([3, 1], loud))\n",
        "E_SORT_EFFECT",
        "",
    );
}

#[test]
fn the_old_int_key_and_comparator_sorts_still_work() {
    let out = run(r#"record Item:
    key: Int
    order: Int

fn item_key(item: Item) -> Int:
    return item.key

fn compare_items(left: Item, right: Item) -> Int:
    return left.key - right.key

fn orders(items: List[Item]) -> Text:
    var out = ""
    for item in items:
        set out = out + int_to_text(item.order)
    return out

fn main() -> Unit:
    let items = [Item(1, 1), Item(0, 2), Item(1, 3), Item(0, 4)]
    print(orders(sort_by_int_key(items, item_key)))
    print(orders(sort_by(items, compare_items)))
    print(orders(sort_by(items, item_key)))
    print(len(merge_sorted_by_int_key([[Item(0, 1), Item(2, 2)], [Item(1, 3)]], item_key)))
"#);
    assert_eq!(out, "2413\n2413\n2413\n3\n");
}

#[test]
fn sort_and_sort_by_use_format_65_and_older_artifacts_are_unchanged() {
    assert!(Artifact::CURRENT_VERSION >= 65);
    let natural = compile(
        "natural.gu",
        "fn main() -> Int:\n    return sort([2, 1])[0]\n",
    )
    .unwrap();
    assert_eq!(natural.format_version, Artifact::CURRENT_VERSION);
    assert!(natural
        .functions
        .iter()
        .flat_map(|function| function.code.iter())
        .any(|instruction| matches!(instruction.op, Op::SortNatural)));
    let keyed = compile(
        "keyed.gu",
        "fn key(value: Int) -> Int:\n    return value\n\nfn main() -> Int:\n    return sort_by([2, 1], key)[0]\n",
    )
    .unwrap();
    assert!(keyed
        .functions
        .iter()
        .flat_map(|function| function.code.iter())
        .any(|instruction| matches!(instruction.op, Op::SortByKey(_))));
    // An artifact below 65 that uses either is rejected, on write and on read.
    for artifact in [natural, keyed] {
        let bytes = artifact.to_bytes().unwrap();
        let mut old = artifact.clone();
        old.format_version = 64;
        assert_eq!(old.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");
        let mut legacy = bytes;
        legacy[4..8].copy_from_slice(&64u32.to_le_bytes());
        assert_eq!(
            Artifact::from_bytes(&legacy).unwrap_err().code,
            "E_ARTIFACT_VERSION"
        );
    }
}

#[test]
fn a_format_64_artifact_still_loads_and_runs() {
    // The int-key and comparator sorts keep their format-36 and format-41 operations, so an
    // artifact that uses them can be written, read and run at format 64.
    let source = r#"record Item:
    key: Int
    order: Int

fn item_key(item: Item) -> Int:
    return item.key

fn compare_items(left: Item, right: Item) -> Int:
    return left.key - right.key

fn main() -> Unit:
    let items = [Item(1, 1), Item(0, 2), Item(1, 3)]
    print(sort_by_int_key(items, item_key)[0].order)
    print(sort_by(items, compare_items)[2].order)
"#;
    let mut artifact = compile("old.gu", source).unwrap();
    artifact.format_version = Artifact::CURRENT_VERSION - 1;
    let bytes = artifact.to_bytes().expect("format 64 can still be written");
    let reread = Artifact::from_bytes(&bytes).expect("format 64 can still be read");
    assert_eq!(reread.format_version, Artifact::CURRENT_VERSION - 1);
    let out = run_artifact(reread, HostPolicy::default()).unwrap();
    assert_eq!(out, "2\n3\n");
}
