//! `else if`, `for i in range(...)`, compound assignment (`+=`), f-text and private record fields.

use std::process::Command;

static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn run(source: &str) -> (bool, String, String) {
    let directory = std::env::temp_dir().join(format!(
        "guion-syntax-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("main.gu"), source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .arg("run")
        .arg("main.gu")
        .current_dir(&directory)
        .output()
        .expect("guion starts");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn run_project(files: &[(&str, &str)]) -> (bool, String, String) {
    let directory = std::env::temp_dir().join(format!(
        "guion-syntax-project-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&directory).unwrap();
    for (name, text) in files {
        std::fs::write(directory.join(name), text).unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_guion"))
        .arg("run")
        .arg("main.gu")
        .current_dir(&directory)
        .output()
        .expect("guion starts");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn else_if_chains_pick_the_first_true_branch() {
    let source = "fn grade(n: Int) -> Text:\n    if n >= 90:\n        return \"A\"\n    else if n >= 80:\n        return \"B\"\n    else if n >= 70:\n        return \"C\"\n    else:\n        return \"F\"\n\nfn main() -> Unit:\n    print(grade(95))\n    print(grade(85))\n    print(grade(75))\n    print(grade(10))\n";
    let (ok, out, err) = run(source);
    assert!(ok, "{err}");
    assert_eq!(out, "A\nB\nC\nF\n");
}

#[test]
fn else_if_without_a_final_else_falls_through() {
    let (ok, out, err) = run(
        "fn main() -> Unit:\n    let n = 5\n    if n == 1:\n        print(\"one\")\n    else if n == 2:\n        print(\"two\")\n    print(\"done\")\n",
    );
    assert!(ok, "{err}");
    assert_eq!(out, "done\n");
}

#[test]
fn elif_still_points_at_else_if() {
    let (ok, _, err) = run(
        "fn main() -> Unit:\n    if true:\n        print(1)\n    elif false:\n        print(2)\n",
    );
    assert!(!ok);
    assert!(
        err.contains("E_PYTHON_ELIF") && err.contains("else if"),
        "{err}"
    );
}

#[test]
fn range_counts_up_down_and_by_steps() {
    let source = "fn main() -> Unit:\n    var up = 0\n    for i in range(4):\n        set up += i\n    print(up)\n    var middle = \"\"\n    for i in range(2, 5):\n        set middle += f\"{i},\"\n    print(middle)\n    var down = \"\"\n    for i in range(5, 0, -2):\n        set down += f\"{i} \"\n    print(down)\n    var none = 0\n    for i in range(3, 3):\n        set none += 1\n    for i in range(3, 0):\n        set none += 1\n    print(none)\n";
    let (ok, out, err) = run(source);
    assert!(ok, "{err}");
    assert_eq!(out, "6\n2,3,4,\n5 3 1 \n0\n");
}

#[test]
fn continue_and_break_work_in_a_range_loop() {
    let source = "fn main() -> Unit:\n    var seen = \"\"\n    for i in range(10):\n        if i % 2 == 0:\n            continue\n        if i > 7:\n            break\n        set seen += f\"{i}\"\n    print(seen)\n";
    let (ok, out, err) = run(source);
    assert!(ok, "{err}");
    assert_eq!(out, "1357\n");
}

#[test]
fn range_outside_a_for_header_and_a_zero_step_are_errors() {
    let (ok, _, err) = run("fn main() -> Unit:\n    let xs = range(3)\n");
    assert!(!ok);
    assert!(err.contains("E_RANGE_CONTEXT"), "{err}");
    let (ok, _, err) = run("fn main() -> Unit:\n    for i in range(0, 3, 0):\n        print(i)\n");
    assert!(!ok);
    assert!(err.contains("E_RANGE_STEP"), "{err}");
}

#[test]
fn compound_assignment_works_on_locals_and_fields() {
    let source = "record Counter:\n    n: Int\n\nfn main() -> Unit:\n    var x = 10\n    set x += 5\n    set x -= 3\n    set x *= 2\n    set x //= 5\n    set x %= 3\n    print(x)\n    var c = Counter(1)\n    set c.n += 41\n    print(c.n)\n    var s = \"a\"\n    set s += \"b\"\n    print(s)\n";
    let (ok, out, err) = run(source);
    assert!(ok, "{err}");
    assert_eq!(out, "1\n42\nab\n");
}

#[test]
fn f_text_fills_holes_with_text_int_and_float() {
    let source = "record P:\n    x: Int\n    name: Text\n\nfn main() -> Unit:\n    let p = P(3, \"Ada\")\n    let n = 7\n    print(f\"n is {n}\")\n    print(f\"{p.name} at {p.x + 1}: {2.5}\")\n    print(f\"{{literal}} {n * 2}\")\n    print(f\"\")\n";
    let (ok, out, err) = run(source);
    assert!(ok, "{err}");
    assert_eq!(out, "n is 7\nAda at 4: 2.5\n{literal} 14\n\n");
}

#[test]
fn f_text_holes_of_other_types_and_malformed_holes_are_errors() {
    let (ok, _, err) = run("fn main() -> Unit:\n    print(f\"{true}\")\n");
    assert!(!ok);
    assert!(err.contains("E_FSTRING_TYPE"), "{err}");
    let (ok, _, err) = run("fn main() -> Unit:\n    print(f\"{\")\n");
    assert!(!ok);
    assert!(err.contains("E_FSTRING_HOLE"), "{err}");
    let (ok, _, err) = run("fn main() -> Unit:\n    print(f\"{}\")\n");
    assert!(!ok);
    assert!(err.contains("E_FSTRING_HOLE"), "{err}");
}

#[test]
fn other_python_string_prefixes_still_get_the_recovery_error() {
    let (ok, _, err) = run("fn main() -> Unit:\n    print(rf\"x\")\n");
    assert!(!ok);
    assert!(err.contains("E_PYTHON_FSTRING"), "{err}");
}

#[test]
fn private_fields_are_usable_in_their_module_and_refused_elsewhere() {
    let library = "pub record Account:\n    id: Int\n    priv balance: Int\n\npub fn open(id: Int) -> Account:\n    return Account(id, 100)\n\npub fn balance_of(account: Account) -> Int:\n    return account.balance\n\npub fn deposit(account: Account, amount: Int) -> Account:\n    var next = account\n    set next.balance += amount\n    return next\n";
    let ok_main = "import \"bank.gu\"\n\nfn main() -> Unit:\n    let a = deposit(open(1), 50)\n    print(a.id)\n    print(balance_of(a))\n";
    let (ok, out, err) = run_project(&[("bank.gu", library), ("main.gu", ok_main)]);
    assert!(ok, "{err}");
    assert_eq!(out, "1\n150\n");

    let read =
        "import \"bank.gu\"\n\nfn main() -> Unit:\n    let a = open(1)\n    print(a.balance)\n";
    let (ok, _, err) = run_project(&[("bank.gu", library), ("main.gu", read)]);
    assert!(!ok);
    assert!(err.contains("E_FIELD_PRIVATE"), "{err}");

    let build = "import \"bank.gu\"\n\nfn main() -> Unit:\n    let a = Account(1, 999999)\n    print(a.id)\n";
    let (ok, _, err) = run_project(&[("bank.gu", library), ("main.gu", build)]);
    assert!(!ok);
    assert!(err.contains("E_FIELD_PRIVATE"), "{err}");

    let assign =
        "import \"bank.gu\"\n\nfn main() -> Unit:\n    var a = open(1)\n    set a.balance = 5\n";
    let (ok, _, err) = run_project(&[("bank.gu", library), ("main.gu", assign)]);
    assert!(!ok);
    assert!(err.contains("E_FIELD_PRIVATE"), "{err}");
}

#[test]
fn a_field_named_priv_is_still_a_plain_field() {
    let (ok, out, err) = run(
        "record R:\n    priv: Int\n\nfn main() -> Unit:\n    let r = R(4)\n    print(r.priv)\n",
    );
    assert!(ok, "{err}");
    assert_eq!(out, "4\n");
}

#[test]
fn the_formatter_keeps_the_new_forms_valid() {
    let directory = std::env::temp_dir().join(format!("guion-fmt-syntax-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let file = directory.join("main.gu");
    std::fs::write(
        &file,
        "fn main() -> Unit:\n    var total=0\n    for i in range(0,5):\n        set total+=i\n        set total//=1\n    if total>3:\n        print(f\"total is {total}\")\n    else if total==3:\n        print(\"three\")\n",
    )
    .unwrap();
    let formatted = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["fmt", "main.gu"])
        .current_dir(&directory)
        .output()
        .expect("guion starts");
    assert!(formatted.status.success());
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("set total += i"), "{text}");
    assert!(text.contains("set total //= 1"), "{text}");
    assert!(text.contains("print(f\"total is {total}\")"), "{text}");
    assert!(text.contains("for i in range(0, 5):"), "{text}");
    let run = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "main.gu"])
        .current_dir(&directory)
        .output()
        .expect("guion starts");
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "total is 10\n");
}

#[test]
fn pass_lets_a_match_arm_do_nothing() {
    let source = "fn check(value: Option[Int]) -> Unit:\n    match value:\n        Some(n):\n            print(f\"got {n}\")\n        None:\n            pass\n\nfn maybe(flag: Bool) -> Option[Int]:\n    if flag:\n        return Some(4)\n    return None\n\nfn main() -> Unit:\n    check(maybe(true))\n    check(maybe(false))\n    print(\"done\")\n";
    let (ok, out, err) = run(source);
    assert!(ok, "{err}");
    assert_eq!(out, "got 4\ndone\n");
}

#[test]
fn pass_can_stand_in_every_block_and_beside_other_statements() {
    let source = "fn main() -> Unit:\n    var total = 0\n    for i in range(3):\n        pass\n    while total < 2:\n        set total += 1\n        pass\n    if total == 2:\n        pass\n    else:\n        print(\"no\")\n    print(total)\n";
    let (ok, out, err) = run(source);
    assert!(ok, "{err}");
    assert_eq!(out, "2\n");
}

#[test]
fn pass_is_only_a_statement_so_it_stays_usable_as_a_name() {
    let source = "fn main() -> Unit:\n    let pass = 7\n    print(pass)\n";
    let (ok, out, err) = run(source);
    assert!(ok, "{err}");
    assert_eq!(out, "7\n");
}

#[test]
fn an_empty_block_diagnostic_points_to_pass() {
    let source = "fn main() -> Unit:\n    if true:\n    print(1)\n";
    let (ok, _out, err) = run(source);
    assert!(!ok);
    assert!(err.contains("E_BLOCK_EMPTY"), "{err}");
    assert!(err.contains("pass"), "{err}");
}

#[test]
fn a_function_whose_only_statement_is_pass_has_no_value_to_return() {
    let source = "fn nothing() -> Int:\n    pass\n\nfn main() -> Unit:\n    print(nothing())\n";
    let (ok, _out, err) = run(source);
    assert!(!ok);
    assert!(!err.contains("panicked"), "{err}");
}

/// Every standard-library module whose public API mentions `StdError` makes the type nameable
/// by itself, with no `import "std:types"`.
#[test]
fn std_modules_that_return_std_error_export_it() {
    // Driven by the directory, so a new module that mentions StdError cannot skip the re-export.
    let stdlib = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib");
    let mut modules = Vec::new();
    for entry in std::fs::read_dir(&stdlib).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        if text.contains("StdError") && !["http_types", "types"].contains(&name.as_str()) {
            modules.push(name);
        }
    }
    assert!(modules.len() >= 20, "{modules:?}");
    for module in modules {
        let source = format!(
            "import \"std:{module}\"\n\nfn code_of(problem: StdError) -> Text:\n    return problem.code\n\nfn main() -> Unit:\n    print(\"ok\")\n"
        );
        let (ok, out, err) = run(&source);
        assert!(ok, "std:{module}: {err}");
        assert_eq!(out, "ok\n", "std:{module}");
    }
}

#[test]
fn importing_std_error_through_several_routes_is_not_a_clash() {
    let source = "import \"std:crypto\"\nimport \"std:io\"\nimport \"std:types\"\nimport \"std:http\"\n\nfn code_of(problem: StdError) -> Text:\n    return problem.code\n\nfn main() -> Unit:\n    print(code_of(std_error(\"E_X\", \"m\")))\n";
    let (ok, out, err) = run(source);
    assert!(ok, "{err}");
    assert_eq!(out, "E_X\n");
}

#[test]
fn json_safe_accessors_report_wrong_types_without_stopping_the_program() {
    let source = r#"import "std:json"

fn describe(raw: Text) -> Text:
    let value = unwrap(json_parse(raw))
    match json_get_text_safe(value, "method"):
        Ok(method):
            return f"method {method}"
        Err(problem):
            return f"no method: {json_error_code(problem)}"

fn ints(raw: Text) -> Text:
    match json_int_list_safe(unwrap(json_parse(raw))):
        Ok(items):
            return f"ints {len(items)}"
        Err(problem):
            return f"not ints: {json_error_code(problem)}"

fn main() -> Unit:
    print(describe("{\"method\": \"ping\"}"))
    print(describe("{\"method\": 7}"))
    print(describe("{}"))
    print(describe("[1]"))
    print(describe("null"))
    print(ints("[1, 2, 3]"))
    print(ints("[]"))
    print(ints("[1, \"x\"]"))
    print(ints("{}"))
    print(ints("5"))
    print("still running")
"#;
    let (ok, out, err) = run(source);
    assert!(ok, "{err}");
    assert_eq!(
        out,
        "method ping\nno method: E_JSON_TYPE\nno method: E_JSON_MISSING\nno method: E_JSON_SHAPE\nno method: E_JSON_SHAPE\nints 3\nints 0\nnot ints: E_JSON_TYPE\nnot ints: E_JSON_SHAPE\nnot ints: E_JSON_SHAPE\nstill running\n"
    );
}
