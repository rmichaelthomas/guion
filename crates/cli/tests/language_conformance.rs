use guion_compiler::{compile_project, Diagnostic};
use guion_model::{Artifact, BigInt, RuntimeError, Value as RuntimeValue};
use guion_runtime::{EffectRecord, HostPolicy, Runtime};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

struct TempProject(PathBuf);

impl TempProject {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock is after Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "guion-language-conformance-{}-{label}-{nonce}",
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
            "# UTF-8 comment\nfn label() -> Text:\n    return \"guion-é\"\n\nfn main() -> Int:\n    return len(label())\n"
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
            "import \"facade.gu\"\n\nfn main() -> Int:\n    let token = Token(41)\n    let incremented = increment(token)\n    assert(incremented.value == 42)\n    return answer()\n"
        }
        ("syntax-relative-module-imports-and-project-root-confinement", "negative") => {
            "import \"../outside.gu\"\n\nfn main() -> Int:\n    return 0\n"
        }
        ("syntax-relative-module-imports-and-project-root-confinement", "interaction") => {
            "import \"first.gu\"\nimport \"second.gu\"\n\nfn main() -> Int:\n    return first() + second()\n"
        }
        ("syntax-public-and-private-declarations", "positive") => {
            "fn helper() -> Int:\n    return 42\n\nfn main() -> Int:\n    return helper()\n"
        }
        ("syntax-public-and-private-declarations", "negative") => {
            "import \"lib.gu\"\n\nfn main() -> Int:\n    return hidden()\n"
        }
        ("syntax-public-and-private-declarations", "interaction") => {
            "import \"first.gu\"\nimport \"second.gu\"\n\nfn main() -> Int:\n    return first() + second()\n"
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
        ("syntax-text-slice-unicode-scalar-ranges", "positive") => {
            "fn main() -> Int:\n    return len(text_slice(\"a🙂é\", 1, 3))\n"
        }
        ("syntax-text-slice-unicode-scalar-ranges", "negative") => {
            "fn main() -> Int:\n    return len(text_slice(\"abc\", 0, \"2\"))\n"
        }
        ("syntax-text-slice-unicode-scalar-ranges", "interaction") => {
            "fn main() -> Int:\n    let text = \"a🙂é\"\n    let middle = text_slice(text, 1, 2)\n    return len(middle) + len(text_slice(text, 0, 1))\n"
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
            "import \"lib.gu\"\n\nfn main() -> Int:\n    return answer()\n"
        }
        ("syntax-import-graph-size-depth-and-total-source-limits", "negative") => {
            "import \"../outside.gu\"\n\nfn main() -> Int:\n    return 0\n"
        }
        ("syntax-import-graph-size-depth-and-total-source-limits", "interaction") => {
            "import \"first.gu\"\nimport \"second.gu\"\n\nfn main() -> Int:\n    return first() + second()\n"
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
            _ => "len(\"guion\") + (20 + 15)",
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
        ("host_operation-local-time-resolution", "positive") => "record LocalDateTime:\n    year: Int\n    month: Int\n    day: Int\n    hour: Int\n    minute: Int\n    second: Int\n    microsecond: Int\nrecord Instant:\n    unix_microseconds: Int\nrecord TemporalResolution:\n    kind: Text\n    local: LocalDateTime\n    timezone: Text\n    instants: List[Instant]\nrecord TimeError:\n    code: Text\n    message: Text\nfn main() -> Int:\n    let result = resolve_local(LocalDateTime(2026, 11, 1, 1, 30, 0, 0), \"America/Los_Angeles\")\n    if is_ok(result) and len(time_zone_data_version()) > 0:\n        return 0\n    else:\n        return 1\n",
        ("host_operation-local-time-resolution", "negative") => "record LocalDateTime:\n    year: Int\n    month: Int\n    day: Int\n    hour: Int\n    minute: Int\n    second: Int\n    microsecond: Int\nrecord Instant:\n    unix_microseconds: Int\nrecord TemporalResolution:\n    kind: Text\n    local: LocalDateTime\n    timezone: Text\n    instants: List[Instant]\nrecord TimeError:\n    code: Text\n    message: Text\nfn main() -> Int:\n    let result = resolve_local(42, \"America/Los_Angeles\")\n    return 0\n",
        ("host_operation-local-time-resolution", "interaction") => "record LocalDateTime:\n    year: Int\n    month: Int\n    day: Int\n    hour: Int\n    minute: Int\n    second: Int\n    microsecond: Int\nrecord Instant:\n    unix_microseconds: Int\nrecord TemporalResolution:\n    kind: Text\n    local: LocalDateTime\n    timezone: Text\n    instants: List[Instant]\nrecord TimeError:\n    code: Text\n    message: Text\nfn main() -> Int:\n    let exact = unwrap(resolve_local(LocalDateTime(2026, 3, 8, 1, 30, 0, 0), \"America/Los_Angeles\"))\n    let gap = unwrap(resolve_local(LocalDateTime(2026, 3, 8, 2, 30, 0, 0), \"America/Los_Angeles\"))\n    let fold = unwrap(resolve_local(LocalDateTime(2026, 11, 1, 1, 30, 0, 0), \"America/Los_Angeles\"))\n    if exact.kind == \"exact\" and len(exact.instants) == 1 and gap.kind == \"gap\" and len(gap.instants) == 0 and fold.kind == \"fold\" and len(fold.instants) == 2 and fold.instants[0].unix_microseconds < fold.instants[1].unix_microseconds and time_zone_data_version() == \"2026c\":\n        return 0\n    else:\n        return 1\n",
        _ => return None,
    };
    let source = if id == "host_operation-local-time-resolution" {
        source.replace(
            "fn main() -> Int:",
            "fn resolve_local(local: LocalDateTime, timezone: Text) -> Result[TemporalResolution, TimeError]:\n    return time_resolve_local(local, timezone)\n\nfn time_zone_data_version() -> Text:\n    return time_zone_data_version_native()\n\nfn main() -> Int:",
        )
    } else {
        source.to_owned()
    };
    Some(source)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DiagnosticLocation {
    code: String,
    source: String,
    line: usize,
    column: usize,
    suggestions: Vec<String>,
}

fn compile_location(error: &Diagnostic) -> DiagnosticLocation {
    DiagnosticLocation {
        code: error.code.clone(),
        source: error.source_name.clone().unwrap_or_default(),
        line: error.span.line,
        column: error.span.column,
        suggestions: error.suggestions().to_vec(),
    }
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

fn compile_rust(
    case_project: &TempProject,
    source: &str,
) -> (Option<Artifact>, Option<DiagnosticLocation>) {
    case_project.write("main.gu", source);
    match compile_project(&case_project.0.join("main.gu"), &case_project.0) {
        Ok(artifact) => (Some(artifact), None),
        Err(error) => (None, Some(compile_location(&error))),
    }
}

#[test]
fn manifest_current_language_corpus_compiles_and_runs() {
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
        67,
        "Task 2 manifest feature count changed; update corpus classification"
    );

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
            let (artifact, diagnostic) = compile_rust(&case_project, &fixture.source);
            let accepted = artifact.is_some();
            if accepted != fixture.accepts {
                failures.push(format!(
                    "{id}.{class}: acceptance={accepted}, expected={}, diagnostic={diagnostic:?}",
                    fixture.accepts
                ));
            }
            if let Some(artifact) = artifact {
                if let Err(error) = observe(artifact, &case_project.0) {
                    failures.push(format!("{id}.{class}: {error}"));
                }
            }
            ran += 1;
        }
    }
    assert_eq!(
        ran, 201,
        "every current positive, negative and interaction case must run"
    );
    assert!(
        failures.is_empty(),
        "compiler corpus mismatches:\n{}",
        failures.join("\n")
    );
}

#[test]
fn temporal_resolver_conformance_is_declared() {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest: Value = serde_json::from_slice(
        &fs::read(repository.join("language/conformance/manifest.json")).unwrap(),
    )
    .unwrap();
    let entry = manifest["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == "host_operation-local-time-resolution")
        .expect("temporal resolver has a conformance entry");
    assert_eq!(entry["compiler_status"]["bootstrap"], "implemented");
    assert_eq!(entry["coverage"]["positive"]["state"], "implemented");
    assert_eq!(entry["coverage"]["negative"]["state"], "implemented");
    assert_eq!(entry["coverage"]["interaction"]["state"], "implemented");
}

#[test]
fn collection_updates_preserve_preexisting_aliases() {
    let project = TempProject::new("collection-aliases");
    let source = "fn main() -> Int:\n    let original_list = [20]\n    let list_alias = original_list\n    let changed_list = list_push(original_list, 22)\n    let original_map = {\"answer\": 40}\n    let map_alias = original_map\n    let changed_map = map_set(original_map, \"other\", 2)\n    if len(list_alias) == 1 and changed_list[1] == 22 and not map_has(map_alias, \"other\") and map_has(changed_map, \"other\"):\n        return 42\n    else:\n        return 0\n";
    let (artifact, diagnostic) = compile_rust(&project, source);
    assert!(diagnostic.is_none(), "{diagnostic:?}");
    let observed = observe(artifact.expect("compiled artifact"), &project.0).expect("runtime");
    assert_eq!(
        observed.result,
        Ok(RuntimeValue::Int(BigInt::from_usize(42)))
    );
}

fn extra_modules(id: &str, class: &str) -> Vec<(&'static str, &'static str)> {
    match (id, class) {
        ("syntax-relative-module-imports-and-project-root-confinement", "positive") => vec![
            ("facade.gu", "pub import \"inner.gu\"\npub fn answer() -> Int:\n    return 42\n"),
            ("inner.gu", "pub record Token:\n    value: Int\n\npub fn increment(token: Token) -> Token:\n    return Token(token.value + 1)\n"),
        ],
        ("syntax-import-graph-size-depth-and-total-source-limits", "positive") => vec![
            ("lib.gu", "pub fn answer() -> Int:\n    return 42\n"),
        ],
        ("syntax-relative-module-imports-and-project-root-confinement", "interaction")
        | ("syntax-import-graph-size-depth-and-total-source-limits", "interaction") => vec![
            ("first.gu", "import \"shared.gu\"\n\npub fn first() -> Int:\n    return shared_value()\n"),
            ("second.gu", "import \"shared.gu\"\n\npub fn second() -> Int:\n    return shared_value()\n"),
            ("shared.gu", "pub fn shared_value() -> Int:\n    return 21\n"),
        ],
        ("syntax-public-and-private-declarations", "negative") => vec![
            ("lib.gu", "fn hidden() -> Int:\n    return 42\n"),
        ],
        ("syntax-public-and-private-declarations", "interaction") => vec![
            ("first.gu", "fn helper() -> Int:\n    return 20\n\npub fn first() -> Int:\n    return helper()\n"),
            ("second.gu", "fn helper() -> Int:\n    return 22\n\npub fn second() -> Int:\n    return helper()\n"),
        ],
        _ => Vec::new(),
    }
}

// ---- `break` and `continue` (artifact format unchanged: they lower to jumps) ----

struct LoopJumpCase {
    name: &'static str,
    source: &'static str,
    // Ok(value returned by main) or Err((code, line, column)).
    expected: Result<usize, (&'static str, usize, usize)>,
}

fn loop_jump_cases() -> Vec<LoopJumpCase> {
    vec![
        LoopJumpCase {
            name: "while-true-break",
            source: "fn main() -> Int:\n    var i = 0\n    while true:\n        if i == 5:\n            break\n        set i = i + 1\n    return i * 10 + 2\n",
            expected: Ok(52),
        },
        LoopJumpCase {
            name: "for-continue",
            source: "fn main() -> Int:\n    var total = 0\n    for n in [1, 2, 3, 4, 5, 6]:\n        if n % 2 == 0:\n            continue\n        set total = total + n\n    return total\n",
            expected: Ok(9),
        },
        LoopJumpCase {
            name: "while-continue",
            source: "fn main() -> Int:\n    var i = 0\n    var total = 0\n    while i < 6:\n        set i = i + 1\n        if i == 3:\n            continue\n        set total = total + i\n    return total\n",
            expected: Ok(18),
        },
        LoopJumpCase {
            name: "for-break",
            source: "fn main() -> Int:\n    var total = 0\n    for n in [1, 2, 3, 4, 5]:\n        if n == 4:\n            break\n        set total = total + n\n    return total\n",
            expected: Ok(6),
        },
        LoopJumpCase {
            name: "break-at-function-top-level",
            source: "fn main() -> Int:\n    break\n    return 0\n",
            expected: Err(("E_NO_LOOP", 2, 5)),
        },
        LoopJumpCase {
            name: "continue-at-function-top-level",
            source: "fn main() -> Int:\n    continue\n    return 0\n",
            expected: Err(("E_NO_LOOP", 2, 5)),
        },
        LoopJumpCase {
            name: "break-after-the-loop-has-ended",
            source: "fn main() -> Int:\n    var i = 0\n    while i < 3:\n        set i = i + 1\n    if i == 3:\n        break\n    return i\n",
            expected: Err(("E_NO_LOOP", 6, 9)),
        },
        LoopJumpCase {
            name: "parallel-map-worker-loops",
            source: "fn work(limit: Int) -> Int:\n    var i = 0\n    var total = 0\n    while true:\n        set i = i + 1\n        if i > limit:\n            break\n        if i % 2 == 1:\n            continue\n        set total = total + i\n    return total\n\nfn run() -> Result[List[Int], TaskError]:\n    return parallel_map([4, 6, 10], work, 2)\n\nfn main() -> Int:\n    var sum = 0\n    for value in unwrap(run()):\n        set sum = sum + value\n    return sum\n",
            expected: Ok(48),
        },
        LoopJumpCase {
            name: "nested-inner-break-leaves-only-the-inner-loop",
            source: "fn main() -> Int:\n    var count = 0\n    for a in [1, 2, 3]:\n        for b in [1, 2, 3, 4]:\n            if b == 3:\n                break\n            set count = count + 1\n    return count\n",
            expected: Ok(6),
        },
        LoopJumpCase {
            name: "nested-inner-continue-skips-only-the-inner-item",
            source: "fn main() -> Int:\n    var count = 0\n    var i = 0\n    while i < 3:\n        set i = i + 1\n        for b in [1, 2, 3, 4]:\n            if b % 2 == 1:\n                continue\n            set count = count + 1\n    return count\n",
            expected: Ok(6),
        },
        LoopJumpCase {
            name: "break-and-continue-inside-match-arms",
            source: "fn pick(n: Int) -> Option[Int]:\n    if n > 2:\n        return Some(n)\n    return None\n\nfn main() -> Int:\n    var found = 0\n    for n in [1, 2, 3, 4]:\n        match pick(n):\n            Some(value):\n                set found = value\n                break\n            None:\n                continue\n        set found = 99\n    return found\n",
            expected: Ok(3),
        },
    ]
}

#[test]
fn break_and_continue_cases_compile_and_run() {
    for case in loop_jump_cases() {
        let project = TempProject::new(case.name);
        let (artifact, diagnostic) = compile_rust(&project, case.source);
        match case.expected {
            Err((code, line, column)) => {
                let expected = DiagnosticLocation {
                    code: code.into(),
                    source: "main.gu".into(),
                    line,
                    column,
                    suggestions: Vec::new(),
                };
                assert!(artifact.is_none(), "{}", case.name);
                assert_eq!(diagnostic.as_ref(), Some(&expected), "{}", case.name);
            }
            Ok(value) => {
                let artifact =
                    artifact.unwrap_or_else(|| panic!("{}: rejected: {diagnostic:?}", case.name));
                let observed = observe(artifact, &project.0).expect("artifact loads");
                assert_eq!(
                    observed.result,
                    Ok(RuntimeValue::Int(BigInt::from_usize(value))),
                    "{}",
                    case.name
                );
            }
        }
    }
}

// ---- Suggestions on unknown call names (B9) ----

struct UnknownCallCase {
    name: &'static str,
    source: &'static str,
    callee: &'static str,
    line: usize,
    column: usize,
    suggestions: &'static [&'static str],
}

fn unknown_call_cases() -> Vec<UnknownCallCase> {
    vec![
        UnknownCallCase {
            name: "sortt-suggests-sort-then-sqrt",
            source: "fn main() -> Int:\n    return sortt(3)\n",
            callee: "sortt",
            line: 2,
            column: 12,
            suggestions: &["sort", "sqrt"],
        },
        UnknownCallCase {
            name: "map-offers-main-and-the-list-builtin",
            source: "fn main() -> Int:\n    return map(3)\n",
            callee: "map",
            line: 2,
            column: 12,
            suggestions: &["main", "map_list"],
        },
        UnknownCallCase {
            name: "filter-suggests-filter-list",
            source: "fn main() -> Int:\n    return filter(3)\n",
            callee: "filter",
            line: 2,
            column: 12,
            suggestions: &["filter_list"],
        },
        UnknownCallCase {
            name: "fold-suggests-fold-list",
            source: "fn main() -> Int:\n    return fold(3)\n",
            callee: "fold",
            line: 2,
            column: 12,
            suggestions: &["fold_list"],
        },
        UnknownCallCase {
            name: "iterat-suggests-the-module-function",
            source: "fn iterate(n: Int) -> Int:\n    return n\n\nfn main() -> Int:\n    return iterat(3)\n",
            callee: "iterat",
            line: 5,
            column: 12,
            suggestions: &["iterate"],
        },
        UnknownCallCase {
            name: "nonsense-name-gets-none",
            source: "fn main() -> Int:\n    return qzxwv(3)\n",
            callee: "qzxwv",
            line: 2,
            column: 12,
            suggestions: &[],
        },
        UnknownCallCase {
            name: "three-way-tie-is-capped-and-sorted-by-name",
            source: "fn tab1() -> Int:\n    return 1\n\nfn tab2() -> Int:\n    return 1\n\nfn tab3() -> Int:\n    return 1\n\nfn tab() -> Int:\n    return 1\n\nfn main() -> Int:\n    return tabb()\n",
            callee: "tabb",
            line: 14,
            column: 12,
            suggestions: &["tab", "tab1", "tab2"],
        },
        UnknownCallCase {
            name: "closer-candidate-sorts-first",
            source: "fn length() -> Int:\n    return 1\n\nfn main() -> Int:\n    return lenth(3)\n",
            callee: "lenth",
            line: 5,
            column: 12,
            suggestions: &["length", "len"],
        },
    ]
}

#[test]
fn unknown_call_suggestions_are_reported() {
    for case in unknown_call_cases() {
        let project = TempProject::new(case.name);
        let (artifact, diagnostic) = compile_rust(&project, case.source);
        let expected = DiagnosticLocation {
            code: "E_NAME_UNKNOWN".into(),
            source: "main.gu".into(),
            line: case.line,
            column: case.column,
            suggestions: case.suggestions.iter().map(|s| s.to_string()).collect(),
        };
        assert!(artifact.is_none(), "{}", case.name);
        assert_eq!(diagnostic.as_ref(), Some(&expected), "{}", case.name);
        let _ = case.callee;
    }
}

// ---- General `sort` and `sort_by` (artifact format 65) ----

struct SortCase {
    name: &'static str,
    source: &'static str,
    // Ok(stdout) or Err((code, line, column)).
    expected: Result<&'static str, (&'static str, usize, usize)>,
}

fn sort_cases() -> Vec<SortCase> {
    vec![
        SortCase {
            name: "sort-text-int-float",
            source: "fn main() -> Unit:\n    print(sort([\"bb\", \"ccc\", \"a\"]))\n    print(sort([3, -1, 2]))\n    print(sort([2.5, -0.0, 0.0, -1.5]))\n",
            expected: Ok("[a, bb, ccc]\n[-1, 2, 3]\n[-1.5, -0, 0, 2.5]\n"),
        },
        SortCase {
            name: "sort-by-text-and-len-keys",
            source: "fn same(word: Text) -> Text:\n    return word\n\nfn length(word: Text) -> Int:\n    return len(word)\n\nfn main() -> Unit:\n    let words = [\"bb\", \"a\", \"ccc\"]\n    print(sort_by(words, same))\n    print(sort_by(words, length))\n    print(words)\n",
            expected: Ok("[a, bb, ccc]\n[a, bb, ccc]\n[bb, a, ccc]\n"),
        },
        SortCase {
            name: "sort-by-float-key-is-stable",
            source: "record Cell:\n    id: Int\n    value: Float64\n\nfn by_value(cell: Cell) -> Float64:\n    return cell.value\n\nfn main() -> Unit:\n    let cells = [Cell(1, 0.0), Cell(2, -0.0), Cell(3, 0.0), Cell(4, -0.0)]\n    var out = \"\"\n    for cell in sort_by(cells, by_value):\n        set out = out + int_to_text(cell.id)\n    print(out)\n",
            expected: Ok("2413\n"),
        },
        SortCase {
            name: "sort-by-comparator-still-works",
            source: "fn compare(left: Int, right: Int) -> Int:\n    return right - left\n\nfn main() -> Unit:\n    print(sort_by([1, 3, 2], compare))\n",
            expected: Ok("[3, 2, 1]\n"),
        },
        SortCase {
            name: "sort-by-int-key-still-works",
            source: "fn negate(value: Int) -> Int:\n    return 0 - value\n\nfn main() -> Unit:\n    print(sort_by_int_key([1, 3, 2], negate))\n",
            expected: Ok("[3, 2, 1]\n"),
        },
        SortCase {
            name: "sort-by-bool-key-is-rejected",
            source: "fn flag(value: Int) -> Bool:\n    return value > 1\n\nfn main() -> Int:\n    return len(sort_by([3, 1, 2], flag))\n",
            expected: Err(("E_SORT_KEY", 5, 35)),
        },
        SortCase {
            name: "sort-by-unit-key-is-rejected",
            source: "fn nothing(value: Int) -> Unit:\n    print(value)\n\nfn main() -> Int:\n    return len(sort_by([3, 1, 2], nothing))\n",
            expected: Err(("E_SORT_KEY", 5, 35)),
        },
        SortCase {
            name: "sort-of-a-bool-list-is-rejected",
            source: "fn main() -> Int:\n    return len(sort([true, false]))\n",
            expected: Err(("E_SORT_KEY", 2, 21)),
        },
    ]
}

#[test]
fn sort_cases_compile_and_run() {
    cases_compile_and_run(sort_cases());
}

fn cases_compile_and_run(cases: Vec<SortCase>) {
    for case in cases {
        let project = TempProject::new(case.name);
        let (artifact, diagnostic) = compile_rust(&project, case.source);
        match case.expected {
            Err((code, line, column)) => {
                let expected = DiagnosticLocation {
                    code: code.into(),
                    source: "main.gu".into(),
                    line,
                    column,
                    suggestions: Vec::new(),
                };
                assert!(artifact.is_none(), "{}", case.name);
                assert_eq!(diagnostic.as_ref(), Some(&expected), "{}", case.name);
            }
            Ok(stdout) => {
                let artifact =
                    artifact.unwrap_or_else(|| panic!("{}: rejected: {diagnostic:?}", case.name));
                assert_eq!(artifact.format_version, Artifact::CURRENT_VERSION);
                let observed = observe(artifact, &project.0).expect("artifact loads");
                assert_eq!(observed.stdout, stdout, "{}: output", case.name);
            }
        }
    }
}

// ---- First-class function values (artifact format 66) ----

fn function_value_cases() -> Vec<SortCase> {
    let double = "fn double(x: Int) -> Int:\n    return x * 2\n\n";
    let leak = |body: &'static str| -> &'static str {
        Box::leak(format!("{double}{body}").into_boxed_str())
    };
    vec![
        SortCase {
            name: "fn-value-let-and-call",
            source: leak("fn main() -> Unit:\n    let f = double\n    let g: fn(Int) -> Int = f\n    print(f(4))\n    print(g(5))\n"),
            expected: Ok("8\n10\n"),
        },
        SortCase {
            name: "fn-value-apply-twice",
            source: leak("fn apply_twice(f: fn(Int) -> Int, x: Int) -> Int:\n    return f(f(x))\n\nfn main() -> Unit:\n    print(apply_twice(double, 3))\n"),
            expected: Ok("12\n"),
        },
        SortCase {
            name: "fn-value-returned-and-composed",
            source: leak("fn add_one(x: Int) -> Int:\n    return x + 1\n\nfn pick(first: Bool) -> fn(Int) -> Int:\n    if first:\n        return double\n    return add_one\n\nfn compose(f: fn(Int) -> Int, g: fn(Int) -> Int, x: Int) -> Int:\n    return f(g(x))\n\nfn main() -> Unit:\n    print(compose(pick(true), pick(false), 5))\n"),
            expected: Ok("12\n"),
        },
        SortCase {
            name: "fn-value-local-shadows-function",
            source: leak("fn add_one(x: Int) -> Int:\n    return x + 1\n\nfn apply(double: fn(Int) -> Int, x: Int) -> Int:\n    return double(x)\n\nfn main() -> Unit:\n    print(apply(add_one, 1))\n    print(double(1))\n"),
            expected: Ok("2\n2\n"),
        },
        SortCase {
            name: "fn-value-in-record-list-and-map",
            source: leak("record Handler:\n    name: Text\n    run: fn(Int) -> Int\n\nfn main() -> Unit:\n    let handler = Handler(\"double\", double)\n    let run = handler.run\n    print(run(21))\n    let fns = [double, double]\n    var total = 0\n    for f in fns:\n        set total = total + f(3)\n    print(total)\n    let by_name = {\"double\": double}\n    match map_get(by_name, \"double\"):\n        Some(f):\n            print(f(9))\n        None:\n            print(\"missing\")\n"),
            expected: Ok("42\n12\n18\n"),
        },
        SortCase {
            name: "fn-value-sort-by-key-variable",
            source: "record Person:\n    name: Text\n    score: Int\n\nfn score_of(person: Person) -> Int:\n    return person.score\n\nfn name_of(person: Person) -> Text:\n    return person.name\n\nfn names(people: List[Person]) -> Text:\n    var out = \"\"\n    for person in people:\n        set out = out + person.name\n    return out\n\nfn main() -> Unit:\n    let people = [Person(\"c\", 3), Person(\"a\", 1), Person(\"b\", 2)]\n    let key = score_of\n    print(names(sort_by(people, key)))\n    print(names(sort_by(people, score_of)))\n    let by_name = name_of\n    print(names(sort_by(people, by_name)))\n",
            expected: Ok("abc\nabc\nabc\n"),
        },
        SortCase {
            name: "fn-value-sort-by-int-key-and-comparator-variables",
            source: "fn negate(x: Int) -> Int:\n    return 0 - x\n\nfn descending(left: Int, right: Int) -> Int:\n    return right - left\n\nfn main() -> Unit:\n    let key = negate\n    print(sort_by_int_key([1, 3, 2], key))\n    print(merge_sorted_by_int_key([[3, 1], [2]], key))\n    let compare = descending\n    print(sort_by([1, 3, 2], compare))\n",
            expected: Ok("[3, 2, 1]\n[3, 2, 1]\n[3, 2, 1]\n"),
        },
        SortCase {
            name: "fn-value-parallel-map-variable-worker",
            source: "fn square(x: Int) -> Int:\n    return x * x\n\nfn main() -> Unit:\n    let worker = square\n    match parallel_map([1, 2, 3], worker, 2):\n        Ok(values):\n            print(values)\n        Err(error):\n            print(\"failed\")\n",
            expected: Ok("[1, 4, 9]\n"),
        },
        SortCase {
            name: "fn-value-list-builtins",
            source: leak("fn add(total: Int, x: Int) -> Int:\n    return total + x\n\nfn big(x: Int) -> Bool:\n    return x > 2\n\nfn main() -> Unit:\n    print(map_list([1, 2, 3], double))\n    print(filter_list([1, 2, 3, 4], big))\n    print(fold_list([1, 2, 3], 10, add))\n    print(any_list([1, 2, 3], big))\n    print(all_list([1, 2, 3], big))\n    let f = double\n    print(map_list([4], f))\n"),
            expected: Ok("[2, 4, 6]\n[3, 4]\n16\ntrue\nfalse\n[8]\n"),
        },
        SortCase {
            name: "fn-value-compare-is-rejected",
            source: leak("fn main() -> Unit:\n    let f = double\n    let g = double\n    print(f == g)\n"),
            expected: Err(("E_FN_COMPARE", 7, 13)),
        },
        SortCase {
            name: "fn-value-call-with-wrong-arity",
            source: "fn add(a: Int, b: Int) -> Int:\n    return a + b\n\nfn main() -> Unit:\n    let f = add\n    print(f(1))\n",
            expected: Err(("E_CALL_ARITY", 6, 11)),
        },
        SortCase {
            name: "fn-value-call-with-wrong-type",
            source: "fn add(a: Int, b: Int) -> Int:\n    return a + b\n\nfn main() -> Unit:\n    let f = add\n    print(f(1, \"two\"))\n",
            expected: Err(("E_TYPE_ARGUMENT", 6, 16)),
        },
        SortCase {
            name: "fn-value-annotation-mismatch",
            source: leak("fn main() -> Unit:\n    let f: fn(Int) -> Text = double\n    print(1)\n"),
            expected: Err(("E_TYPE_ANNOTATION", 5, 5)),
        },
        SortCase {
            name: "fn-value-passed-with-the-wrong-shape",
            source: "fn add(a: Int, b: Int) -> Int:\n    return a + b\n\nfn apply(f: fn(Int) -> Int, x: Int) -> Int:\n    return f(x)\n\nfn main() -> Unit:\n    print(apply(add, 1))\n",
            expected: Err(("E_TYPE_ARGUMENT", 8, 17)),
        },
        SortCase {
            name: "fn-value-not-serializable-json",
            source: leak("fn main() -> Unit:\n    let f = double\n    print(json_stringify(json_set(json_object(), \"k\", f)))\n"),
            expected: Err(("E_FN_NOT_SERIALIZABLE", 6, 55)),
        },
        SortCase {
            name: "fn-value-not-serializable-record-field-in-process-args",
            source: leak("record Box:\n    run: fn(Int) -> Int\n\nfn main() -> Unit:\n    let box = Box(double)\n    native_process_run(\"ls\", [box], \"\")\n"),
            expected: Err(("E_FN_NOT_SERIALIZABLE", 9, 30)),
        },
        SortCase {
            name: "fn-value-not-serializable-print",
            source: leak("fn main() -> Unit:\n    print(double)\n"),
            expected: Err(("E_FN_NOT_SERIALIZABLE", 5, 11)),
        },
        SortCase {
            name: "fn-value-map-list-wrong-shape",
            source: "fn add(a: Int, b: Int) -> Int:\n    return a + b\n\nfn main() -> Unit:\n    print(map_list([1], add))\n",
            expected: Err(("E_TYPE_ARGUMENT", 5, 25)),
        },
        SortCase {
            name: "fn-value-fold-list-wrong-accumulator",
            source: "fn text_of(a: Text, b: Int) -> Text:\n    return a\n\nfn main() -> Unit:\n    print(fold_list([1], 0, text_of))\n",
            expected: Err(("E_TYPE_ARGUMENT", 5, 29)),
        },
        SortCase {
            name: "fn-value-call-of-a-non-function-local",
            source: "fn main() -> Unit:\n    let zzzzz = 3\n    print(zzzzz(1))\n",
            expected: Err(("E_NAME_UNKNOWN", 3, 11)),
        },
    ]
}

#[test]
fn function_value_cases_compile_and_run() {
    cases_compile_and_run(function_value_cases());
}
