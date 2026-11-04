use std::process::Command;

fn run(source: &str) -> std::process::Output {
    let dir = std::env::temp_dir().join(format!("guion-native-record-{}", std::process::id()));
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
