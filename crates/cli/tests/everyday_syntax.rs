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

const LOOKUPS: &str = "enum Color:\n    Red\n    Green\n\nenum Shape:\n    Circle(radius: Int)\n    Dot\n\nfn look(n: Int) -> Result[Option[Int], Text]:\n    if n < 0:\n        return Err(\"negative\")\n    return Ok(half(n))\n\nfn half(n: Int) -> Option[Int]:\n    if n == 0:\n        return None\n    return Some(n // 2)\n\nfn color(n: Int) -> Option[Color]:\n    if n == 0:\n        return None\n    if n == 1:\n        return Some(Red)\n    return Some(Green)\n\nfn shape(n: Int) -> Result[Shape, Text]:\n    if n < 0:\n        return Err(\"no shape\")\n    if n == 0:\n        return Ok(Dot)\n    return Ok(Circle(n))\n\nfn nested(n: Int) -> Result[Result[Int, Text], Text]:\n    if n < 0:\n        return Err(\"outer\")\n    return Ok(inner(n))\n\nfn inner(n: Int) -> Result[Int, Text]:\n    if n == 0:\n        return Err(\"inner\")\n    return Ok(n)\n\n";

#[test]
fn nested_patterns_match_every_combination_once() {
    let source = format!(
        "{LOOKUPS}fn describe(n: Int) -> Text:\n    match look(n):\n        Err(e):\n            return \"error \" + e\n        Ok(None):\n            return \"none\"\n        Ok(Some(x)):\n            return \"some \" + int_to_text(x)\n\nfn paint(n: Int) -> Text:\n    match color(n):\n        Some(Red):\n            return \"red\"\n        None:\n            return \"no color\"\n        Some(Green):\n            return \"green\"\n\nfn area(n: Int) -> Int:\n    match shape(n):\n        Ok(Circle(r)):\n            return r * r\n        Ok(Dot):\n            return 0\n        Err(_):\n            return 0 - 1\n\nfn deep(n: Int) -> Text:\n    var out = \"\"\n    match nested(n):\n        Ok(Ok(_)):\n            set out = \"ok ok\"\n        Ok(Err(e)):\n            set out = \"ok \" + e\n        Err(e):\n            set out = e\n    return out\n\nfn main() -> Unit:\n    for n in [4, 0, 0 - 1]:\n        print(describe(n))\n        print(paint(n + 1))\n        print(area(n))\n        print(deep(n))\n    let probe = fn(n: Int) -> Text: describe(n)\n    print(probe(6))\n"
    );
    let (ok, out, err) = run(&source);
    assert!(ok, "{err}");
    assert_eq!(
        out,
        "some 2\ngreen\n16\nok ok\nnone\nred\n0\nok inner\nerror negative\nno color\n-1\nouter\nsome 3\n"
    );
}

#[test]
fn nested_patterns_reject_gaps_overlaps_and_mismatches() {
    let arms = |arms: &str| {
        format!("{LOOKUPS}fn main() -> Int:\n    match look(1):\n{arms}    return 0\n")
    };
    for (body, code, message) in [
        (
            "        Ok(Some(x)):\n            pass\n        Err(e):\n            pass\n",
            "E_MATCH_NONEXHAUSTIVE",
            "match does not handle `Ok(None)`",
        ),
        (
            "        Ok(Some(x)):\n            pass\n        Ok(None):\n            pass\n",
            "E_MATCH_NONEXHAUSTIVE",
            "match does not handle `Err(_)`",
        ),
        (
            "        Ok(Some(x)):\n            pass\n        Ok(None):\n            pass\n        Ok(None):\n            pass\n        Err(e):\n            pass\n",
            "E_MATCH_DUPLICATE",
            "two arms match `Ok(None)`",
        ),
        (
            "        Ok(Some(x)):\n            pass\n        Ok(y):\n            pass\n        Err(e):\n            pass\n",
            "E_MATCH_DUPLICATE",
            "`y` at `Ok(_)` overlaps",
        ),
        (
            "        Ok(Some(x)):\n            pass\n        Ok(Some(y)):\n            pass\n        Ok(None):\n            pass\n        Err(e):\n            pass\n",
            "E_MATCH_DUPLICATE",
            "two arms match the same values (`Ok(Some(_))`)",
        ),
        (
            "        Ok(Some(x)):\n            pass\n        Ok(Err(y)):\n            pass\n        Err(e):\n            pass\n",
            "E_MATCH_PATTERN",
            "`Err` and `Some` cannot match the same value",
        ),
        (
            "        Ok(Blue(x)):\n            pass\n        Ok(None):\n            pass\n        Err(e):\n            pass\n",
            "E_MATCH_PATTERN",
            "`Blue` is not a variant",
        ),
    ] {
        let (ok, _, err) = run(&arms(body));
        assert!(!ok, "{body}");
        assert!(err.contains(code), "{body}: {err}");
        assert!(err.contains(message), "{body}: {err}");
    }
    // An enum's variants hold scalars and lists, so a pattern inside one is refused.
    let (ok, _, err) = run(&format!(
        "{LOOKUPS}fn main() -> Int:\n    match shape(1):\n        Ok(Circle(Some(r))):\n            pass\n        Ok(Dot):\n            pass\n        Err(e):\n            pass\n    return 0\n"
    ));
    assert!(!ok);
    assert!(err.contains("E_MATCH_PATTERN"), "{err}");
    // A missing enum variant inside a constructor is reported as for a plain enum match.
    let (ok, _, err) = run(&format!(
        "{LOOKUPS}fn main() -> Int:\n    match color(1):\n        Some(Red):\n            pass\n        None:\n            pass\n    return 0\n"
    ));
    assert!(!ok);
    assert!(err.contains("E_MATCH_MISSING_VARIANT"), "{err}");
    // Against the wrong type the checker says what the arms expected and what the value is.
    let (ok, _, err) = run(&format!(
        "{LOOKUPS}fn main() -> Int:\n    match half(1):\n        Some(Some(x)):\n            pass\n        Some(None):\n            pass\n        None:\n            pass\n    return 0\n"
    ));
    assert!(!ok);
    assert!(err.contains("E_MATCH_TYPE"), "{err}");
    assert!(
        err.contains("the arms match an Option (`Some`, `None`), but this value is Int"),
        "{err}"
    );
    // A `None` arm that cannot sit beside `Ok` is reported at that arm, not at 0:0.
    let (ok, _, err) = run(&arms(
        "        Ok(Some(x)):\n            pass\n        None:\n            pass\n",
    ));
    assert!(!ok);
    let none_line = LOOKUPS.matches('\n').count() + 5;
    assert!(
        err.contains(&format!(
            "main.gu:{none_line}:9: error[E_MATCH_PATTERN]: `None` and `Ok` cannot match the same value"
        )),
        "{err}"
    );
}

#[test]
fn enum_variant_patterns_on_another_type_are_match_type_errors() {
    for (scrutinee, arms, found) in [
        // `Err` of `look` holds Text.
        (
            "look(1)",
            "        Ok(x):\n            pass\n        Err(Red):\n            pass\n",
            "Text",
        ),
        (
            "look(1)",
            "        Ok(Red):\n            pass\n        Ok(Green):\n            pass\n        Err(Red):\n            pass\n        Err(Green):\n            pass\n",
            "Option[Int]",
        ),
        // One level: a Text, and a value of another enum.
        (
            "\"red\"",
            "        Red:\n            pass\n        Green:\n            pass\n",
            "Text",
        ),
        (
            "Dot",
            "        Red:\n            pass\n        Green:\n            pass\n",
            "Shape",
        ),
    ] {
        let (ok, _, err) = run(&format!(
            "{LOOKUPS}fn main() -> Int:\n    match {scrutinee}:\n{arms}    return 0\n"
        ));
        assert!(!ok, "{arms}");
        assert!(
            err.contains(&format!(
                "error[E_MATCH_TYPE]: `Red` is a `Color` variant, but this value is {found}"
            )),
            "{arms}: {err}"
        );
    }
}

#[test]
fn match_temporaries_cannot_be_named() {
    let body = |name: &str| {
        format!(
            "{LOOKUPS}fn main() -> Int:\n    match look(1):\n        Ok(Some(x)):\n            print({name})\n        Ok(None):\n            pass\n        Err(e):\n            pass\n    match color(1):\n        Some(Red):\n            print({name})\n        Some(Green):\n            pass\n        None:\n            pass\n    return 0\n"
        )
    };
    for name in [
        "__match_part_1",
        "__match_part_2",
        "__enum_match_1",
        "__enum_match_2",
    ] {
        let (ok, _, err) = run(&body(name));
        assert!(!ok, "{name}");
        assert!(
            err.contains(&format!("error[E_NAME_UNKNOWN]: unknown name `{name}`")),
            "{name}: {err}"
        );
    }
    // The temporaries' real names start with `%`, which no name in source can.
    let (ok, _, err) = run(&body("%match_part_1"));
    assert!(!ok);
    assert!(err.contains("E_SYNTAX"), "{err}");
}

#[test]
fn option_and_result_constructors_build_wherever_their_type_is_known() {
    let (ok, out, err) = run(
        "record Box:\n    value: Option[Int]\n\nfn lookup(flag: Int) -> Result[Option[Int], Text]:\n    if flag == 0:\n        return Ok(None)\n    if flag == 1:\n        return Ok(Some(7))\n    return Err(\"bad\")\n\nfn wrapped() -> Option[Result[Int, Text]]:\n    return Some(Ok(1))\n\nfn failed() -> Option[Result[Int, Text]]:\n    return Some(Err(\"no\"))\n\nfn show(value: Option[Text]) -> Text:\n    match value:\n        Some(text):\n            return text\n        None:\n            return \"none\"\n\nfn twice[T](value: T) -> Option[Option[T]]:\n    return Some(Some(value))\n\nfn main() -> Unit:\n    print(lookup(0))\n    print(lookup(1))\n    print(lookup(2))\n    print(wrapped())\n    print(failed())\n    let boxed = Box(Some(1))\n    print(boxed.value)\n    let nothing: Option[Int] = None\n    print(nothing)\n    let inferred = Some(\"hi\")\n    print(show(inferred))\n    print(show(None))\n    let held: List[Option[Int]] = [Some(1), None]\n    print(held)\n    var later: Option[Int] = None\n    set later = Some(3)\n    print(later)\n    let nested: Result[Option[Int], Text] = Ok(Some(5))\n    print(nested)\n    let pair: Map[Text, Result[Int, Text]] = {\"a\": Ok(1), \"b\": Err(\"x\")}\n    print(pair)\n    print(twice(4))\n    let lift = fn(x: Int) -> Result[Option[Int], Text]: Ok(Some(x))\n    print(lift(8))\n",
    );
    assert!(ok, "{err}");
    assert_eq!(
        out,
        "Ok(None)\nOk(Some(7))\nErr(bad)\nSome(Ok(1))\nSome(Err(no))\nSome(1)\nNone\nhi\nnone\n[Some(1), None]\nSome(3)\nOk(Some(5))\n{\"a\": Ok(1), \"b\": Err(x)}\nSome(Some(4))\nOk(Some(8))\n"
    );
    // Where nothing says which type is meant, the error says how to write it.
    for (source, code, message) in [
        (
            "fn main() -> Int:\n    let value = None\n    return 0\n",
            "E_OPTION_CONSTRUCTOR_CONTEXT",
            "cannot tell which Option type `None` is here; write the type where it is bound, for example `let value: Option[Int] = None`",
        ),
        (
            "fn main() -> Int:\n    let value = Ok(1)\n    return 0\n",
            "E_RESULT_CONSTRUCTOR_CONTEXT",
            "cannot tell which Result type `Ok` makes here",
        ),
        (
            "fn main() -> Int:\n    return None\n",
            "E_OPTION_CONSTRUCTOR_CONTEXT",
            "`None` is an Option, but this position expects Int",
        ),
        (
            "fn main() -> Int:\n    return Some(1)\n",
            "E_OPTION_CONSTRUCTOR_CONTEXT",
            "`Some` makes an Option, but this position expects Int",
        ),
        (
            "fn main() -> Int:\n    let value: Text = Err(\"x\")\n    return 0\n",
            "E_RESULT_CONSTRUCTOR_CONTEXT",
            "`Err` makes a Result, but this position expects Text",
        ),
        (
            "fn main() -> Int:\n    let value: Result[Option[Int], Text] = Ok(Some(\"x\"))\n    return 0\n",
            "E_OPTION_CONSTRUCTOR_TYPE",
            "Some value must have type",
        ),
    ] {
        let (ok, _, err) = run(source);
        assert!(!ok, "{source}");
        assert!(err.contains(code) && err.contains(message), "{source}: {err}");
    }
}

#[test]
fn bytes_index_gives_an_int_byte_and_checks_bounds() {
    let (ok, out, err) = run(
        "fn main() -> Unit:\n    let data = bytes_from_ints([0, 7, 255])\n    var total = 0\n    var i = 0\n    while i < len(data):\n        set total = total + data[i]\n        set i = i + 1\n    print(total)\n    print(data[2] == 255)\n",
    );
    assert!(ok, "{err}");
    assert_eq!(out, "262\ntrue\n");
    for (index, code, message) in [
        (
            "3",
            "R_INDEX_BOUNDS",
            "bytes index 3 is out of range for length 3",
        ),
        (
            "0 - 1",
            "R_INDEX_INVALID",
            "bytes index must be a non-negative",
        ),
    ] {
        let (ok, _, err) = run(&format!(
            "fn main() -> Int:\n    let data = bytes_from_ints([0, 7, 255])\n    return data[{index}]\n"
        ));
        assert!(!ok, "{index}");
        assert!(
            err.contains(code) && err.contains(message),
            "{index}: {err}"
        );
    }
    let (ok, _, err) =
        run("fn main() -> Int:\n    let data = bytes_from_ints([1])\n    return data[\"0\"]\n");
    assert!(!ok);
    assert!(
        err.contains("E_TYPE_INDEX") && err.contains("bytes index must be Int"),
        "{err}"
    );
}
