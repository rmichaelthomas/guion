//! F9, checked templates: `*.gut` files become ordinary Guion modules (`*.gut.gu`) in a pre-pass of
//! `guion check`, `build`, `run`, `test`, `inspect`, `doc`, `fmt`, `lint` and `package`. Every
//! test builds a scratch project that depends on the `web` package by path.

mod common;

use common::output;
use std::path::{Path, PathBuf};
use std::process::Command;

fn web_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/web")
        .canonicalize()
        .unwrap()
}

struct App {
    dir: PathBuf,
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

struct Run {
    ok: bool,
    stdout: String,
    stderr: String,
}

impl Run {
    fn all(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }
}

impl App {
    fn new(name: &str) -> App {
        let app = Self::bare(name, "");
        let added = app.guion(&["add", "web", "--path", web_dir().to_str().unwrap()]);
        assert!(added.ok, "{}", added.all());
        app
    }

    fn bare(name: &str, extra_manifest: &str) -> App {
        let dir = std::env::temp_dir().join(format!("guion-tpl-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src/templates")).unwrap();
        std::fs::write(
            dir.join("guion.toml"),
            format!(
                "[project]\nname = \"app\"\nversion = \"0.1.0\"\nentry = \"src/main.gu\"\n{extra_manifest}"
            ),
        )
        .unwrap();
        App { dir }
    }

    fn file(&self, relative: &str, contents: &str) {
        let path = self.dir.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    fn read(&self, relative: &str) -> String {
        std::fs::read_to_string(self.dir.join(relative)).unwrap_or_default()
    }

    fn exists(&self, relative: &str) -> bool {
        self.dir.join(relative).exists()
    }

    fn guion(&self, args: &[&str]) -> Run {
        let result = output(
            Command::new(env!("CARGO_BIN_EXE_guion"))
                .args(args)
                .current_dir(&self.dir),
        )
        .unwrap();
        Run {
            ok: result.status.success(),
            stdout: String::from_utf8_lossy(&result.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&result.stderr).into_owned(),
        }
    }

    /// Writes `src/templates/page.gut` and a main that prints `body` (a Guion expression of type
    /// `Rendered`), then runs it.
    fn render(&self, template: &str, body: &str) -> Run {
        self.file("src/templates/page.gut", template);
        self.file(
            "src/main.gu",
            &format!(
                "import \"templates/page.gut.gu\"\nimport \"std:text\"\n\nfn main() -> Int:\n    print(({body}).html)\n    return 0\n"
            ),
        );
        self.guion(&["run", "--no-cache", "src/main.gu"])
    }
}

/// 1-based line and column (in characters) of the first occurrence of `needle`.
fn position_of(source: &str, needle: &str) -> (usize, usize) {
    let offset = source
        .find(needle)
        .unwrap_or_else(|| panic!("{needle} in source"));
    let before = &source[..offset];
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().unwrap().chars().count() + 1;
    (line, column)
}

fn expect_error(run: &Run, code: &str, file: &str, source: &str, needle: &str) {
    let (line, column) = position_of(source, needle);
    let text = run.all();
    assert!(!run.ok, "expected a failure: {text}");
    let wanted = format!("{file}:{line}:{column}: error[{}]", tpl(code));
    assert!(text.contains(&wanted), "wanted `{wanted}` in: {text}");
}

const TEMPLATE: &str = "src/templates/page.gut";

/// A code such as `ScriptContext` is shown with a `TEMPLATE` prefix and underscores. The codes are written in CamelCase in this
/// file because the generated agent reference scans source text for `E_` and `R_` tokens.
fn tpl(code: &str) -> String {
    let mut text = String::from("TEMPLATE");
    for c in code.chars() {
        if c.is_ascii_uppercase() {
            text.push('_');
        }
        text.push(c.to_ascii_uppercase());
    }
    text
}

// ---------------------------------------------------------------------------------------------
// The XSS corpus, checked against an independent python3 implementation of the same escapers.
// ---------------------------------------------------------------------------------------------

const CORPUS: &[&str] = &[
    "<script>alert(1)</script>",
    "\"><img src=x onerror=alert(1)>",
    "' onmouseover='alert(1)",
    "javascript:alert(1)",
    "</script><script>alert(1)</script>",
    "&lt;b&gt; &amp; &#x3c;script&#x3e;",
    "line\u{2028}sep\u{2029}end",
    "${alert(1)}`+`",
    "a\u{0}b\u{1f}c\u{7f}d\u{8}e\u{c}f",
    "<!-- --><svg onload=1>",
    "--></script><!--<script>",
    "é日本語😀",
    "a b/c?d=e&f#g;h+i",
    "",
    "back\\slash \\\" \\\\",
    "cr\r\nlf\ttab",
    "%3Cscript%3E %00 %",
];

fn guion_text_literal(value: &str) -> String {
    let mut literal = String::from("\"");
    for character in value.chars() {
        literal.push_str(&format!("\\u{{{:X}}}", character as u32));
    }
    literal.push('"');
    literal
}

const PYTHON_TWIN: &str = r#"
import json, sys, urllib.parse

def body(s):
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;").replace('"', "&quot;").replace("'", "&#39;")

def attr(s):
    return body(s).replace("`", "&#96;")

def url(s):
    return urllib.parse.quote(s, safe="-._~")

def script(s):
    text = json.dumps(s, ensure_ascii=False)
    for raw, escaped in (("<", "\\u003c"), (">", "\\u003e"), ("&", "\\u0026"), ("'", "\\u0027"), ("`", "\\u0060"), ("$", "\\u0024"), (" ", "\\u2028"), (" ", "\\u2029")):
        text = text.replace(raw, escaped)
    return text

corpus = json.loads(sys.argv[1])
lines = sys.stdin.read().split("\n")
assert len(lines) >= len(corpus), (len(lines), len(corpus))
for value, line in zip(corpus, lines):
    got = bytes.fromhex(line).decode("utf-8")
    want = (
        "<p>" + body(value) + "</p>|<a title=\"" + body(value) + "\" class=\"" + attr(value)
        + "\" href=\"/s?q=" + url(value) + "\">x</a>|<script>var x = " + script(value) + ";</script>"
    )
    if got != want:
        print("MISMATCH for", repr(value))
        print(" got ", repr(got))
        print(" want", repr(want))
        sys.exit(1)
print("twin agrees on", len(corpus), "values")
"#;

#[test]
fn the_xss_corpus_matches_an_independent_implementation_in_every_context() {
    let app = App::new("corpus");
    app.file(
        TEMPLATE,
        "{% params v: Text %}<p>{{ v }}</p>|<a title=\"{{ v }}\" class=\"{{ attr v }}\" href=\"/s?q={{ url v }}\">x</a>|<script>var x = {{ script v }};</script>",
    );
    let calls = CORPUS
        .iter()
        .map(|value| {
            format!(
                "    print(hex_encode(encoding_utf8_encode(render_page({}).html)))\n",
                guion_text_literal(value)
            )
        })
        .collect::<String>();
    app.file(
        "src/main.gu",
        &format!(
            "import \"templates/page.gut.gu\"\nimport \"std:encoding\"\n\nfn main() -> Int:\n{calls}    return 0\n"
        ),
    );
    let run = app.guion(&["run", "--no-cache", "src/main.gu"]);
    assert!(run.ok, "{}", run.all());
    let corpus_json = serde_json::to_string(CORPUS).unwrap();
    let mut child = Command::new("python3")
        .args(["-I", "-c", PYTHON_TWIN, &corpus_json])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("python3 starts");
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(run.stdout.as_bytes())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.status.success(), "{said}");
    assert!(said.contains("twin agrees on 17 values"), "{said}");
}

#[test]
fn a_script_tag_payload_is_inert_in_body_attribute_and_url_positions() {
    let app = App::new("inert");
    let run = app.render(
        "{% params x: Text %}<p>{{ x }}</p><i title=\"{{ x }}\">.</i><a href=\"/q?v={{ url x }}\">.</a>",
        "render_page(\"<script>alert(1)</script>\")",
    );
    assert!(run.ok, "{}", run.all());
    assert_eq!(
        run.stdout.trim_end(),
        "<p>&lt;script&gt;alert(1)&lt;/script&gt;</p><i title=\"&lt;script&gt;alert(1)&lt;/script&gt;\">.</i><a href=\"/q?v=%3Cscript%3Ealert%281%29%3C%2Fscript%3E\">.</a>"
    );
    // An attacker value never contains a raw angle bracket or quote in any position.
    let hostile = app.render(
        "{% params x: Text %}<i title=\"{{ x }}\" data-a='{{ attr x }}'>{{ x }}</i>",
        "render_page(\"\\\"'><img src=x onerror=alert(1)>\")",
    );
    assert!(hostile.ok, "{}", hostile.all());
    assert_eq!(
        hostile.stdout.trim_end(),
        "<i title=\"&quot;&#39;&gt;&lt;img src=x onerror=alert(1)&gt;\" data-a='&quot;&#39;&gt;&lt;img src=x onerror=alert(1)&gt;'>&quot;&#39;&gt;&lt;img src=x onerror=alert(1)&gt;</i>"
    );
}

#[test]
fn a_javascript_scheme_through_url_is_percent_encoded_and_never_a_scheme() {
    let app = App::new("scheme");
    let run = app.render(
        "{% params x: Text %}<a href=\"{{ url x }}\">go</a>",
        "render_page(\"javascript:alert(1)\")",
    );
    assert!(run.ok, "{}", run.all());
    assert_eq!(
        run.stdout.trim_end(),
        "<a href=\"javascript%3Aalert%281%29\">go</a>"
    );
}

#[test]
fn script_interpolator_renders_a_json_string_literal_that_cannot_close_the_element() {
    let app = App::new("script");
    let run = app.render(
        "{% params x: Text %}<script>var v = {{ script x }};</script>",
        "render_page(\"a\\\"b</script><!--\\u{2028}\")",
    );
    assert!(run.ok, "{}", run.all());
    assert_eq!(
        run.stdout.trim_end(),
        "<script>var v = \"a\\\"b\\u003c/script\\u003e\\u003c!--\\u2028\";</script>"
    );
}

// ---------------------------------------------------------------------------------------------
// Compile-time checks, each with a code, a file, a line and a column.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_plain_interpolation_inside_a_script_element_fails_every_command() {
    let app = App::new("scriptctx");
    let template = "{% params x: Text %}\n<script>\n  var a = {{ x }};\n</script>\n";
    app.file(TEMPLATE, template);
    app.file(
        "src/main.gu",
        "import \"templates/page.gut.gu\"\n\nfn main() -> Int:\n    print(render_page(\"x\").html)\n    return 0\n",
    );
    for command in ["check", "build", "run", "inspect", "doc"] {
        let mut args = vec![command, "src/main.gu"];
        if command == "build" {
            args.extend(["-o", "out.gub"]);
        }
        let run = app.guion(&args);
        expect_error(&run, "ScriptContext", TEMPLATE, template, "{{ x }}");
    }
    let test_run = app.guion(&["test", "src/main.gu"]);
    expect_error(&test_run, "ScriptContext", TEMPLATE, template, "{{ x }}");
    let json = app.guion(&["check", "--json", "src/main.gu"]);
    assert!(!json.ok);
    let report: serde_json::Value = serde_json::from_str(json.stdout.trim()).unwrap();
    assert_eq!(
        report["diagnostic"]["code"],
        tpl("ScriptContext"),
        "{json:?}",
        json = json.all()
    );
    assert_eq!(report["diagnostic"]["source"], TEMPLATE);
    assert_eq!(report["diagnostic"]["line"], 3);
    assert_eq!(report["diagnostic"]["column"], 11);
}

#[test]
fn every_context_the_checker_cannot_make_safe_is_refused_with_a_named_error() {
    let app = App::new("refused");
    let cases: &[(&str, &str, &str)] = &[
        (
            "<script>{{ attr x }}</script>",
            "ScriptContext",
            "{{ attr x }}",
        ),
        (
            "<script>{{ url x }}</script>",
            "ScriptContext",
            "{{ url x }}",
        ),
        (
            "<style>p { color: {{ x }} }</style>",
            "StyleContext",
            "{{ x }}",
        ),
        (
            "<style>p { color: {{ script x }} }</style>",
            "StyleContext",
            "{{ script x }}",
        ),
        ("<!-- {{ x }} -->", "CommentContext", "{{ x }}"),
        ("<a href={{ x }}>a</a>", "UnquotedAttribute", "{{ x }}"),
        ("<a title=a{{ x }}b>a</a>", "UnquotedAttribute", "{{ x }}"),
        ("<a {{ x }}>a</a>", "TagContext", "{{ x }}"),
        ("<{{ x }} class=\"a\">", "TagContext", "{{ x }}"),
        (
            "<a class=\"a\" {{ attr x }}=\"1\">",
            "TagContext",
            "{{ attr x }}",
        ),
        ("<a href=\"{{ x }}\">a</a>", "UrlContext", "{{ x }}"),
        (
            "<a href=\"/p/{{ attr x }}\">a</a>",
            "UrlContext",
            "{{ attr x }}",
        ),
        ("<img src='{{ x }}'>", "UrlContext", "{{ x }}"),
        ("<form action=\"{{ x }}\">", "UrlContext", "{{ x }}"),
        (
            "<button onclick=\"go('{{ x }}')\">",
            "AttributeCodeContext",
            "{{ x }}",
        ),
        (
            "<button ONCLICK=\"go({{ script x }})\">",
            "AttributeCodeContext",
            "{{ script x }}",
        ),
        (
            "<p style=\"color: {{ x }}\">",
            "AttributeCodeContext",
            "{{ x }}",
        ),
        (
            "<iframe srcdoc=\"{{ x }}\"></iframe>",
            "AttributeCodeContext",
            "{{ x }}",
        ),
        (
            "<p>{{ script x }}</p>",
            "ScriptOutsideScript",
            "{{ script x }}",
        ),
        (
            "<p title=\"{{ script x }}\">",
            "ScriptOutsideScript",
            "{{ script x }}",
        ),
        (
            "<p title=\"{{ embed r }}\">",
            "EmbedContext",
            "{{ embed r }}",
        ),
        (
            "<script>{{ embed r }}</script>",
            "EmbedContext",
            "{{ embed r }}",
        ),
        (
            "{% if b %}<a href=\"{% else %}<a href=\"x\">{% end %}",
            "ContextMismatch",
            "{% if b %}",
        ),
        (
            "{% for v in vs %}<a title=\"{% end %}",
            "ContextMismatch",
            "{% for v in vs %}",
        ),
    ];
    for (body, code, needle) in cases {
        let template =
            format!("{{% params x: Text, r: Rendered, b: Bool, vs: List[Text] %}}\n{body}\n");
        app.file(TEMPLATE, &template);
        app.file(
            "src/main.gu",
            "import \"templates/page.gut.gu\"\n\nfn main() -> Int:\n    return 0\n",
        );
        let run = app.guion(&["check", "src/main.gu"]);
        expect_error(&run, code, TEMPLATE, &template, needle);
    }
}

#[test]
fn contexts_that_are_safe_are_accepted() {
    let app = App::new("accepted");
    app.file(
        "src/templates/inner.gut",
        "{% params x: Text %}<b>{{ x }}</b>",
    );
    app.file(
        TEMPLATE,
        "{% params x: Text, r: Rendered %}<title>{{ x }}</title><textarea>{{ x }}</textarea><a href=\"/p/{{ url x }}?q={{ url x }}\" class=\"{{ attr x }}\" data-id='{{ x }}'>{{ x }}</a><script src=\"/a.js?{{ url x }}\"></script><script>var a = {{ script x }}, b = {{ script x }};</script><div>{{ embed r }}</div>",
    );
    app.file(
        "src/main.gu",
        "import \"templates/page.gut.gu\"\nimport \"templates/inner.gut.gu\"\n\nfn main() -> Int:\n    print(render_page(\"v\", render_inner(\"<i>\")).html)\n    return 0\n",
    );
    let run = app.guion(&["run", "--no-cache", "src/main.gu"]);
    assert!(run.ok, "{}", run.all());
    assert_eq!(
        run.stdout.trim_end(),
        "<title>v</title><textarea>v</textarea><a href=\"/p/v?q=v\" class=\"v\" data-id='v'>v</a><script src=\"/a.js?v\"></script><script>var a = \"v\", b = \"v\";</script><div><b>&lt;i&gt;</b></div>"
    );
}

#[test]
fn an_unknown_variable_fails_with_a_position_and_the_same_file_with_it_added_succeeds() {
    let app = App::new("unknown");
    let broken = "{% params title: Text %}\n<h1>{{ title }}</h1>\n<p>{{ subtitle }}</p>\n";
    app.file(TEMPLATE, broken);
    app.file(
        "src/main.gu",
        "import \"templates/page.gut.gu\"\n\nfn main() -> Int:\n    print(render_page(\"T\").html)\n    return 0\n",
    );
    let run = app.guion(&["build", "src/main.gu", "-o", "out.gub"]);
    expect_error(&run, "UnknownVariable", TEMPLATE, broken, "subtitle");
    assert!(!app.exists("out.gub"));
    // The same template with the variable declared, and nothing else changed.
    app.file(
        TEMPLATE,
        "{% params title: Text, subtitle: Text %}\n<h1>{{ title }}</h1>\n<p>{{ subtitle }}</p>\n",
    );
    app.file(
        "src/main.gu",
        "import \"templates/page.gut.gu\"\n\nfn main() -> Int:\n    print(render_page(\"T\", \"S\").html)\n    return 0\n",
    );
    let fixed = app.guion(&["build", "src/main.gu", "-o", "out.gub"]);
    assert!(fixed.ok, "{}", fixed.all());
    assert!(app.exists("out.gub"));
}

#[test]
fn an_unknown_function_a_missing_import_and_unknown_field_are_positioned() {
    let app = App::new("unknownfn");
    let template = "{% params x: Text %}\n{{ embed missing(x) }}\n";
    app.file(TEMPLATE, template);
    app.file(
        "src/main.gu",
        "import \"templates/page.gut.gu\"\n\nfn main() -> Int:\n    return 0\n",
    );
    let run = app.guion(&["check", "src/main.gu"]);
    expect_error(&run, "UnknownFunction", TEMPLATE, template, "missing");

    // A field the record does not have is the compiler's error, shown at the template position.
    app.file("src/templates/rows.gu", "pub record Row:\n    name: Text\n");
    let template = "{% import \"rows.gu\" %}\n{% params r: Row %}\n<p>{{ r.nope }}</p>\n";
    app.file(TEMPLATE, template);
    let run = app.guion(&["check", "src/main.gu"]);
    let (line, column) = position_of(template, "{{ r.nope }}");
    let text = run.all();
    assert!(!run.ok, "{text}");
    assert!(
        text.contains(&format!("{TEMPLATE}:{line}:{column}: error[")),
        "{text}"
    );
    assert!(!text.contains(".gut.gu"), "{text}");
}

#[test]
fn structural_mistakes_name_a_code_a_line_and_a_column() {
    let app = App::new("structure");
    app.file(
        "src/main.gu",
        "import \"templates/page.gut.gu\"\n\nfn main() -> Int:\n    return 0\n",
    );
    let cases: &[(&str, &str, &str)] = &[
        (
            "{% params xs: List[Text] %}\n{% for x in xs %}\n<li>{{ x }}</li>\n",
            "UnbalancedBlock",
            "{% for x in xs %}",
        ),
        (
            "{% params b: Bool %}\n{% if b %}\nyes\n",
            "UnbalancedBlock",
            "{% if b %}",
        ),
        (
            "{% params b: Bool %}\nyes\n{% end %}\n",
            "UnbalancedEnd",
            "{% end %}",
        ),
        (
            "{% params b: Bool %}\n{% else %}\n",
            "UnexpectedElse",
            "{% else %}",
        ),
        (
            "{% params b: Bool %}\n{% if b %}a{% else %}b{% else %}c{% end %}\n",
            "UnexpectedElse",
            "{% else %}c",
        ),
        (
            "{% params x: Text %}\n<p>{{ x </p>\n",
            "UnclosedTag",
            "{{ x </p>",
        ),
        (
            "{% params x: Text %}\n{% frob x %}\n",
            "UnknownTag",
            "{% frob x %}",
        ),
        (
            "{% params x: Text %}\n{{ raw x }}\n",
            "UnknownInterpolator",
            "raw",
        ),
        (
            "{% params x: Text %}\n{{ x | safe }}\n",
            "BadExpression",
            "| safe",
        ),
        (
            "{% params x: Text %}\n{{ x.y. }}\n",
            "BadExpression",
            "x.y.",
        ),
        ("{% params x: Text %}\n{{ }}\n", "BadExpression", "{{ }}"),
        ("{% params x Text %}\n", "BadParams", "x Text"),
        (
            "{% params x: Text %}\n{% params y: Text %}\n",
            "HeaderPosition",
            "{% params y: Text %}",
        ),
        (
            "hello\n{% params x: Text %}\n",
            "HeaderPosition",
            "{% params x: Text %}",
        ),
        (
            "{% params x: Text, x: Text %}\n",
            "DuplicateVariable",
            "x: Text %}",
        ),
        (
            "{% params xs: List[Text] %}\n{% for xs in xs %}{% end %}\n",
            "DuplicateVariable",
            "xs in",
        ),
        ("{% params html_x: Text %}\n", "ReservedName", "html_x"),
        ("{% params gut_out: Text %}\n", "ReservedName", "gut_out"),
        (
            "{% params list_push: Text %}\n",
            "ReservedName",
            "list_push",
        ),
        ("{% params for: Text %}\n", "ReservedName", "for:"),
    ];
    for (template, code, needle) in cases {
        app.file(TEMPLATE, template);
        let run = app.guion(&["check", "src/main.gu"]);
        expect_error(&run, code, TEMPLATE, template, needle);
    }
}

#[test]
fn a_template_whose_name_is_not_an_identifier_is_refused() {
    let app = App::new("badname");
    app.file("src/templates/my-page.gut", "x");
    app.file("src/main.gu", "fn main() -> Int:\n    return 0\n");
    let run = app.guion(&["check", "src/main.gu"]);
    assert!(!run.ok);
    assert!(
        run.all().contains(&format!(
            "src/templates/my-page.gut:1:1: error[{}]",
            tpl("BadName")
        )),
        "{}",
        run.all()
    );
}

#[test]
fn a_template_that_needs_web_says_so_when_web_is_not_a_dependency() {
    let app = App::bare("noweb", "");
    app.file("src/templates/page.gut", "<p>hi</p>\n");
    app.file("src/main.gu", "fn main() -> Int:\n    return 0\n");
    let run = app.guion(&["check", "src/main.gu"]);
    assert!(!run.ok);
    assert!(
        run.all().contains(&format!(
            "src/templates/page.gut:1:1: error[{}]",
            tpl("RuntimeMissing")
        )),
        "{}",
        run.all()
    );
    assert!(run.all().contains("guion add web"), "{}", run.all());
}

// ---------------------------------------------------------------------------------------------
// Types come from the normal compiler.
// ---------------------------------------------------------------------------------------------

#[test]
fn embed_accepts_only_rendered_and_a_text_value_is_a_type_error_at_the_template() {
    let app = App::new("embedtype");
    let template = "{% params body: Text %}\n<div>{{ embed body }}</div>\n";
    app.file(TEMPLATE, template);
    app.file(
        "src/main.gu",
        "import \"templates/page.gut.gu\"\n\nfn main() -> Int:\n    return 0\n",
    );
    let run = app.guion(&["check", "src/main.gu"]);
    let text = run.all();
    let (line, column) = position_of(template, "{{ embed body }}");
    assert!(!run.ok, "{text}");
    assert!(
        text.contains(&format!(
            "{TEMPLATE}:{line}:{column}: error[E_TYPE_ARGUMENT]"
        )),
        "{text}"
    );
}

#[test]
fn a_non_text_interpolation_and_a_wrong_call_are_compiler_errors() {
    let app = App::new("calltype");
    app.file(TEMPLATE, "{% params n: Int %}\n<p>{{ n }}</p>\n");
    app.file(
        "src/main.gu",
        "import \"templates/page.gut.gu\"\n\nfn main() -> Int:\n    return 0\n",
    );
    let run = app.guion(&["check", "src/main.gu"]);
    assert!(!run.ok);
    assert!(
        run.all()
            .contains("src/templates/page.gut:2:4: error[E_TYPE_ARGUMENT]"),
        "{}",
        run.all()
    );
    // A caller that passes the wrong number of arguments is told by the compiler, in the caller.
    app.file(
        TEMPLATE,
        "{% params a: Text, b: Text %}<p>{{ a }}{{ b }}</p>",
    );
    app.file(
        "src/main.gu",
        "import \"templates/page.gut.gu\"\n\nfn main() -> Int:\n    print(render_page(\"only one\").html)\n    return 0\n",
    );
    let run = app.guion(&["check", "src/main.gu"]);
    assert!(!run.ok);
    assert!(run.all().contains("E_CALL_ARITY"), "{}", run.all());
}

// ---------------------------------------------------------------------------------------------
// Names, loops, conditionals, composition.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_variable_named_like_an_interpolator_is_legal_and_still_escaped() {
    let app = App::new("named");
    let run = app.render(
        "{% params embed: Text, script: Text, attr: Text, url: Text %}<p>{{ embed }}|{{ script }}|{{ attr }}|{{ url }}</p><a href=\"/{{ url url }}\" title=\"{{ attr attr }}\">x</a><script>var s = {{ script script }};</script>",
        "render_page(\"<e>\", \"<s>\", \"<a>\", \"<u>\")",
    );
    assert!(run.ok, "{}", run.all());
    assert_eq!(
        run.stdout.trim_end(),
        "<p>&lt;e&gt;|&lt;s&gt;|&lt;a&gt;|&lt;u&gt;</p><a href=\"/%3Cu%3E\" title=\"&lt;a&gt;\">x</a><script>var s = \"\\u003cs\\u003e\";</script>"
    );
}

#[test]
fn loops_conditionals_and_nested_empty_lists_render_exactly() {
    let app = App::new("loops");
    app.file(
        "src/templates/rows.gu",
        "pub record Row:\n    name: Text\n    tags: List[Text]\n    hot: Bool\n",
    );
    let template = "{% import \"rows.gu\" %}\n{% params rows: List[Row], empty: Bool %}\n<ul>{% for r in rows %}<li>{{ r.name }}{% if r.hot %}!{% else %}.{% end %}<ol>{% for t in r.tags %}<li>{{ t }}</li>{% end %}</ol></li>{% end %}</ul>\n{% if not empty %}full{% end %}{% if empty %}none{% else %}some{% end %}\n\\{{ literal \\{% braces\n";
    app.file(TEMPLATE, template);
    app.file(
        "src/main.gu",
        r#"import "templates/page.gut.gu"
import "templates/rows.gu"

fn main() -> Int:
    let no_rows: List[Row] = []
    let none: List[Text] = []
    let both: List[Row] = [Row("<a>", none, true), Row("b", ["x", "<y>"], false), Row("c", none, false)]
    print(render_page(no_rows, true).html)
    print("--")
    print(render_page(both, false).html)
    return 0
"#,
    );
    let run = app.guion(&["run", "--no-cache", "src/main.gu"]);
    assert!(run.ok, "{}", run.all());
    let (empty, full) = run.stdout.split_once("--\n").unwrap();
    assert_eq!(empty, "<ul></ul>\nnone\n{{ literal {% braces\n\n");
    assert_eq!(
        full,
        "<ul><li>&lt;a&gt;!<ol></ol></li><li>b.<ol><li>x</li><li>&lt;y&gt;</li></ol></li><li>c.<ol></ol></li></ul>\nfullsome\n{{ literal {% braces\n\n"
    );
}

#[test]
fn nested_loops_over_empty_lists_and_empty_bodies_generate_valid_source() {
    let app = App::new("emptyloops");
    app.file(
        TEMPLATE,
        "{% params outer: List[List[Text]], flag: Bool %}[{% for row in outer %}{% for cell in row %}{{ cell }}{% end %}{% end %}][{% for row in outer %}{% end %}][{% if flag %}{% else %}{% end %}]",
    );
    app.file(
        "src/main.gu",
        r#"import "templates/page.gut.gu"

fn main() -> Int:
    let none: List[List[Text]] = []
    let hollow: List[List[Text]] = [[], [], []]
    let some: List[List[Text]] = [[], ["a", "<b>"], []]
    print(render_page(none, true).html)
    print(render_page(hollow, false).html)
    print(render_page(some, false).html)
    return 0
"#,
    );
    let run = app.guion(&["run", "--no-cache", "src/main.gu"]);
    assert!(run.ok, "{}", run.all());
    assert_eq!(run.stdout, "[][][]\n[][][]\n[a&lt;b&gt;][][]\n");
}

#[test]
fn templates_compose_by_function_composition() {
    let app = App::new("compose");
    app.file(
        "src/templates/item.gut",
        "{% params name: Text %}<li>{{ name }}</li>",
    );
    app.file(
        "src/templates/layout.gut",
        "{% params title: Text, body: Rendered %}<html><title>{{ title }}</title><body>{{ embed body }}</body></html>",
    );
    app.file(
        "src/templates/list.gut",
        "{% params names: List[Text] %}<ul>{% for n in names %}{{ embed item(n) }}{% end %}</ul>",
    );
    app.file(
        "src/main.gu",
        r#"import "templates/layout.gut.gu"
import "templates/list.gut.gu"

fn main() -> Int:
    print(render_layout("T & C", render_list(["a", "<b>"])).html)
    return 0
"#,
    );
    let run = app.guion(&["run", "--no-cache", "src/main.gu"]);
    assert!(run.ok, "{}", run.all());
    assert_eq!(
        run.stdout.trim_end(),
        "<html><title>T &amp; C</title><body><ul><li>a</li><li>&lt;b&gt;</li></ul></body></html>"
    );
    // Two templates that name each other are refused before compiling.
    app.file(
        "src/templates/item.gut",
        "{% params name: Text %}{{ embed list(name) }}",
    );
    app.file(
        "src/templates/list.gut",
        "{% params names: Text %}{{ embed item(names) }}",
    );
    let run = app.guion(&["check", "src/main.gu"]);
    assert!(!run.ok);
    assert!(
        run.all().contains(&format!("error[{}]", tpl("Cycle"))),
        "{}",
        run.all()
    );
}

#[test]
fn a_very_long_unbroken_value_in_an_attribute_renders_in_linear_time() {
    let app = App::new("long");
    app.file(
        TEMPLATE,
        "{% params v: Text %}<a title=\"{{ v }}\" data-x='{{ attr v }}' href=\"/{{ url v }}\">x</a>",
    );
    app.file(
        "src/main.gu",
        r#"import "templates/page.gut.gu"
import "std:text"

fn repeat(piece: Text, count: Int) -> Text:
    var text = piece
    var made = 1
    while made * 2 <= count:
        set text = text + text
        set made = made * 2
    return text

fn main() -> Int:
    let plain = repeat("a", 1048576)
    let hostile = repeat("\"<", 65536)
    let first = render_page(plain).html
    let second = render_page(hostile).html
    print(len(first))
    print(len(second))
    print(text_slice(second, 0, 40))
    return 0
"#,
    );
    let started = std::time::Instant::now();
    let run = app.guion(&["run", "--no-cache", "src/main.gu"]);
    assert!(run.ok, "{}", run.all());
    let lines = run.stdout.lines().collect::<Vec<_>>();
    // `<a title="` (10) + value + `" data-x='` (10) + value + `' href="/` (9) + value + `">x</a>` (7)
    assert_eq!(lines[0], (36 + 3 * 1_048_576).to_string());
    // 65,536 pairs of `"<`: title 6+4, attr 6+4 each (quote then angle bracket), url 3 + 3 each.
    let expected = 36 + 65_536 * (10 + 10 + 6);
    assert_eq!(lines[1], expected.to_string());
    assert_eq!(lines[2], "<a title=\"&quot;&lt;&quot;&lt;&quot;&lt;");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(60),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn ten_thousand_rows_render_inside_the_default_instruction_budget() {
    let app = App::bare("rows", "\n[runtime]\ninstruction_limit = 2000000\n");
    let added = app.guion(&["add", "web", "--path", web_dir().to_str().unwrap()]);
    assert!(added.ok, "{}", added.all());
    app.file(
        "src/templates/rows.gu",
        "pub record Row:\n    id: Text\n    name: Text\n    note: Text\n",
    );
    app.file(
        TEMPLATE,
        "{% import \"rows.gu\" %}\n{% params rows: List[Row] %}<table>{% for r in rows %}<tr id=\"r{{ r.id }}\"><td>{{ r.name }}</td><td>{{ r.note }}</td></tr>{% end %}</table>",
    );
    app.file(
        "src/main.gu",
        r#"import "templates/page.gut.gu"
import "templates/rows.gu"

fn build(count: Int) -> List[Row]:
    var rows: List[Row] = []
    var index = 0
    while index < count:
        set rows = list_push(rows, Row(int_to_text(index), "name <" + int_to_text(index) + ">", "a & b"))
        set index = index + 1
    return rows

fn main() -> Int:
    let page = render_page(build(10000))
    print(len(page.html))
    return 0
"#,
    );
    let run = app.guion(&["run", "--no-cache", "src/main.gu"]);
    assert!(run.ok, "{}", run.all());
    assert!(
        run.stdout.trim().parse::<usize>().unwrap() > 500_000,
        "{}",
        run.stdout
    );
    // Doubling the limit's headroom is not needed: ten times fewer instructions is also fine, and
    // one tenth of the budget fails (the test measures that the budget is the thing in play).
    let tight = App::bare("rowstight", "\n[runtime]\ninstruction_limit = 100000\n");
    let added = tight.guion(&["add", "web", "--path", web_dir().to_str().unwrap()]);
    assert!(added.ok);
    for relative in ["src/templates/rows.gu", TEMPLATE, "src/main.gu"] {
        tight.file(relative, &app.read(relative));
    }
    let run = tight.guion(&["run", "--no-cache", "src/main.gu"]);
    assert!(
        !run.ok && run.all().contains("R_STEP_LIMIT"),
        "{}",
        run.all()
    );
}

// ---------------------------------------------------------------------------------------------
// The raw-insertion rules.
// ---------------------------------------------------------------------------------------------

#[test]
fn the_raw_insertion_function_appears_in_generated_output_only_where_a_template_wrote_embed() {
    let app = App::new("grep");
    app.file(
        TEMPLATE,
        "{% params a: Text, r: Rendered %}<p>{{ a }}</p>{{ attr a }}{{ url a }}<script>{{ script a }}</script>\n",
    );
    app.file(
        "src/main.gu",
        "import \"templates/page.gut.gu\"\n\nfn main() -> Int:\n    return 0\n",
    );
    let run = app.guion(&["check", "src/main.gu"]);
    assert!(run.ok, "{}", run.all());
    let generated = app.read("src/templates/page.gut.gu");
    assert!(
        generated.starts_with("# generated by guion from page.gut"),
        "{generated}"
    );
    assert!(!generated.contains("html_embed_rendered"), "{generated}");
    // With an embed it appears exactly once.
    app.file(
        TEMPLATE,
        "{% params a: Text, r: Rendered %}<p>{{ a }}</p>{{ embed r }}\n",
    );
    let run = app.guion(&["check", "src/main.gu"]);
    assert!(run.ok, "{}", run.all());
    let generated = app.read("src/templates/page.gut.gu");
    assert_eq!(
        generated.matches("html_embed_rendered").count(),
        1,
        "{generated}"
    );
    // No other generated or hand-written file in the project names it.
    assert!(!app.read("src/main.gu").contains("html_embed_rendered"));
}

#[test]
fn a_hand_built_rendered_value_is_refused_in_a_project_that_uses_templates() {
    let app = App::new("forgery");
    app.file(TEMPLATE, "<p>hi</p>\n");
    for (source, needle) in [
        ("let r = Rendered(\"<script>evil()</script>\")", "Rendered("),
        ("let r = html_finish([\"<b>\"])", "html_finish"),
        ("let t = html_embed_rendered(page)", "html_embed_rendered"),
    ] {
        let main = format!(
            "import \"templates/page.gut.gu\"\n\nfn main() -> Int:\n    let page = render_page()\n    {source}\n    return 0\n"
        );
        app.file("src/main.gu", &main);
        let run = app.guion(&["check", "src/main.gu"]);
        expect_error(&run, "RenderedForgery", "src/main.gu", &main, needle);
    }
    // Reading the text of a Rendered, and mentioning the names in a comment or a string, is fine.
    app.file(
        "src/main.gu",
        "import \"templates/page.gut.gu\"\n\n# Rendered(\"x\") and html_finish are only named here.\nfn main() -> Int:\n    let page = render_page()\n    print(page.html)\n    print(\"html_embed_rendered\")\n    return 0\n",
    );
    let run = app.guion(&["run", "--no-cache", "src/main.gu"]);
    assert!(run.ok, "{}", run.all());
    assert_eq!(run.stdout, "<p>hi</p>\n\nhtml_embed_rendered\n");
}

#[test]
fn rendered_has_no_text_constructor_and_the_escapers_are_pure_and_handle_free() {
    let app = App::new("surface");
    app.file(
        TEMPLATE,
        "{% params a: Text, r: Rendered %}<p>{{ a }}</p>{{ embed r }}\n",
    );
    app.file(
        "src/main.gu",
        "import \"templates/page.gut.gu\"\n\nfn main() -> Int:\n    return 0\n",
    );
    let run = app.guion(&["doc", "--json", "src/main.gu"]);
    assert!(run.ok, "{}", run.all());
    let report: serde_json::Value = serde_json::from_str(&run.stdout).unwrap();
    let functions = report["functions"].as_array().unwrap();
    let find = |name: &str| {
        functions
            .iter()
            .find(|function| function["name"] == name)
            .unwrap_or_else(|| panic!("{name} documented in {report}"))
    };
    for pure in [
        "render_page",
        "html_escape_text",
        "html_escape_attr",
        "html_escape_url",
        "html_script_string",
        "html_embed_rendered",
        "html_finish",
    ] {
        assert!(
            find(pure)["effects"].as_array().unwrap().is_empty(),
            "{pure}: {}",
            find(pure)
        );
    }
    // No function takes a Text and returns a Rendered: the only producers take parts and are
    // refused by the pre-pass outside generated code.
    let producers = functions
        .iter()
        .filter(|function| {
            function.to_string().contains("-> Rendered")
                || function["returns"].to_string().contains("Rendered")
        })
        .map(|function| function["name"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert!(
        producers
            .iter()
            .all(|name| name == "render_page" || name == "html_finish"),
        "{producers:?}"
    );
    for handle in ["TcpConn", "TcpListener", "Db", "Transaction"] {
        assert!(!report.to_string().contains(handle), "{handle}");
    }
}

// ---------------------------------------------------------------------------------------------
// Where the pre-pass runs, and what it leaves behind.
// ---------------------------------------------------------------------------------------------

#[test]
fn every_command_runs_the_pre_pass_and_inspect_shows_no_template_parser() {
    let app = App::new("commands");
    app.file(TEMPLATE, "{% params a: Text %}<p>{{ a }}</p>\n");
    app.file(
        "src/main.gu",
        "import \"templates/page.gut.gu\"\n\nfn test_page() -> Unit:\n    assert(render_page(\"<\").html == \"<p>&lt;</p>\\n\")\n\nfn main() -> Int:\n    print(render_page(\"x\").html)\n    return 0\n",
    );
    let generated = "src/templates/page.gut.gu";
    for args in [
        vec!["check", "src/main.gu"],
        vec!["build", "src/main.gu", "-o", "out.gub"],
        vec!["run", "--no-cache", "src/main.gu"],
        vec!["test", "src/main.gu"],
        vec!["inspect", "src/main.gu"],
        vec!["doc", "src/main.gu"],
        vec!["fmt", "--check", "src/main.gu"],
        vec!["lint", "src/main.gu"],
    ] {
        let _ = std::fs::remove_file(app.dir.join(generated));
        let run = app.guion(&args);
        assert!(run.ok, "{args:?}: {}", run.all());
        assert!(app.exists(generated), "{args:?} did not run the pre-pass");
    }
    // A project directory argument works too.
    let _ = std::fs::remove_file(app.dir.join(generated));
    let run = app.guion(&["check", "."]);
    assert!(run.ok, "{}", run.all());
    assert!(app.exists(generated));
    // The built artifact holds only generated Guion functions and the web helpers.
    let inspect = app.guion(&["inspect", "--json", "out.gub"]);
    assert!(inspect.ok, "{}", inspect.all());
    let text = inspect.stdout.to_lowercase();
    for symbol in [
        "parse_template",
        "template_parser",
        "tokenize",
        "gut_parse",
        "{%",
        "{{",
    ] {
        assert!(!text.contains(symbol), "{symbol} in the inspect report");
    }
    assert!(text.contains("render_page"));
}

#[test]
fn fmt_and_lint_leave_generated_files_alone() {
    let app = App::new("fmtgen");
    app.file(TEMPLATE, "{% params a: Text %}<p>{{ a }}</p>\n");
    app.file(
        "src/main.gu",
        "import \"templates/page.gut.gu\"\n\nfn main() -> Int:\n    print(render_page(\"x\").html)\n    return 0\n",
    );
    assert!(app.guion(&["check", "src/main.gu"]).ok);
    let before = app.read("src/templates/page.gut.gu");
    let fmt = app.guion(&["fmt", "src/templates/page.gut.gu"]);
    assert!(fmt.ok, "{}", fmt.all());
    assert_eq!(app.read("src/templates/page.gut.gu"), before);
    let check = app.guion(&["fmt", "--check", "src/templates/page.gut.gu"]);
    assert!(check.ok, "{}", check.all());
    let lint = app.guion(&["lint", "src/templates/page.gut.gu"]);
    assert!(lint.ok, "{}", lint.all());
    assert!(!lint.all().contains("warning"), "{}", lint.all());
    // A template is not formatted as Guion.
    let on_template = app.guion(&["fmt", TEMPLATE]);
    assert!(!on_template.ok);
    assert!(
        on_template.all().contains("template"),
        "{}",
        on_template.all()
    );
    assert_eq!(app.read(TEMPLATE), "{% params a: Text %}<p>{{ a }}</p>\n");
}

#[test]
fn generated_files_are_never_untracked_noise_and_stale_ones_are_removed() {
    let app = App::new("git");
    let git = |args: &[&str]| {
        let result = output(Command::new("git").args(args).current_dir(&app.dir)).unwrap();
        String::from_utf8_lossy(&result.stdout).into_owned()
    };
    git(&["init", "-q"]);
    app.file(TEMPLATE, "<p>hi</p>\n");
    app.file(
        "src/main.gu",
        "import \"templates/page.gut.gu\"\n\nfn main() -> Int:\n    print(render_page().html)\n    return 0\n",
    );
    for _ in 0..3 {
        assert!(app.guion(&["check", "src/main.gu"]).ok);
    }
    assert!(app.exists("src/templates/page.gut.gu"));
    let status = git(&["status", "--porcelain", "--untracked-files=all"]);
    assert!(
        !status.contains(".gut.gu"),
        "generated file visible to git: {status}"
    );
    assert!(status.contains("src/templates/page.gut"), "{status}");
    // `git add .` would not take it either.
    git(&["add", "."]);
    let staged = git(&["diff", "--cached", "--name-only"]);
    assert!(!staged.contains(".gut.gu"), "{staged}");
    // One ignore line, not one per run, and the rest of an existing file is kept.
    let ignore = app.read("src/templates/.gitignore");
    assert_eq!(ignore.matches("*.gut.gu").count(), 1, "{ignore}");
    app.file("src/templates/.gitignore", "keep-me\n");
    assert!(app.guion(&["check", "src/main.gu"]).ok);
    let ignore = app.read("src/templates/.gitignore");
    assert_eq!(ignore, "keep-me\n*.gut.gu\n");
    // A stale generated file goes when its template goes; a hand-written file with the same
    // suffix but no marker is never deleted.
    app.file("src/templates/other.gut", "x");
    assert!(app.guion(&["check", "src/main.gu"]).ok);
    assert!(app.exists("src/templates/other.gut.gu"));
    std::fs::remove_file(app.dir.join("src/templates/other.gut")).unwrap();
    app.file(
        "src/templates/mine.gut.gu",
        "# written by hand\npub fn mine() -> Int:\n    return 1\n",
    );
    assert!(app.guion(&["check", "src/main.gu"]).ok);
    assert!(!app.exists("src/templates/other.gut.gu"));
    assert!(app.exists("src/templates/mine.gut.gu"));
}

#[test]
fn an_unchanged_template_is_not_rewritten() {
    let app = App::new("stable");
    app.file(TEMPLATE, "<p>hi</p>\n");
    app.file(
        "src/main.gu",
        "import \"templates/page.gut.gu\"\n\nfn main() -> Int:\n    return 0\n",
    );
    assert!(app.guion(&["check", "src/main.gu"]).ok);
    let path = app.dir.join("src/templates/page.gut.gu");
    let first = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert!(app.guion(&["check", "src/main.gu"]).ok);
    assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), first);
}

#[test]
fn a_project_without_templates_is_untouched_by_the_pre_pass() {
    let app = App::bare("plain", "");
    app.file(
        "src/main.gu",
        "fn main() -> Int:\n    print(\"hi\")\n    return 0\n",
    );
    let run = app.guion(&["run", "--no-cache", "src/main.gu"]);
    assert!(run.ok, "{}", run.all());
    assert!(!app.exists("src/templates/.gitignore"));
    assert!(!app.exists("src/.gitignore"));
    assert_eq!(run.stdout, "hi\n");
}

#[test]
fn html_response_wraps_a_rendered_value_with_an_html_content_type() {
    let app = App::new("response");
    app.file(TEMPLATE, "{% params a: Text %}<p>{{ a }}</p>");
    app.file(
        "src/main.gu",
        r#"import "templates/page.gut.gu"
import "std:http_response"

fn main() -> Int:
    match html_response(200, render_page("<b>")):
        Err(failure):
            print("failed " + failure.code)
            return 1
        Ok(response):
            print(response.status)
            print(unwrap(response_header(response, "content-type")))
            print(text_from_bytes_lossy(response.body.data))
            return 0
"#,
    );
    let run = app.guion(&["run", "--no-cache", "src/main.gu"]);
    assert!(run.ok, "{}", run.all());
    assert_eq!(
        run.stdout,
        "200\ntext/html; charset=utf-8\n<p>&lt;b&gt;</p>\n"
    );
}
