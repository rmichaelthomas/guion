use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_RUN: AtomicUsize = AtomicUsize::new(0);

fn run(source: &str) -> std::process::Output {
    let dir = std::env::temp_dir().join(format!(
        "guion-native-record-{}-{}",
        std::process::id(),
        NEXT_RUN.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.gu"), source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .arg("run")
        .arg("main.gu")
        .current_dir(&dir)
        .output()
        .expect("guion starts");
    let _ = std::fs::remove_dir_all(&dir);
    output
}

// A function that returns a record holding Text must behave the same whether it is called
// directly, through another function, or inside Err(...).
#[test]
fn text_records_returned_through_helpers_and_err_survive_native_calls() {
    let source = r#"import "std:types"

record Pair:
    label: Text
    count: Int

fn make(label: Text) -> Pair:
    return Pair(label, 1)

fn wrap(label: Text) -> Pair:
    return make(label)

fn wrap_error(a: Text, b: Text) -> StdError:
    return std_error(a, b)

fn fail(x: Int) -> Result[Int, StdError]:
    if x > 0:
        return Err(wrap_error("E_X", "failed"))
    return Ok(x)

fn main() -> Int:
    assert(wrap("a").label == "a")
    assert(wrap("a").count == 1)
    match fail(1):
        Ok(value):
            return 1
        Err(problem):
            assert(problem.code == "E_X")
            assert(problem.message == "failed")
    match fail(0):
        Ok(value):
            assert(value == 0)
        Err(problem):
            return 2
    return 0
"#;
    let output = run(source);
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

// A list field of a record that has no native layout (it also holds Text, or a list of
// records) reaches native code through the host bridge. The bridge must hand back a native list,
// not an arena handle that machine code would dereference as an address.
#[test]
fn list_fields_of_bridged_records_are_native_lists_in_callees() {
    let source = r#"record Item:
    us: Int

record Bag:
    name: Text
    numbers: List[Int]
    flags: List[Bool]
    ratios: List[Float64]
    words: List[Text]
    items: List[Item]

fn total(bag: Bag) -> Int:
    var sum = len(bag.numbers) + len(bag.flags) + len(bag.ratios) + len(bag.words)
    for number in bag.numbers:
        set sum = sum + number
    for item in bag.items:
        set sum = sum + item.us
    return sum

fn show(bag: Bag) -> Unit:
    for word in bag.words:
        print(word)
    for item in bag.items:
        print(item.us)

fn main() -> Int:
    let bag = Bag("b", [1, 2], [true], [0.5], ["x", "y"], [Item(10), Item(20)])
    show(bag)
    assert(total(bag) == 39)
    return 0
"#;
    let output = run(source);
    assert!(
        output.status.success(),
        "{:?}\n{}{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "x\ny\n10\n20\n");
}
