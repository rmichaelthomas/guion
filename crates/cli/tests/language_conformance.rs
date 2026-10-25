use serde_json::Value;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use veritas_compiler::{compile_project, Diagnostic};
use veritas_model::{Artifact, BigInt, RuntimeError, SharedJson, Value as RuntimeValue};
use veritas_runtime::{EffectRecord, HostPolicy, Instance, Runtime};

struct TempProject(PathBuf);

impl TempProject {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock is after Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "veritas-language-conformance-{}-{label}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temporary project");
        Self(path)
    }

    fn write(&self, path: &str, source: &str) {
        let target = self.0.join(path);
        fs::create_dir_all(target.parent().expect("source has a parent"))
            .expect("create source parent");
        fs::write(target, source).expect("write source");
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone)]
struct Case {
    source: String,
    accepts: bool,
}

fn main_return(expression: &str) -> String {
    format!("fn main() -> Int:\n    return {expression}\n")
}

fn source_case(id: &str, class: &str) -> Option<Case> {
    let accepts = class != "negative";
    let source = if id.starts_with("operator-") {
        operator_case(id, class)?
    } else if id.starts_with("type-") {
        type_case(id, class)?
    } else if id.starts_with("host_operation-") {
        host_case(id, class)?
    } else {
        syntax_case(id, class)?.to_string()
    };
    Some(Case { source, accepts })
}

fn syntax_case(id: &str, class: &str) -> Option<&'static str> {
    Some(match (id, class) {
        ("syntax-utf-8-source-newlines-indentation-and-comments", "positive") => {
            "# café\nfn main() -> Int:\n    return len(\"é\")\n"
        }
        ("syntax-utf-8-source-newlines-indentation-and-comments", "negative") => {
            r#"fn main() -> Int:
    return len("\u{D800}")
"#
        }
        ("syntax-utf-8-source-newlines-indentation-and-comments", "interaction") => {
            "# UTF-8 comment\nfn label() -> Text:\n    return \"veritas-é\"\n\nfn main() -> Int:\n    return len(label())\n"
        }
        ("syntax-function-declarations-with-typed-parameters-and-result", "positive") => {
            "fn add(left: Int, right: Int) -> Int:\n    return left + right\n\nfn main() -> Int:\n    return add(20, 22)\n"
        }
        ("syntax-function-declarations-with-typed-parameters-and-result", "negative") => {
            "fn add(value: Int, value: Int) -> Int:\n    return value\n\nfn main() -> Int:\n    return add(1, 2)\n"
        }
        ("syntax-function-declarations-with-typed-parameters-and-result", "interaction") => {
            "fn touch(value: Int) -> Unit:\n    print(value)\n\nfn answer(value: Int) -> Int:\n    return value + 1\n\nfn main() -> Int:\n    touch(41)\n    return answer(41)\n"
        }
        ("syntax-record-declarations-and-fields", "positive") => {
            "record Point:\n    x: Int\n    y: Int\n\nfn main() -> Int:\n    let point = Point(20, 22)\n    return point.x + point.y\n"
        }
        ("syntax-record-declarations-and-fields", "negative") => {
            "record Point:\n    x: Int\n    x: Int\n\nfn main() -> Int:\n    return 0\n"
        }
        ("syntax-record-declarations-and-fields", "interaction") => {
            "record Point:\n    x: Int\n    y: Int\n\nfn sum(point: Point) -> Int:\n    return point.x + point.y\n\nfn main() -> Int:\n    return sum(Point(20, 22))\n"
        }
        ("syntax-relative-module-imports-and-project-root-confinement", "positive") => {
            "import \"lib.vr\"\n\nfn main() -> Int:\n    return answer()\n"
        }
        ("syntax-relative-module-imports-and-project-root-confinement", "negative") => {
            "import \"../outside.vr\"\n\nfn main() -> Int:\n    return 0\n"
        }
        ("syntax-relative-module-imports-and-project-root-confinement", "interaction") => {
            "import \"first.vr\"\nimport \"second.vr\"\n\nfn main() -> Int:\n    return first() + second()\n"
        }
        ("syntax-public-and-private-declarations", "positive") => {
            "fn helper() -> Int:\n    return 42\n\nfn main() -> Int:\n    return helper()\n"
        }
        ("syntax-public-and-private-declarations", "negative") => {
            "import \"lib.vr\"\n\nfn main() -> Int:\n    return hidden()\n"
        }
        ("syntax-public-and-private-declarations", "interaction") => {
            "import \"first.vr\"\nimport \"second.vr\"\n\nfn main() -> Int:\n    return first() + second()\n"
        }
        ("syntax-entry-point-signatures", "positive") => {
            "fn main() -> Int:\n    return 42\n"
        }
        ("syntax-entry-point-signatures", "negative") => {
            "fn helper() -> Int:\n    return 42\n"
        }
        ("syntax-entry-point-signatures", "interaction") => {
            "fn helper() -> Int:\n    return 42\n\nfn main() -> Unit:\n    print(helper())\n"
        }
        ("syntax-local-inference-and-explicit-annotations", "positive") => {
            "fn main() -> Int:\n    let answer = 42\n    let label: Text = \"answer\"\n    return answer\n"
        }
        ("syntax-local-inference-and-explicit-annotations", "negative") => {
            "fn main() -> Int:\n    let answer: Int = \"forty two\"\n    return answer\n"
        }
        ("syntax-local-inference-and-explicit-annotations", "interaction") => {
            "fn main() -> Int:\n    let values: List[Int] = []\n    var total = len(values)\n    set total = total + 42\n    return total\n"
        }
        ("syntax-let-immutable-bindings", "positive") => {
            "fn main() -> Int:\n    let answer = 42\n    return answer\n"
        }
        ("syntax-let-immutable-bindings", "negative") => {
            "fn main() -> Int:\n    let answer = 41\n    set answer = 42\n    return answer\n"
        }
        ("syntax-let-immutable-bindings", "interaction") => {
            "fn main() -> Int:\n    let first = 40\n    var second = first\n    set second = second + 2\n    return second\n"
        }
        ("syntax-var-mutable-bindings-and-set-assignment", "positive") => {
            "fn main() -> Int:\n    var answer = 40\n    set answer = answer + 2\n    return answer\n"
        }
        ("syntax-var-mutable-bindings-and-set-assignment", "negative") => {
            "fn main() -> Int:\n    set missing = 42\n    return 0\n"
        }
        ("syntax-var-mutable-bindings-and-set-assignment", "interaction") => {
            "fn increment(value: Int) -> Int:\n    return value + 1\n\nfn main() -> Int:\n    var answer = 41\n    set answer = increment(answer)\n    return answer\n"
        }
        ("syntax-return-statements-and-call-statements", "positive") => {
            "fn emit() -> Unit:\n    print(42)\n\nfn main() -> Int:\n    emit()\n    return 0\n"
        }
        ("syntax-return-statements-and-call-statements", "negative") => {
            "fn missing() -> Int:\n    let value = 42\n\nfn main() -> Int:\n    return 0\n"
        }
        ("syntax-return-statements-and-call-statements", "interaction") => {
            "fn value() -> Int:\n    return 42\n\nfn emit(number: Int) -> Unit:\n    print(number)\n\nfn main() -> Int:\n    emit(value())\n    return value()\n"
        }
        ("syntax-if-and-else-statements", "positive") => {
            "fn main() -> Int:\n    if true:\n        return 42\n    else:\n        return 0\n"
        }
        ("syntax-if-and-else-statements", "negative") => {
            "fn main() -> Int:\n    if 1:\n        return 42\n    else:\n        return 0\n"
        }
        ("syntax-if-and-else-statements", "interaction") => {
            "fn main() -> Int:\n    let value = 40\n    if value < 41 and true:\n        return value + 2\n    else:\n        return 0\n"
        }
        ("syntax-while-loops", "positive") => {
            "fn main() -> Int:\n    var value = 0\n    while value < 42:\n        set value = value + 1\n    return value\n"
        }
        ("syntax-while-loops", "negative") => {
            "fn main() -> Int:\n    while 1:\n        return 42\n    return 0\n"
        }
        ("syntax-while-loops", "interaction") => {
            "fn main() -> Int:\n    var value = 0\n    while value < 2:\n        if value == 1:\n            set value = value + 41\n        else:\n            set value = value + 1\n    return value\n"
        }
        ("syntax-for-loops", "positive") => {
            "fn main() -> Int:\n    var total = 0\n    for value in [20, 22]:\n        set total = total + value\n    return total\n"
        }
        ("syntax-for-loops", "negative") => {
            "fn main() -> Int:\n    for value in 42:\n        return value\n    return 0\n"
        }
        ("syntax-for-loops", "interaction") => {
            "fn main() -> Int:\n    let values: Map[Text, Int] = {\"answer\": 42}\n    var total = 0\n    for key in values:\n        set total = total + values[key]\n    return total\n"
        }
        ("syntax-exhaustive-option-match-statements", "positive") => {
            "fn read() -> Option[Int]:\n    return Some(42)\n\nfn main() -> Int:\n    match read():\n        Some(value):\n            return value\n        None:\n            return 0\n"
        }
        ("syntax-exhaustive-option-match-statements", "negative") => {
            "fn read() -> Option[Int]:\n    return Some(42)\n\nfn main() -> Int:\n    match read():\n        Some(value):\n            return value\n"
        }
        ("syntax-exhaustive-option-match-statements", "interaction") => {
            "fn read() -> Option[List[Int]]:\n    return Some([])\n\nfn main() -> Int:\n    match read():\n        Some(values):\n            return len(values)\n        None:\n            return 0\n"
        }
        ("syntax-exhaustive-result-match-statements", "positive") => {
            "fn read() -> Result[Int, Text]:\n    return Ok(42)\n\nfn main() -> Int:\n    match read():\n        Ok(value):\n            return value\n        Err(message):\n            return len(message)\n"
        }
        ("syntax-exhaustive-result-match-statements", "negative") => {
            "fn read() -> Result[Int, Text]:\n    return Ok(42)\n\nfn main() -> Int:\n    match read():\n        Ok(value):\n            return value\n"
        }
        ("syntax-exhaustive-result-match-statements", "interaction") => {
            "fn read() -> Result[List[Int], Text]:\n    return Ok([])\n\nfn use() -> Result[Int, Text]:\n    let values = read()?\n    return Ok(len(values) + 42)\n\nfn main() -> Int:\n    match use():\n        Ok(value):\n            return value\n        Err(message):\n            return len(message)\n"
        }
        ("syntax-integer-float64-bool-and-text-literals", "positive") => {
            "fn main() -> Int:\n    let ratio: Float64 = 1.5\n    if true and ratio > 1.0:\n        return len(\"é\") + 41\n    else:\n        return 0\n"
        }
        ("syntax-integer-float64-bool-and-text-literals", "negative") => {
            r#"fn main() -> Int:
    return len("\u{110000}")
"#
        }
        ("syntax-integer-float64-bool-and-text-literals", "interaction") => {
            "fn main() -> Int:\n    let value = 21\n    let doubled: Float64 = 2.0\n    if doubled == 2.0:\n        return value * 2\n    else:\n        return 0\n"
        }
        ("syntax-function-calls-and-record-construction", "positive") => {
            "record Pair:\n    left: Int\n    right: Int\n\nfn sum(pair: Pair) -> Int:\n    return pair.left + pair.right\n\nfn main() -> Int:\n    return sum(Pair(20, 22))\n"
        }
        ("syntax-function-calls-and-record-construction", "negative") => {
            "fn add(left: Int, right: Int) -> Int:\n    return left + right\n\nfn main() -> Int:\n    return add(42)\n"
        }
        ("syntax-function-calls-and-record-construction", "interaction") => {
            "record Point:\n    x: Int\n\nfn shift(point: Point, amount: Int) -> Point:\n    return Point(point.x + amount)\n\nfn main() -> Int:\n    return shift(Point(40), 2).x\n"
        }
        ("syntax-list-literals-and-contextual-empty-lists", "positive") => {
            "fn main() -> Int:\n    let values: List[Int] = []\n    return len(values)\n"
        }
        ("syntax-list-literals-and-contextual-empty-lists", "negative") => {
            "fn main() -> Int:\n    let values = []\n    return 0\n"
        }
        ("syntax-list-literals-and-contextual-empty-lists", "interaction") => {
            "fn first(values: List[Int]) -> Int:\n    return values[0]\n\nfn main() -> Int:\n    return first([20, 22])\n"
        }
        ("syntax-map-literals-text-keys-and-contextual-empty-maps", "positive") => {
            "fn main() -> Int:\n    let values: Map[Text, Int] = {}\n    return len(values)\n"
        }
        ("syntax-map-literals-text-keys-and-contextual-empty-maps", "negative") => {
            "fn main() -> Int:\n    let values: Map[Text, Int] = {\"answer\": 41, \"answer\": 42}\n    return len(values)\n"
        }
        ("syntax-map-literals-text-keys-and-contextual-empty-maps", "interaction") => {
            "fn main() -> Int:\n    let values = map_set({\"answer\": 40}, \"other\", 2)\n    return values[\"answer\"] + values[\"other\"]\n"
        }
        ("syntax-list-map-and-text-indexing", "positive") => {
            "fn main() -> Int:\n    let values = [42]\n    let mapping = {\"answer\": 42}\n    return values[0] + mapping[\"answer\"] + len(\"x\")\n"
        }
        ("syntax-list-map-and-text-indexing", "negative") => {
            "fn main() -> Int:\n    return 1[0]\n"
        }
        ("syntax-list-map-and-text-indexing", "interaction") => {
            "fn select(values: List[Map[Text, Int]]) -> Int:\n    return values[0][\"answer\"]\n\nfn main() -> Int:\n    return select([{\"answer\": 42}])\n"
        }
        ("syntax-record-field-access", "positive") => {
            "record Point:\n    x: Int\n\nfn main() -> Int:\n    let point = Point(42)\n    return point.x\n"
        }
        ("syntax-record-field-access", "negative") => {
            "record Point:\n    x: Int\n\nfn main() -> Int:\n    let point = Point(42)\n    return point.y\n"
        }
        ("syntax-record-field-access", "interaction") => {
            "record Point:\n    x: Int\n\nfn read(point: Point) -> Int:\n    return point.x\n\nfn main() -> Int:\n    return read(Point(42))\n"
        }
        ("syntax-parenthesized-expressions-and-precedence", "positive") => {
            "fn main() -> Int:\n    return (2 + 3) * 7\n"
        }
        ("syntax-parenthesized-expressions-and-precedence", "negative") => {
            "fn main() -> Int:\n    return (42\n"
        }
        ("syntax-parenthesized-expressions-and-precedence", "interaction") => {
            "fn add(left: Int, right: Int) -> Int:\n    return left + right\n\nfn main() -> Int:\n    return add((20 + 1), (20 + 1))\n"
        }
        ("syntax-postfix-result-propagation-with-question-mark", "positive") => {
            "fn read() -> Result[Int, Text]:\n    return Ok(42)\n\nfn use() -> Result[Int, Text]:\n    return Ok(read()?)\n\nfn main() -> Int:\n    return unwrap(use())\n"
        }
        ("syntax-postfix-result-propagation-with-question-mark", "negative") => {
            "fn main() -> Int:\n    return 42?\n"
        }
        ("syntax-postfix-result-propagation-with-question-mark", "interaction") => {
            "fn read() -> Result[List[Int], Text]:\n    return Ok([])\n\nfn use() -> Result[Int, Text]:\n    let values = read()?\n    return Ok(len(values) + 42)\n\nfn main() -> Int:\n    return unwrap(use())\n"
        }
        ("syntax-import-graph-size-depth-and-total-source-limits", "positive") => {
            "import \"lib.vr\"\n\nfn main() -> Int:\n    return answer()\n"
        }
        ("syntax-import-graph-size-depth-and-total-source-limits", "negative") => {
            "import \"../outside.vr\"\n\nfn main() -> Int:\n    return 0\n"
        }
        ("syntax-import-graph-size-depth-and-total-source-limits", "interaction") => {
            "import \"first.vr\"\nimport \"second.vr\"\n\nfn main() -> Int:\n    return first() + second()\n"
        }
        _ => return None,
    })
}

fn operator_case(id: &str, class: &str) -> Option<String> {
    let name = id.strip_prefix("operator-")?;
    if name == "postfix-propagate" {
        return Some(match class {
            "positive" => "fn read() -> Result[Int, Text]:\n    return Ok(42)\n\nfn main() -> Int:\n    return unwrap(read())\n".into(),
            "negative" => "fn main() -> Int:\n    return 42?\n".into(),
            _ => "fn read() -> Result[Int, Text]:\n    return Ok(42)\n\nfn use() -> Result[Int, Text]:\n    return Ok(read()?)\n\nfn main() -> Int:\n    return unwrap(use())\n".into(),
        });
    }
    let expression = match name {
        "add" => match class {
            "positive" => "20 + 22",
            "negative" => "1 + \"two\"",
            _ => "len(\"veritas\") + (20 + 15)",
        },
        "subtract" => match class {
            "positive" => "50 - 8",
            "negative" => "1 - true",
            _ => "(50 - 10) - 8",
        },
        "multiply" => match class {
            "positive" => "6 * 7",
            "negative" => "6 * \"7\"",
            _ => "(3 * 7) * 2",
        },
        "divide" => match class {
            "positive" => "84 / 2",
            "negative" => "84 / false",
            _ => "(84 / 2) + 0",
        },
        "floor-divide" => match class {
            "positive" => "85 // 2",
            "negative" => "85 // \"2\"",
            _ => "(85 // 2) + 0",
        },
        "remainder" => match class {
            "positive" => "86 % 2",
            "negative" => "86 % false",
            _ => "(86 % 2) + 43",
        },
        "equal" => match class {
            "positive" => "42 == 42",
            "negative" => "1 == \"one\"",
            _ => "(20 + 22) == 42",
        },
        "not-equal" => match class {
            "positive" => "41 != 42",
            "negative" => "1 != \"one\"",
            _ => "(20 + 22) != 41",
        },
        "less-than" => match class {
            "positive" => "41 < 42",
            "negative" => "1 < \"one\"",
            _ => "20 < (21 + 21)",
        },
        "less-or-equal" => match class {
            "positive" => "42 <= 42",
            "negative" => "1 <= \"one\"",
            _ => "(20 + 22) <= 42",
        },
        "greater-than" => match class {
            "positive" => "43 > 42",
            "negative" => "1 > \"one\"",
            _ => "(22 + 21) > 42",
        },
        "greater-or-equal" => match class {
            "positive" => "42 >= 42",
            "negative" => "1 >= \"one\"",
            _ => "(20 + 22) >= 42",
        },
        "and" => match class {
            "positive" => "true and true",
            "negative" => "1 and true",
            _ => "(20 + 22 == 42) and true",
        },
        "or" => match class {
            "positive" => "false or true",
            "negative" => "1 or false",
            _ => "false or (20 + 22 == 42)",
        },
        "unary-minus" => match class {
            "positive" => "-42",
            "negative" => "-\"42\"",
            _ => "-(-42)",
        },
        "not" => match class {
            "positive" => "not false",
            "negative" => "not 1",
            _ => "not (20 != 20)",
        },
        _ => return None,
    };
    if matches!(
        name,
        "equal"
            | "not-equal"
            | "less-than"
            | "less-or-equal"
            | "greater-than"
            | "greater-or-equal"
            | "and"
            | "or"
            | "not"
    ) && class != "negative"
    {
        Some(format!("fn main() -> Int:\n    if {expression}:\n        return 42\n    else:\n        return 0\n"))
    } else {
        Some(main_return(expression))
    }
}

fn type_case(id: &str, class: &str) -> Option<String> {
    let name = id.strip_prefix("type-")?;
    if class == "negative" {
        let (declaration, value_type, wrong) = match name {
            "int" => ("", "Int", "\"bad\""),
            "float64" => ("", "Float64", "42"),
            "record" => ("record Point:\n    x: Int\n\n", "Point", "\"bad\""),
            "bytes" => ("", "Bytes", "\"bytes\""),
            "bool" => ("", "Bool", "1"),
            "text" => ("", "Text", "42"),
            "list" => ("", "List[Int]", "[\"bad\"]"),
            "map" => ("", "Map[Text, Int]", "{\"key\": \"bad\"}"),
            "option" => ("", "Option[Int]", "Some(\"bad\")"),
            "result" => ("", "Result[Int, Text]", "Ok(\"bad\")"),
            "taskerror" => ("", "TaskError", "1"),
            "json" => ("", "Json", "1"),
            "jsonerror" => ("", "JsonError", "1"),
            "fileerror" => ("", "FileError", "1"),
            "httperror" => ("", "HttpError", "1"),
            "unit" => ("", "Unit", "1"),
            _ => return None,
        };
        return Some(format!("{declaration}fn take(value: {value_type}) -> Int:\n    return 0\n\nfn main() -> Int:\n    return take({wrong})\n"));
    }
    let (declaration, value_type, expression, final_body) = match name {
        "int" => ("", "Int", "42", "return value"),
        "float64" => (
            "",
            "Float64",
            "42.0",
            "if value == 42.0:\n    return 42\nelse:\n    return 0",
        ),
        "record" => (
            "record Point:\n    x: Int\n\n",
            "Point",
            "Point(42)",
            "return value.x",
        ),
        "bytes" => (
            "",
            "Bytes",
            "unwrap(read_bytes(\"missing\"))",
            "return len(value)",
        ),
        "bool" => (
            "",
            "Bool",
            "true",
            "if value:\n    return 42\nelse:\n    return 0",
        ),
        "text" => ("", "Text", "\"answer\"", "return len(value) + 36"),
        "list" => ("", "List[Int]", "[42]", "return value[0]"),
        "map" => (
            "",
            "Map[Text, Int]",
            "{\"answer\": 42}",
            "return value[\"answer\"]",
        ),
        "option" => (
            "fn create_option() -> Option[Int]:\n    return Some(42)\n\n",
            "Option[Int]",
            "create_option()",
            "return unwrap(value)",
        ),
        "result" => (
            "fn create_result() -> Result[Int, Text]:\n    return Ok(42)\n\n",
            "Result[Int, Text]",
            "create_result()",
            "return unwrap(value)",
        ),
        "taskerror" => (
            "fn worker(value: Int) -> Int:\n    return value\n\n",
            "TaskError",
            "error(parallel_map([1], worker, 1))",
            "return task_index(value)",
        ),
        "json" => (
            "",
            "Json",
            "unwrap(json_parse(\"42\"))",
            "return json_as_int(value)",
        ),
        "jsonerror" => (
            "",
            "JsonError",
            "error(json_parse(\"invalid\"))",
            "return json_error_line(value)",
        ),
        "fileerror" => (
            "",
            "FileError",
            "error(read_text(\"missing\"))",
            "return file_error_line(value)",
        ),
        "httperror" => (
            "",
            "HttpError",
            "error(http_get(\"https://example.invalid\"))",
            "return len(http_error_code(value))",
        ),
        "unit" => (
            "fn unit_value() -> Unit:\n    print(0)\n\n",
            "Unit",
            "unit_value()",
            "return 42",
        ),
        _ => return None,
    };
    let mut source = String::from(declaration);
    if class == "interaction" && name != "unit" {
        source.push_str(&format!(
            "fn identity(value: {value_type}) -> {value_type}:\n    return value\n\n"
        ));
    }
    source.push_str("fn main() -> Int:\n");
    if name == "unit" {
        source.push_str(&format!("    {expression}\n    {final_body}\n"));
    } else {
        source.push_str(&format!("    let value: {value_type} = {expression}\n"));
        let result_name = if class == "interaction" {
            source.push_str("    let copied = identity(value)\n");
            "copied"
        } else {
            "value"
        };
        let final_body = final_body.replace("value", result_name);
        for line in final_body.lines() {
            source.push_str("    ");
            source.push_str(line);
            source.push('\n');
        }
    }
    Some(source)
}

fn host_case(id: &str, class: &str) -> Option<String> {
    let source = match (id, class) {
        ("host_operation-output", "positive") => "fn main() -> Int:\n    print(42)\n    return 0\n",
        ("host_operation-output", "negative") => "fn main() -> Int:\n    print()\n    return 0\n",
        ("host_operation-output", "interaction") => "fn emit(value: Int) -> Unit:\n    print(value)\n\nfn main() -> Int:\n    emit(42)\n    return 0\n",
        ("host_operation-arguments", "positive") => "fn main() -> Int:\n    return len(args())\n",
        ("host_operation-arguments", "negative") => "fn main() -> Int:\n    return len(args(1))\n",
        ("host_operation-arguments", "interaction") => "fn main() -> Int:\n    let values = args()\n    return len(values)\n",
        ("host_operation-argument-suffix", "positive") => "fn main() -> Int:\n    return len(args_from(0))\n",
        ("host_operation-argument-suffix", "negative") => "fn main() -> Int:\n    return len(args_from(\"zero\"))\n",
        ("host_operation-argument-suffix", "interaction") => "fn main() -> Int:\n    let values = args_from(0)\n    return len(values)\n",
        ("host_operation-environment", "positive") => "fn main() -> Int:\n    let value = env(\"PATH\")\n    return 0\n",
        ("host_operation-environment", "negative") => "fn main() -> Int:\n    let value = env(1)\n    return 0\n",
        ("host_operation-environment", "interaction") => "fn main() -> Int:\n    match env(\"PATH\"):\n        Some(value):\n            return len(value)\n        None:\n            return 0\n",
        ("host_operation-read-text", "positive") => "fn main() -> Int:\n    let value = read_text(\"input.txt\")\n    if is_ok(value):\n        return 42\n    else:\n        return 0\n",
        ("host_operation-read-text", "negative") => "fn main() -> Int:\n    let value = read_text(1)\n    return is_ok(value)\n",
        ("host_operation-read-text", "interaction") => "fn main() -> Int:\n    match read_text(\"input.txt\"):\n        Ok(value):\n            return len(value)\n        Err(problem):\n            return len(file_error_message(problem))\n",
        ("host_operation-write-text", "positive") => "fn main() -> Int:\n    let result = write_text(\"output.txt\", \"answer\")\n    if is_ok(result):\n        return 42\n    else:\n        return 0\n",
        ("host_operation-write-text", "negative") => "fn main() -> Int:\n    return is_ok(write_text(\"output.txt\", 42))\n",
        ("host_operation-write-text", "interaction") => "fn main() -> Int:\n    let result = write_text(\"output.txt\", \"answer\")\n    if is_ok(result):\n        return 42\n    else:\n        return 0\n",
        ("host_operation-atomic-text-replace", "positive") => "fn main() -> Int:\n    let result = write_text_atomic(\"output.txt\", \"answer\")\n    if is_ok(result):\n        return 42\n    else:\n        return 0\n",
        ("host_operation-atomic-text-replace", "negative") => "fn main() -> Int:\n    return is_ok(write_text_atomic(\"output.txt\", 42))\n",
        ("host_operation-atomic-text-replace", "interaction") => "fn main() -> Int:\n    let result = write_text_atomic(\"output.txt\", \"answer\")\n    if is_ok(result):\n        return 42\n    else:\n        return 0\n",
        ("host_operation-clock", "positive") => "record Date:\n    year: Int\n    month: Int\n    day: Int\n\nfn main() -> Int:\n    let timestamp = unix_time_ms()\n    let current_date = time_today_local()\n    if current_date.month >= 1 and current_date.month <= 12:\n        return timestamp - timestamp\n    else:\n        return 1\n",
        ("host_operation-clock", "negative") => "fn main() -> Int:\n    let timestamp = unix_time_ms(1)\n    return timestamp\n",
        ("host_operation-clock", "interaction") => "record Date:\n    year: Int\n    month: Int\n    day: Int\n\nfn main() -> Int:\n    let first = unix_time_ms()\n    let current_date = time_today_local()\n    let second = unix_time_ms()\n    if second >= first and current_date.day >= 1 and current_date.day <= 31:\n        return 42\n    else:\n        return 0\n",
        _ => return None,
    };
    Some(source.into())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DiagnosticLocation {
    code: String,
    source: String,
    line: usize,
    column: usize,
}

fn bootstrap_location(error: &Diagnostic) -> DiagnosticLocation {
    DiagnosticLocation {
        code: error.code.clone(),
        source: error.source_name.clone().unwrap_or_default(),
        line: error.span.line,
        column: error.span.column,
    }
}

fn self_host_location(diagnostic: &Value) -> Option<DiagnosticLocation> {
    Some(DiagnosticLocation {
        code: diagnostic.get("code")?.as_str()?.to_string(),
        source: diagnostic.get("source_name")?.as_str()?.to_string(),
        line: diagnostic.get("span")?.get("line")?.as_u64()? as usize,
        column: diagnostic.get("span")?.get("column")?.as_u64()? as usize,
    })
}

#[derive(Debug, PartialEq)]
struct RuntimeObservation {
    result: Result<RuntimeValue, (String, String)>,
    stdout: String,
    stderr: String,
    effects: Vec<EffectRecord>,
}

fn observe(artifact: Artifact, file_root: &Path) -> Result<RuntimeObservation, String> {
    let mut instance = Runtime::vm_only()
        .load(
            artifact,
            HostPolicy::default()
                .with_file_access(file_root, true, true)
                .with_clock_access(),
        )
        .map_err(|error| {
            format!(
                "artifact verification failed: {}: {}",
                error.code, error.message
            )
        })?;
    let result = instance
        .invoke("main", Vec::new())
        .map_err(|RuntimeError { code, message, .. }| (code, message));
    Ok(RuntimeObservation {
        result,
        stdout: instance.take_output(),
        stderr: instance.take_program_stderr(),
        effects: instance.effect_report().to_vec(),
    })
}

fn compile_both(
    binary: &Path,
    compiler_artifact: &Path,
    case_project: &TempProject,
    source: &str,
    case_id: &str,
) -> (
    Option<Artifact>,
    Option<Artifact>,
    Option<DiagnosticLocation>,
    Option<DiagnosticLocation>,
) {
    case_project.write("main.vr", source);
    let bootstrap = compile_project(&case_project.0.join("main.vr"), &case_project.0);
    let self_host = Command::new(binary)
        .args(["run", "--json", "--file-root"])
        .arg(&case_project.0)
        .arg(compiler_artifact)
        .arg("main.vr")
        .current_dir(&case_project.0)
        .output()
        .unwrap_or_else(|error| panic!("{case_id}: launch self-host compiler: {error}"));
    assert!(
        self_host.status.success(),
        "{case_id}: {}",
        String::from_utf8_lossy(&self_host.stderr)
    );
    let report: Value = serde_json::from_slice(&self_host.stdout).unwrap_or_else(|error| {
        panic!(
            "{case_id}: invalid self-host run report: {error}: {}",
            String::from_utf8_lossy(&self_host.stdout)
        )
    });
    assert_eq!(report["exit_code"], 0, "{case_id}: {report}");
    assert!(
        report["diagnostic"].is_null(),
        "{case_id}: runtime failed while compiling: {report}"
    );
    let program_stdout = report["program_stdout"]
        .as_str()
        .expect("self-host compiler prints JSON");
    let result: Value = serde_json::from_str(program_stdout).unwrap_or_else(|error| {
        panic!("{case_id}: invalid compiler result: {error}: {program_stdout}")
    });
    let self_host_diagnostic = result.get("diagnostic").and_then(self_host_location);
    let (bootstrap_artifact, bootstrap_diagnostic) = match bootstrap {
        Ok(artifact) => (Some(artifact), None),
        Err(error) => (None, Some(bootstrap_location(&error))),
    };
    let self_host_artifact = if self_host_diagnostic.is_none() {
        let ir = case_project.0.join("self-host-output.json");
        let artifact_path = case_project.0.join("self-host-output.vrb");
        fs::write(&ir, program_stdout).expect("write self-host IR for verification");
        let assembled = Command::new(binary)
            .args(["assemble-ir"])
            .arg(&ir)
            .arg("--output")
            .arg(&artifact_path)
            .current_dir(&case_project.0)
            .output()
            .unwrap_or_else(|error| panic!("{case_id}: launch self-host IR verifier: {error}"));
        assert!(
            assembled.status.success(),
            "{case_id}: {}",
            String::from_utf8_lossy(&assembled.stderr)
        );
        Some(
            Artifact::from_bytes(
                &fs::read(&artifact_path).expect("read assembled self-host artifact"),
            )
            .unwrap_or_else(|error| {
                panic!("{case_id}: self-host IR produced an invalid artifact: {error:?}")
            }),
        )
    } else {
        None
    };
    (
        bootstrap_artifact,
        self_host_artifact,
        bootstrap_diagnostic,
        self_host_diagnostic,
    )
}

fn prepare_self_host_compiler() -> PathBuf {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let self_host = repository.join("language/self_host");
    let filenames = [
        "veritas.toml",
        "compiler.vr",
        "lexer.vr",
        "parser.vr",
        "builtin_catalog.vr",
        "artifact.vr",
    ];
    let mut hasher = DefaultHasher::new();
    for filename in filenames {
        filename.hash(&mut hasher);
        fs::read(self_host.join(filename))
            .expect("read self-host compiler source for cache identity")
            .hash(&mut hasher);
    }
    let project = repository.join("target").join(format!(
        "veritas-language-conformance-{:016x}",
        hasher.finish()
    ));
    fs::create_dir_all(&project).expect("create compiler artifact cache");
    let artifact = project.join("compiler.vrb");
    if artifact.is_file() {
        return artifact;
    }
    for filename in filenames {
        fs::copy(self_host.join(filename), project.join(filename))
            .expect("copy self-host compiler");
    }
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_veritas"));
    let stage_zero = Command::new(&binary)
        .args(["run", "compiler.vr", "compiler.vr"])
        .current_dir(&project)
        .output()
        .expect("compile self-host compiler with bootstrap compiler");
    assert!(
        stage_zero.status.success(),
        "{}{}",
        String::from_utf8_lossy(&stage_zero.stdout),
        String::from_utf8_lossy(&stage_zero.stderr)
    );
    let ir = project.join("compiler.vri.json");
    fs::write(&ir, stage_zero.stdout).expect("write compiler IR");
    let assembled = Command::new(&binary)
        .args(["assemble-ir"])
        .arg(&ir)
        .arg("--output")
        .arg(&artifact)
        .current_dir(&project)
        .output()
        .expect("assemble self-host compiler artifact");
    assert!(
        assembled.status.success(),
        "{}",
        String::from_utf8_lossy(&assembled.stderr)
    );
    artifact
}

#[test]
fn manifest_current_language_corpus_has_bootstrap_and_self_host_parity() {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest: Value = serde_json::from_slice(
        &fs::read(repository.join("language/conformance/manifest.json")).expect("read manifest"),
    )
    .expect("parse conformance manifest");
    let entries = manifest["entries"].as_array().expect("manifest entries");
    let task_two: Vec<_> = entries
        .iter()
        .filter(|entry| entry["owner_task"] == 2)
        .collect();
    assert_eq!(
        task_two.len(),
        65,
        "Task 2 manifest feature count changed; update corpus classification"
    );

    let compiler_artifact = prepare_self_host_compiler();
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_veritas"));
    let mut ran = 0usize;
    let mut failures = Vec::new();
    for entry in task_two {
        let id = entry["id"].as_str().expect("feature ID");
        for class in ["positive", "negative", "interaction"] {
            let fixture = source_case(id, class)
                .unwrap_or_else(|| panic!("missing {class} corpus case for {id}"));
            let case_project = TempProject::new(id);
            for (path, source) in extra_modules(id, class) {
                case_project.write(path, source);
            }
            let (
                bootstrap_artifact,
                self_host_artifact,
                bootstrap_diagnostic,
                self_host_diagnostic,
            ) = compile_both(
                &binary,
                &compiler_artifact,
                &case_project,
                &fixture.source,
                &format!("{id}.{class}"),
            );
            let bootstrap_ok = bootstrap_artifact.is_some();
            let self_host_ok = self_host_artifact.is_some();
            if bootstrap_ok != fixture.accepts {
                failures.push(format!("{id}.{class}: bootstrap acceptance={}, expected={}, diagnostic={bootstrap_diagnostic:?}", bootstrap_ok, fixture.accepts));
            }
            if self_host_ok != fixture.accepts {
                failures.push(format!("{id}.{class}: self-host acceptance={}, expected={}, diagnostic={self_host_diagnostic:?}", self_host_ok, fixture.accepts));
            }
            if !fixture.accepts {
                if self_host_diagnostic != bootstrap_diagnostic {
                    failures.push(format!("{id}.{class}: self-host diagnostic/location={self_host_diagnostic:?}, bootstrap diagnostic/location={bootstrap_diagnostic:?}"));
                }
            } else if let (Some(bootstrap), Some(self_host)) =
                (bootstrap_artifact, self_host_artifact)
            {
                let bootstrap_observation = observe(bootstrap, &case_project.0);
                let self_host_observation = observe(self_host, &case_project.0);
                match (bootstrap_observation, self_host_observation) {
                    (Ok(bootstrap), Ok(self_host)) if bootstrap != self_host => {
                        failures.push(format!("{id}.{class}: bootstrap runtime={bootstrap:?}, self-host runtime={self_host:?}"));
                    }
                    (Err(error), _) => failures.push(format!("{id}.{class}: bootstrap {error}")),
                    (_, Err(error)) => failures.push(format!("{id}.{class}: self-host {error}")),
                    _ => {}
                }
            }
            ran += 1;
        }
    }
    assert_eq!(
        ran, 195,
        "every current positive, negative and interaction case must run"
    );
    assert!(
        failures.is_empty(),
        "compiler corpus mismatches:\n{}",
        failures.join("\n")
    );
}

#[test]
fn compiler_stages_compile_full_task_two_corpus_and_locked_roots() {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let stage_directory = repository.join("target/self-hosted-bootstrap");
    let stage2 = Artifact::from_bytes(
        &fs::read(stage_directory.join("stage2.vrb"))
            .expect("run three-stage bootstrap before stage conformance"),
    )
    .expect("native reader accepts cached C2");
    let stage3 = Artifact::from_bytes(
        &fs::read(stage_directory.join("stage3.vrb"))
            .expect("run three-stage bootstrap before stage conformance"),
    )
    .expect("native reader accepts cached C3");
    let project = TempProject::new("stage-two-three");
    let standard_root = project.0.join("stdlib");
    let package_root = project.0.join("packages/report_helpers");
    fs::create_dir_all(&standard_root).unwrap();
    fs::create_dir_all(package_root.join("render")).unwrap();
    let policy = HostPolicy {
        instruction_limit: 500_000_000,
        allocation_limit_bytes: 8_589_934_592,
        call_depth_limit: 160,
        max_collection_items: 1_000_000,
        ..HostPolicy::default()
    }
    .with_file_access(&project.0, true, false)
    .with_file_root_alias("std", &standard_root)
    .with_file_root_alias("pkg:report_helpers", &package_root);
    let mut stage2_runtime = Runtime::new()
        .load(stage2, policy.clone())
        .expect("load C2 self-hosted compiler");
    let mut stage3_runtime = Runtime::new()
        .load(stage3, policy)
        .expect("load C3 self-hosted compiler");

    let manifest: Value = serde_json::from_slice(
        &fs::read(repository.join("language/conformance/manifest.json"))
            .expect("read compiler conformance inventory"),
    )
    .expect("parse compiler conformance inventory");
    let entries = manifest["entries"].as_array().expect("manifest entries");
    let task_two: Vec<_> = entries
        .iter()
        .filter(|entry| entry["owner_task"] == 2)
        .collect();
    assert_eq!(task_two.len(), 65, "Task 2 conformance inventory changed");
    let mut cases_run = 0usize;
    for entry in task_two {
        let id = entry["id"].as_str().expect("feature ID");
        for class in ["positive", "negative", "interaction"] {
            let fixture =
                source_case(id, class).unwrap_or_else(|| panic!("missing {class} case for {id}"));
            eprintln!("C2/C3 corpus case {}/195: {id}.{class}", cases_run + 1);
            let case_root = format!("cases/{id}/{class}");
            let source_name = format!("{case_root}/main.vr");
            project.write(&source_name, &fixture.source);
            for (path, source) in extra_modules(id, class) {
                project.write(&format!("{case_root}/{path}"), source);
            }
            let stage2_result = compile_with_stage(
                &mut stage2_runtime,
                &source_name,
                &fixture.source,
                "",
                serde_json::json!({}),
            );
            let stage3_result = compile_with_stage(
                &mut stage3_runtime,
                &source_name,
                &fixture.source,
                "",
                serde_json::json!({}),
            );
            match (fixture.accepts, stage2_result, stage3_result) {
                (true, Ok(stage2_bytes), Ok(stage3_bytes)) => {
                    assert_eq!(
                        stage2_bytes, stage3_bytes,
                        "C2/C3 artifact mismatch for {id}.{class}"
                    );
                    let artifact = Artifact::from_bytes(&stage2_bytes)
                        .unwrap_or_else(|error| panic!("{id}.{class}: {error:?}"));
                    Runtime::new()
                        .load(artifact, HostPolicy::default())
                        .unwrap_or_else(|error| panic!("{id}.{class}: verifier: {error:?}"));
                }
                (false, Err(stage2_error), Err(stage3_error)) => {
                    assert_eq!(
                        stage2_error, stage3_error,
                        "C2/C3 diagnostic mismatch for {id}.{class}"
                    );
                    assert_eq!(
                        stage2_error["schema"], "veritas-compile-failure/v1",
                        "negative input must be a structured compile failure: {id}.{class}"
                    );
                }
                (expected, stage2_result, stage3_result) => panic!(
                    "C2/C3 acceptance mismatch for {id}.{class}: expected {expected}, C2={stage2_result:?}, C3={stage3_result:?}"
                ),
            }
            cases_run += 1;
        }
    }
    assert!(
        cases_run >= 180,
        "unexpectedly small C2/C3 corpus run: {cases_run}"
    );

    let standard_source = "import \"std:csv\"\nimport \"pkg:report_helpers/render/table.vr\"\n\nfn main() -> Int:\n    return csv_rows() + rendered_rows()\n";
    project.write(
        "stdlib/csv.vr",
        "pub fn csv_rows() -> Int:\n    return 40\n",
    );
    project.write(
        "packages/report_helpers/render/table.vr",
        "pub fn rendered_rows() -> Int:\n    return 2\n",
    );
    project.write("roots/main.vr", standard_source);
    let roots = serde_json::json!({ "report_helpers": "pkg:report_helpers" });
    let stage2_bytes = compile_with_stage(
        &mut stage2_runtime,
        "roots/main.vr",
        standard_source,
        "std",
        roots.clone(),
    )
    .expect("C2 resolves explicitly granted standard and package roots");
    let stage3_bytes = compile_with_stage(
        &mut stage3_runtime,
        "roots/main.vr",
        standard_source,
        "std",
        roots,
    )
    .expect("C3 resolves explicitly granted standard and package roots");
    assert_eq!(stage2_bytes, stage3_bytes);
    let mut application = Runtime::new()
        .load(
            Artifact::from_bytes(&stage2_bytes).unwrap(),
            HostPolicy::default(),
        )
        .expect("verify application built by C2");
    assert_eq!(
        application.invoke("main", vec![]).unwrap(),
        RuntimeValue::Int(BigInt::from_usize(42))
    );
}

fn compile_with_stage(
    runtime: &mut Instance,
    source_name: &str,
    source: &str,
    stdlib_root: &str,
    package_roots: Value,
) -> Result<Vec<u8>, Value> {
    let result = runtime
        .invoke(
            "compiler_compile_project",
            vec![
                RuntimeValue::Text(source_name.into()),
                RuntimeValue::Text(source.into()),
                RuntimeValue::Text(stdlib_root.into()),
                RuntimeValue::Json(SharedJson::from_serde(package_roots)),
            ],
        )
        .expect("C2/C3 compiler entry returns a Result");
    match result {
        RuntimeValue::ResultOk(value) => match *value {
            RuntimeValue::Bytes(bytes) => Ok(bytes.as_ref().clone()),
            value => panic!("C2/C3 returned {value:?}; expected Bytes"),
        },
        RuntimeValue::ResultErr(value) => match *value {
            RuntimeValue::Json(error) => Err(error.to_serde()),
            value => panic!("C2/C3 returned {value:?}; expected JSON diagnostic"),
        },
        value => panic!("C2/C3 returned {value:?}; expected Result"),
    }
}

#[test]
fn collection_updates_preserve_preexisting_aliases_in_both_compilers() {
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_veritas"));
    let compiler_artifact = prepare_self_host_compiler();
    let project = TempProject::new("collection-aliases");
    let source = "fn main() -> Int:\n    let original_list = [20]\n    let list_alias = original_list\n    let changed_list = list_push(original_list, 22)\n    let original_map = {\"answer\": 40}\n    let map_alias = original_map\n    let changed_map = map_set(original_map, \"other\", 2)\n    if len(list_alias) == 1 and changed_list[1] == 22 and not map_has(map_alias, \"other\") and map_has(changed_map, \"other\"):\n        return 42\n    else:\n        return 0\n";
    let (bootstrap, self_host, bootstrap_diagnostic, self_host_diagnostic) = compile_both(
        &binary,
        &compiler_artifact,
        &project,
        source,
        "collection alias preservation",
    );
    assert!(bootstrap_diagnostic.is_none(), "{bootstrap_diagnostic:?}");
    assert!(self_host_diagnostic.is_none(), "{self_host_diagnostic:?}");
    let bootstrap =
        observe(bootstrap.expect("bootstrap artifact"), &project.0).expect("bootstrap runtime");
    let self_host =
        observe(self_host.expect("self-host artifact"), &project.0).expect("self-host runtime");
    assert_eq!(
        bootstrap, self_host,
        "compiler outputs, effects, and aliases must agree"
    );
    assert_eq!(
        bootstrap.result,
        Ok(RuntimeValue::Int(BigInt::from_usize(42)))
    );
}

fn extra_modules(id: &str, class: &str) -> Vec<(&'static str, &'static str)> {
    match (id, class) {
        ("syntax-relative-module-imports-and-project-root-confinement", "positive")
        | ("syntax-import-graph-size-depth-and-total-source-limits", "positive") => vec![
            ("lib.vr", "pub fn answer() -> Int:\n    return 42\n"),
        ],
        ("syntax-relative-module-imports-and-project-root-confinement", "interaction")
        | ("syntax-import-graph-size-depth-and-total-source-limits", "interaction") => vec![
            ("first.vr", "import \"shared.vr\"\n\npub fn first() -> Int:\n    return shared_value()\n"),
            ("second.vr", "import \"shared.vr\"\n\npub fn second() -> Int:\n    return shared_value()\n"),
            ("shared.vr", "pub fn shared_value() -> Int:\n    return 21\n"),
        ],
        ("syntax-public-and-private-declarations", "negative") => vec![
            ("lib.vr", "fn hidden() -> Int:\n    return 42\n"),
        ],
        ("syntax-public-and-private-declarations", "interaction") => vec![
            ("first.vr", "fn helper() -> Int:\n    return 20\n\npub fn first() -> Int:\n    return helper()\n"),
            ("second.vr", "fn helper() -> Int:\n    return 22\n\npub fn second() -> Int:\n    return helper()\n"),
        ],
        _ => Vec::new(),
    }
}
