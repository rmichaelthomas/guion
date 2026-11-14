pub use guion_model::Diagnostic;
use guion_model::{
    Artifact, BigInt, ExternalFunction, Float64, Function, HigherOrderKind, Instruction, Op,
    RecordDefinition, RecordField, Span, Type, Value,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Component, Path, PathBuf},
    time::Instant,
};

mod builtin_catalog;

const FN_NO_CLOSURES_MESSAGE: &str =
    "function values name an existing function; closures are not supported";

/// Opt-in timings for source-module loading and function lowering.
#[derive(Clone, Debug, Default)]
pub struct CompileProfile {
    /// Timing data for each unique module in the resolved source graph.
    pub modules: Vec<ModuleLoadProfile>,
    /// Time spent type-checking and lowering the combined program.
    pub compile_program_ns: u128,
}

/// Timings and source size for one resolved module.
#[derive(Clone, Debug, Default)]
pub struct ModuleLoadProfile {
    /// Stable logical source identity, such as `main.gu` or `std:time`.
    pub source_name: String,
    /// UTF-8 source size in bytes.
    pub source_bytes: usize,
    /// Number of direct imports declared by this module.
    pub import_count: usize,
    /// Time spent canonicalizing, validating, and stat'ing the module path.
    pub path_validation_ns: u128,
    /// Time spent reading the module source.
    pub source_read_ns: u128,
    /// Time spent lexing and parsing the module source.
    pub parse_ns: u128,
    /// Time spent resolving the module's direct import paths, excluding child loads.
    pub import_resolution_ns: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Kind {
    Name,
    Number,
    Text,
    /// `f"..."`: text whose `{...}` holes are expressions.
    FormatText,
    Symbol,
    Newline,
    Indent,
    Dedent,
    Eof,
}
#[derive(Clone, Debug)]
struct Token {
    kind: Kind,
    text: String,
    span: Span,
}

/// Suggestion rule for an unknown call name (B9). A candidate qualifies when its Levenshtein
/// distance over Unicode scalar values is at most `max(2, len(name) / 3)`, or when it is the
/// name followed by `_list` (the list-taking builtin of that name: `map` finds `map_list`,
/// `filter` finds `filter_list`) and the name has at least three characters. A candidate that
/// qualifies only by the `_list` rule counts as exactly the threshold away. Candidates are
/// sorted by distance, then by name, and at most three are kept. The self-hosted compiler
/// (`language/self_host/compiler.gu`, `suggest_names`) implements the same rule.
fn suggest_names(name: &str, candidates: &[&str]) -> Vec<String> {
    let name_chars: Vec<char> = name.chars().collect();
    let threshold = std::cmp::max(2, name_chars.len() / 3);
    let list_form = (name_chars.len() >= 3).then(|| format!("{name}_list"));
    let mut scored: Vec<(usize, &str)> = candidates
        .iter()
        .copied()
        .filter(|candidate| *candidate != name)
        .filter_map(|candidate| {
            let distance = edit_distance(&name_chars, candidate);
            if distance <= threshold {
                Some((distance, candidate))
            } else if list_form.as_deref() == Some(candidate) {
                Some((threshold, candidate))
            } else {
                None
            }
        })
        .collect();
    scored.sort();
    scored.dedup();
    scored
        .into_iter()
        .take(3)
        .map(|(_, candidate)| candidate.to_string())
        .collect()
}

fn edit_distance(left: &[char], right: &str) -> usize {
    let right: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    for (row, left_char) in left.iter().enumerate() {
        let mut current = vec![row + 1];
        for (column, right_char) in right.iter().enumerate() {
            let substitution = previous[column] + usize::from(left_char != right_char);
            current.push(
                substitution
                    .min(previous[column + 1] + 1)
                    .min(current[column] + 1),
            );
        }
        previous = current;
    }
    previous[right.len()]
}

fn diagnostic(code: &str, message: impl Into<String>, span: Span) -> Diagnostic {
    Diagnostic::new(code, message, span)
}

fn lex(source: &str, source_id: u32) -> Result<Vec<Token>, Diagnostic> {
    let mut tokens = Vec::new();
    let mut indent = vec![0usize];
    let mut offset = 0;
    for (row, raw) in source.split_inclusive('\n').enumerate() {
        let line = raw
            .strip_suffix('\n')
            .unwrap_or(raw)
            .strip_suffix('\r')
            .unwrap_or(raw.strip_suffix('\n').unwrap_or(raw));
        let bytes = line.as_bytes();
        let mut pos = 0;
        while bytes.get(pos) == Some(&b' ') {
            pos += 1;
        }
        if bytes.get(pos) == Some(&b'\t') {
            return Err(diagnostic(
                "E_INDENT_TAB",
                "tabs are not allowed for indentation",
                Span {
                    source_id,
                    start: offset + pos,
                    end: offset + pos + 1,
                    line: row + 1,
                    column: pos + 1,
                },
            ));
        }
        if pos == bytes.len() || bytes.get(pos) == Some(&b'#') {
            offset += raw.len();
            continue;
        }
        if pos % 4 != 0 {
            return Err(diagnostic(
                "E_INDENT_WIDTH",
                "indentation must use multiples of four spaces",
                Span {
                    source_id,
                    start: offset,
                    end: offset + pos,
                    line: row + 1,
                    column: 1,
                },
            ));
        }
        let current = *indent.last().unwrap();
        if pos > current {
            if pos != current + 4 {
                return Err(diagnostic(
                    "E_INDENT_WIDTH",
                    "indentation may increase by one level at a time",
                    Span {
                        source_id,
                        start: offset,
                        end: offset + pos,
                        line: row + 1,
                        column: 1,
                    },
                ));
            }
            indent.push(pos);
            tokens.push(Token {
                kind: Kind::Indent,
                text: String::new(),
                span: Span {
                    source_id,
                    start: offset,
                    end: offset + pos,
                    line: row + 1,
                    column: 1,
                },
            });
        } else if pos < current {
            while pos < *indent.last().unwrap() {
                indent.pop();
                tokens.push(Token {
                    kind: Kind::Dedent,
                    text: String::new(),
                    span: Span {
                        source_id,
                        start: offset,
                        end: offset + pos,
                        line: row + 1,
                        column: pos + 1,
                    },
                });
            }
            if pos != *indent.last().unwrap() {
                return Err(diagnostic(
                    "E_INDENT_WIDTH",
                    "indentation does not match an outer block",
                    Span {
                        source_id,
                        start: offset,
                        end: offset + pos,
                        line: row + 1,
                        column: 1,
                    },
                ));
            }
        }
        let mut i = pos;
        while i < bytes.len() {
            let c = bytes[i] as char;
            if c == ' ' {
                i += 1;
                continue;
            }
            if c == '#' {
                break;
            }
            let start = i;
            let kind;
            let text;
            let format_literal = c == 'f' && bytes.get(i + 1) == Some(&b'"');
            if format_literal {
                i += 1;
            }
            if let Some(prefix_len) = python_fstring_prefix(bytes, i).filter(|_| !format_literal) {
                return Err(diagnostic(
                    "E_PYTHON_FSTRING",
                    "Guion text with values inside is written f\"... {name} ...\" with a double quote and no other prefix; for other text use concatenation with `+`.",
                    Span {
                        source_id,
                        start: offset + i,
                        end: offset + i + prefix_len,
                        line: row + 1,
                        column: line[..i].chars().count() + 1,
                    },
                ));
            } else if (c.is_ascii_alphabetic() || c == '_') && !format_literal {
                i += 1;
                while i < bytes.len()
                    && ((bytes[i] as char).is_ascii_alphanumeric() || bytes[i] == b'_')
                {
                    i += 1;
                }
                kind = Kind::Name;
                text = line[start..i].to_owned();
            } else if c.is_ascii_digit() {
                i += 1;
                while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
                    i += 1;
                }
                if bytes.get(i) == Some(&b'.') {
                    i += 1;
                    let fractional_start = i;
                    while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
                        i += 1;
                    }
                    if i == fractional_start {
                        return Err(diagnostic(
                            "E_FLOAT_LITERAL",
                            "a decimal point must be followed by digits",
                            Span {
                                source_id,
                                start: offset + start,
                                end: offset + i,
                                line: row + 1,
                                column: start + 1,
                            },
                        ));
                    }
                }
                if matches!(bytes.get(i), Some(b'e' | b'E')) {
                    i += 1;
                    if matches!(bytes.get(i), Some(b'+' | b'-')) {
                        i += 1;
                    }
                    let exponent_start = i;
                    while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
                        i += 1;
                    }
                    if i == exponent_start {
                        return Err(diagnostic(
                            "E_FLOAT_LITERAL",
                            "a floating-point exponent must contain digits",
                            Span {
                                source_id,
                                start: offset + start,
                                end: offset + i,
                                line: row + 1,
                                column: start + 1,
                            },
                        ));
                    }
                }
                kind = Kind::Number;
                text = line[start..i].to_owned();
            } else if c == '"' || format_literal {
                i += 1;
                let mut value = String::new();
                let mut closed = false;
                while i < bytes.len() {
                    let ch = line[i..].chars().next().unwrap();
                    if ch == '"' {
                        i += 1;
                        closed = true;
                        break;
                    }
                    if ch == '\\' {
                        i += 1;
                        if i >= bytes.len() {
                            break;
                        }
                        let escaped = bytes[i] as char;
                        let escape_start = i - 1;
                        match escaped {
                            'n' => value.push('\n'),
                            'r' => value.push('\r'),
                            't' => value.push('\t'),
                            '"' => value.push('"'),
                            '\\' => value.push('\\'),
                            'u' => {
                                let digits_start = i + 2;
                                if bytes.get(i + 1) != Some(&b'{') {
                                    return Err(diagnostic(
                                        "E_STRING_ESCAPE",
                                        "Unicode escapes must use braces with hexadecimal digits",
                                        Span {
                                            source_id,
                                            start: offset + escape_start,
                                            end: offset + i + 1,
                                            line: row + 1,
                                            column: line[..escape_start].chars().count() + 1,
                                        },
                                    ));
                                }
                                let mut digits_end = digits_start;
                                while bytes.get(digits_end).is_some_and(u8::is_ascii_hexdigit) {
                                    digits_end += 1;
                                }
                                let Some(close) =
                                    bytes.get(digits_end).filter(|byte| **byte == b'}')
                                else {
                                    return Err(diagnostic(
                                        "E_STRING_ESCAPE",
                                        "Unicode escape must contain hexadecimal digits and a closing brace",
                                        Span {
                                            source_id,
                                            start: offset + escape_start,
                                            end: offset + digits_end.min(bytes.len()),
                                            line: row + 1,
                                            column: line[..escape_start].chars().count() + 1,
                                        },
                                    ));
                                };
                                let digits = &line[digits_start..digits_end];
                                let scalar = (1..=6).contains(&digits.len())
                                    .then(|| u32::from_str_radix(digits, 16).ok())
                                    .flatten()
                                    .and_then(char::from_u32)
                                    .ok_or_else(|| {
                                        diagnostic(
                                            "E_STRING_ESCAPE",
                                            "Unicode escape must identify a valid Unicode scalar value",
                                            Span {
                                                source_id,
                                                start: offset + escape_start,
                                                end: offset + digits_end + 1,
                                                line: row + 1,
                                                column: line[..escape_start].chars().count() + 1,
                                            },
                                        )
                                    })?;
                                value.push(scalar);
                                let _ = close;
                                i = digits_end + 1;
                                continue;
                            }
                            _ => {
                                return Err(diagnostic(
                                    "E_STRING_ESCAPE",
                                    "unsupported string escape",
                                    Span {
                                        source_id,
                                        start: offset + escape_start,
                                        end: offset + i + 1,
                                        line: row + 1,
                                        column: line[..escape_start].chars().count() + 1,
                                    },
                                ))
                            }
                        }
                        i += 1;
                    } else {
                        value.push(ch);
                        i += ch.len_utf8();
                    }
                }
                if !closed {
                    return Err(diagnostic(
                        "E_STRING_UNCLOSED",
                        "string literal is missing its closing quote",
                        Span {
                            source_id,
                            start: offset + start,
                            end: offset + bytes.len(),
                            line: row + 1,
                            column: line[..start].chars().count() + 1,
                        },
                    ));
                }
                kind = if format_literal {
                    Kind::FormatText
                } else {
                    Kind::Text
                };
                text = value;
            } else {
                let pair = bytes
                    .get(i..i + 2)
                    .and_then(|s| std::str::from_utf8(s).ok())
                    .unwrap_or("");
                if ["->", "==", "!=", "<=", ">=", "//"].contains(&pair) {
                    text = pair.to_owned();
                    i += 2;
                } else if "+-*/%() :,<>[]=?{}.".contains(c) {
                    text = c.to_string();
                    i += 1;
                } else {
                    return Err(diagnostic(
                        "E_SYNTAX",
                        format!("unexpected character `{c}`"),
                        Span {
                            source_id,
                            start: offset + i,
                            end: offset + i + c.len_utf8(),
                            line: row + 1,
                            column: line[..i].chars().count() + 1,
                        },
                    ));
                }
                kind = Kind::Symbol;
            }
            tokens.push(Token {
                kind,
                text,
                span: Span {
                    source_id,
                    start: offset + start,
                    end: offset + i,
                    line: row + 1,
                    column: line[..start].chars().count() + 1,
                },
            });
        }
        tokens.push(Token {
            kind: Kind::Newline,
            text: String::new(),
            span: Span {
                source_id,
                start: offset + line.len(),
                end: offset + line.len(),
                line: row + 1,
                column: line.len() + 1,
            },
        });
        offset += raw.len();
    }
    while indent.len() > 1 {
        indent.pop();
        tokens.push(Token {
            kind: Kind::Dedent,
            text: String::new(),
            span: Span {
                source_id,
                start: source.len(),
                end: source.len(),
                line: source.lines().count().max(1),
                column: 1,
            },
        });
    }
    tokens.push(Token {
        kind: Kind::Eof,
        text: String::new(),
        span: Span {
            source_id,
            start: source.len(),
            end: source.len(),
            line: source.lines().count().max(1),
            column: 1,
        },
    });
    Ok(tokens)
}

fn python_fstring_prefix(bytes: &[u8], start: usize) -> Option<usize> {
    let first = bytes.get(start).copied()?;
    let has_quote = |index: usize| {
        bytes
            .get(index)
            .is_some_and(|byte| *byte == b'\'' || *byte == b'"')
    };
    if matches!(first, b'f' | b'F') && has_quote(start + 1) {
        return Some(1);
    }
    if matches!(first, b'r' | b'R')
        && bytes
            .get(start + 1)
            .is_some_and(|byte| *byte == b'f' || *byte == b'F')
        && has_quote(start + 2)
    {
        return Some(2);
    }
    if matches!(first, b'f' | b'F')
        && bytes
            .get(start + 1)
            .is_some_and(|byte| *byte == b'r' || *byte == b'R')
        && has_quote(start + 2)
    {
        return Some(2);
    }
    None
}

fn starts_statement(tokens: &[Token], index: usize) -> bool {
    tokens[..index]
        .iter()
        .rev()
        .find(|token| !matches!(token.kind, Kind::Indent | Kind::Dedent))
        .is_none_or(|token| token.kind == Kind::Newline)
}

fn python_recovery_diagnostic(tokens: &[Token]) -> Option<Diagnostic> {
    for (index, token) in tokens.iter().enumerate() {
        if token.kind != Kind::Name {
            continue;
        }
        let (code, message) = match token.text.as_str() {
            "def" if starts_statement(tokens, index) => (
                "E_PYTHON_DEF",
                "Python `def` is not Guion syntax; use `fn` declarations such as `fn name(parameters) -> Type:` and indent the body.",
            ),
            "elif" if starts_statement(tokens, index) => (
                "E_PYTHON_ELIF",
                "Python `elif` is not Guion syntax; write `else if condition:` (or a nested `if` inside `else`).",
            ),
            "class" if starts_statement(tokens, index) => (
                "E_PYTHON_CLASS",
                "Python classes are not Guion syntax; use `record` for data fields and top-level `fn` functions for behavior. Inheritance is not supported.",
            ),
            "try" | "except" if starts_statement(tokens, index) => (
                "E_PYTHON_TRY",
                "Python `try`/`except` is not Guion syntax; return `Result[T, E]` and use an exhaustive `match` with `Ok` and `Err` arms.",
            ),
            "True" | "False" => (
                "E_PYTHON_BOOL",
                "Python boolean spellings differ; use Guion `true` and `false`.",
            ),
            "from" if starts_statement(tokens, index) => (
                "E_PYTHON_IMPORT",
                "Guion imports modules by quoted relative path; selective imports are not supported. Use `import \"relative-path.gu\"` and call exported names directly.",
            ),
            "import"
                if starts_statement(tokens, index)
                    && tokens.get(index + 1).is_some_and(|next| next.kind == Kind::Name) =>
            {
                (
                    "E_PYTHON_IMPORT",
                    "Guion imports modules by quoted relative path; use `import \"relative-path.gu\"` and call exported names directly.",
                )
            }
            _ => continue,
        };
        return Some(diagnostic(code, message, token.span));
    }


    let mut bracket_depth = 0usize;
    for token in tokens {
        if token.kind == Kind::Symbol && token.text == "[" {
            bracket_depth += 1;
        } else if token.kind == Kind::Symbol && token.text == "]" {
            bracket_depth = bracket_depth.saturating_sub(1);
        } else if bracket_depth > 0 && token.kind == Kind::Name && token.text == "for" {
            return Some(diagnostic(
                "E_PYTHON_COMPREHENSION",
                "Python list comprehensions are not supported; use an explicit loop with `list_push` to build a list.",
                token.span,
            ));
        }
    }
    None
}

#[derive(Clone, Debug)]
struct Program {
    functions: Vec<FunctionAst>,
    externals: Vec<ExternalAst>,
    records: Vec<RecordAst>,
    imports: Vec<ImportAst>,
    array_elements: Vec<(usize, String, Span)>,
}
#[derive(Clone, Debug)]
struct ImportAst {
    path: String,
    span: Span,
    reexport: bool,
}
#[derive(Clone, Debug)]
struct RecordAst {
    name: String,
    fields: Vec<(String, Type, Span)>,
    /// Fields written `priv name: Type`: read, assigned and constructed only in the module that
    /// declares the record.
    private_fields: Vec<String>,
    span: Span,
    public: bool,
    module_id: usize,
}
#[derive(Clone, Debug)]
struct ExternalAst {
    name: String,
    module: String,
    callable: String,
    params: Vec<(String, Type, Span)>,
    result: Type,
    span: Span,
    module_id: usize,
}
#[derive(Clone, Debug)]
struct FunctionAst {
    name: String,
    params: Vec<(String, Type, Span)>,
    result: Type,
    body: Vec<Stmt>,
    span: Span,
    public: bool,
    module_id: usize,
}
#[derive(Clone, Debug)]
enum Stmt {
    Let(String, bool, Option<Type>, Expr, Span),
    Set(String, Expr, Span),
    Return(Expr, Span),
    If(Expr, Vec<Stmt>, Vec<Stmt>, Span),
    While(Expr, Vec<Stmt>, Span),
    For(String, Expr, Vec<Stmt>, Span),
    Match(Expr, Vec<MatchArm>, Span),
    Break(Span),
    Continue(Span),
    Expr(Expr, Span),
}
#[derive(Clone, Debug)]
struct MatchArm {
    pattern: MatchPattern,
    body: Vec<Stmt>,
    span: Span,
}
#[derive(Clone, Debug)]
enum MatchPattern {
    Some(String, Span),
    None,
    Ok(String, Span),
    Err(String, Span),
}
#[derive(Clone, Debug)]
enum Expr {
    Int(BigInt, Span),
    Float(Float64, Span),
    Bool(bool, Span),
    Text(String, Span),
    Name(String, Span),
    Unary(String, Box<Expr>, Span),
    Binary(String, Box<Expr>, Box<Expr>, Span),
    Call(String, Vec<Expr>, Vec<(String, Expr, Span)>, Span),
    Try(Box<Expr>, Span),
    List(Vec<Expr>, Span),
    Map(Vec<(Expr, Expr)>, Span),
    Index(Box<Expr>, Box<Expr>, Span),
    Field(Box<Expr>, String, Span),
    /// `f"text {expression} text"`.
    Interpolated(Vec<FormatPart>, Span),
}
#[derive(Clone, Debug)]
enum FormatPart {
    Literal(String),
    Hole(Expr),
}
impl Expr {
    fn span(&self) -> Span {
        match self {
            Expr::Int(_, s)
            | Expr::Float(_, s)
            | Expr::Bool(_, s)
            | Expr::Text(_, s)
            | Expr::Name(_, s)
            | Expr::Unary(_, _, s)
            | Expr::Binary(_, _, _, s)
            | Expr::Call(_, _, _, s) => *s,
            Expr::Try(_, s) => *s,
            Expr::List(_, s) | Expr::Map(_, s) | Expr::Index(_, _, s) | Expr::Field(_, _, s) => *s,
            Expr::Interpolated(_, s) => *s,
        }
    }
}

/// `set s = s + a + b` parses as `(s + a) + b`, whose first addition copies `s`. Text
/// concatenation is associative, so this regroups it as `s + (a + b)`: the operands still
/// evaluate in source order, and the final addition is followed by the store into `s`, which
/// the VM turns into an in-place append. Callers apply it only to Text locals.
fn regroup_local_text_append(value: &Expr, name: &str) -> Option<Expr> {
    let mut spine = Vec::new();
    let mut cursor = value;
    while let Expr::Binary(op, left, _, _) = cursor {
        if op != "+" {
            break;
        }
        spine.push(cursor);
        cursor = left;
    }
    if spine.len() < 2 || !matches!(cursor, Expr::Name(source, _) if source == name) {
        return None;
    }
    let Some(Expr::Binary(op, head, first, span)) = spine.pop() else {
        unreachable!("the spine holds only additions");
    };
    let mut rest = (**first).clone();
    while let Some(Expr::Binary(op, _, right, span)) = spine.pop() {
        rest = Expr::Binary(op.clone(), Box::new(rest), right.clone(), *span);
    }
    Some(Expr::Binary(
        op.clone(),
        head.clone(),
        Box::new(rest),
        *span,
    ))
}

const ARRAY_ELEMENT_TYPES_MESSAGE: &str =
    "Array element type must be one of i8, i16, i32, i64, u8, u16, u32, u64, f32, f64, bool";

struct Parser {
    tokens: Vec<Token>,
    at: usize,
    module_id: usize,
    /// Names written where an `Array[...]` element type belongs, with the module that wrote
    /// them: a record name or an unknown one, told apart after every module is loaded.
    array_elements: Vec<(usize, String, Span)>,
}
impl Parser {
    fn cur(&self) -> &Token {
        &self.tokens[self.at.min(self.tokens.len() - 1)]
    }
    fn advance(&mut self) -> Token {
        let t = self.cur().clone();
        if t.kind != Kind::Eof {
            self.at += 1;
        }
        t
    }
    fn eat(&mut self, s: &str) -> Option<Token> {
        if self.cur().text == s {
            Some(self.advance())
        } else {
            None
        }
    }
    fn expect(&mut self, s: &str) -> Result<Token, Diagnostic> {
        self.eat(s)
            .ok_or_else(|| diagnostic("E_SYNTAX", format!("expected `{s}`"), self.cur().span))
    }
    /// Splits `f"..."` text into literal pieces and `{expression}` holes. `{{` and `}}` are
    /// literal braces. A hole holds an expression without string literals.
    fn format_text(&self, token: &Token) -> Result<Expr, Diagnostic> {
        let mut parts = Vec::new();
        let mut literal = String::new();
        let mut chars = token.text.chars().peekable();
        while let Some(character) = chars.next() {
            match character {
                '{' if chars.peek() == Some(&'{') => {
                    chars.next();
                    literal.push('{');
                }
                '}' if chars.peek() == Some(&'}') => {
                    chars.next();
                    literal.push('}');
                }
                '}' => {
                    return Err(diagnostic(
                        "E_FSTRING_HOLE",
                        "a lone `}` in f-text; write `}}` for a literal brace",
                        token.span,
                    ));
                }
                '{' => {
                    let mut hole = String::new();
                    let mut closed = false;
                    for inner in chars.by_ref() {
                        if inner == '}' {
                            closed = true;
                            break;
                        }
                        hole.push(inner);
                    }
                    if !closed {
                        return Err(diagnostic(
                            "E_FSTRING_HOLE",
                            "a `{` in f-text is not closed; write `{{` for a literal brace",
                            token.span,
                        ));
                    }
                    let hole = hole.trim();
                    if hole.is_empty() || hole.contains('"') || hole.contains('\\') || hole.contains('{') {
                        return Err(diagnostic(
                            "E_FSTRING_HOLE",
                            "a hole in f-text holds one expression without string literals, such as {count} or {point.x + 1}",
                            token.span,
                        ));
                    }
                    let mut tokens = lex(hole, token.span.source_id)?;
                    for hole_token in &mut tokens {
                        hole_token.span = token.span;
                    }
                    let mut sub = Parser {
                        tokens,
                        at: 0,
                        module_id: self.module_id,
                        array_elements: Vec::new(),
                    };
                    let expression = sub.expr(0)?;
                    if !matches!(sub.cur().kind, Kind::Newline | Kind::Eof | Kind::Dedent) {
                        return Err(diagnostic(
                            "E_FSTRING_HOLE",
                            "a hole in f-text holds one expression",
                            token.span,
                        ));
                    }
                    if !literal.is_empty() {
                        parts.push(FormatPart::Literal(std::mem::take(&mut literal)));
                    }
                    parts.push(FormatPart::Hole(expression));
                }
                other => literal.push(other),
            }
        }
        if !literal.is_empty() || parts.is_empty() {
            parts.push(FormatPart::Literal(literal));
        }
        Ok(Expr::Interpolated(parts, token.span))
    }
    fn name(&mut self) -> Result<Token, Diagnostic> {
        if self.cur().kind == Kind::Name {
            Ok(self.advance())
        } else {
            Err(diagnostic("E_SYNTAX", "expected a name", self.cur().span))
        }
    }
    fn ty(&mut self) -> Result<Type, Diagnostic> {
        let t = self.name()?;
        match t.text.as_str() {
            "Int" => Ok(Type::Int),
            "Float64" => Ok(Type::Float64),
            "Bool" => Ok(Type::Bool),
            "Text" => Ok(Type::Text),
            "Bytes" => Ok(Type::Bytes),
            "Unit" => Ok(Type::Unit),
            "TaskError" => Ok(Type::TaskError),
            "JsonError" => Ok(Type::JsonError),
            "FileError" => Ok(Type::FileError),
            "HttpError" => Ok(Type::HttpError),
            "FileReader" | "Db" | "Transaction" | "TomlDocument" => {
                Ok(Type::NativeResource(t.text))
            }
            "Json" => Ok(Type::Json),
            "fn" => {
                self.expect("(")?;
                let mut parameters = Vec::new();
                if self.cur().text != ")" {
                    loop {
                        parameters.push(self.ty()?);
                        if self.eat(",").is_none() {
                            break;
                        }
                    }
                }
                self.expect(")")?;
                self.expect("->")?;
                let result = self.ty()?;
                Ok(Type::Fn(parameters, Box::new(result)))
            }
            "Array" => {
                self.expect("[")?;
                if self.cur().kind == Kind::Name {
                    if let Some(element) = guion_model::ElementType::from_name(&self.cur().text) {
                        self.advance();
                        self.expect("]")?;
                        return Ok(Type::Array(element));
                    }
                }
                let inner_span = self.cur().span;
                let inner = self.ty()?;
                self.expect("]")?;
                match inner {
                    // Whether the name is a record is only known once every module is
                    // loaded; `compile_program` reports it with the right code.
                    Type::Record(name) => {
                        self.array_elements.push((self.module_id, name, inner_span));
                        // A placeholder: `Array[Bool]` is not a type a program can write, and
                        // every name recorded above is checked after parsing, which always
                        // fails (`E_TYPE_UNKNOWN` or `E_TYPE_ANNOTATION`). A parse path that
                        // skipped that check would accept the program with this type, so
                        // every route to a finished program must go through
                        // `compile_program`.
                        Ok(Type::Array(guion_model::ElementType::Bool))
                    }
                    _ => Err(diagnostic(
                        "E_TYPE_ANNOTATION",
                        ARRAY_ELEMENT_TYPES_MESSAGE,
                        inner_span,
                    )),
                }
            }
            "List" => {
                self.expect("[")?;
                let item = self.ty()?;
                self.expect("]")?;
                Ok(Type::List(Box::new(item)))
            }
            "Map" => {
                self.expect("[")?;
                let key = self.ty()?;
                self.expect(",")?;
                let item = self.ty()?;
                self.expect("]")?;
                if key != Type::Text {
                    return Err(diagnostic(
                        "E_TYPE_MAP_KEY",
                        "Map keys must have type Text",
                        t.span,
                    ));
                }
                Ok(Type::Map(Box::new(item)))
            }
            "Option" => {
                self.expect("[")?;
                let item = self.ty()?;
                self.expect("]")?;
                Ok(Type::Option(Box::new(item)))
            }
            "Result" => {
                self.expect("[")?;
                let ok = self.ty()?;
                self.expect(",")?;
                let error = self.ty()?;
                self.expect("]")?;
                Ok(Type::Result(Box::new(ok), Box::new(error)))
            }
            _ => {
                if guion_model::ElementType::from_name(&t.text).is_some() {
                    return Err(diagnostic(
                        "E_TYPE_UNKNOWN",
                        format!(
                            "unknown type `{}`; element types are only valid inside Array[...]",
                            t.text
                        ),
                        t.span,
                    ));
                }
                Ok(Type::Record(t.text))
            }
        }
    }
    fn record(&mut self, public: bool) -> Result<RecordAst, Diagnostic> {
        let start = self.expect("record")?.span;
        let name = self.name()?;
        self.expect(":")?;
        self.expect_newline()?;
        if self.cur().kind != Kind::Indent {
            return Err(diagnostic(
                "E_RECORD_EMPTY",
                "records must declare at least one field",
                self.cur().span,
            ));
        }
        self.advance();
        let mut fields = Vec::new();
        let mut private_fields = Vec::new();
        let mut names = std::collections::HashSet::new();
        while self.cur().kind != Kind::Dedent && self.cur().kind != Kind::Eof {
            if self.cur().kind == Kind::Newline {
                self.advance();
                continue;
            }
            let private = self.cur().text == "priv"
                && self
                    .tokens
                    .get(self.at + 1)
                    .is_some_and(|next| next.kind == Kind::Name);
            if private {
                self.advance();
            }
            let field = self.name()?;
            self.expect(":")?;
            let ty = self.ty()?;
            self.expect_newline()?;
            if private {
                private_fields.push(field.text.clone());
            }
            if !names.insert(field.text.clone()) {
                return Err(diagnostic(
                    "E_RECORD_FIELD_DUPLICATE",
                    format!(
                        "record `{}` declares field `{}` more than once",
                        name.text, field.text
                    ),
                    field.span,
                ));
            }
            fields.push((field.text, ty, field.span));
        }
        if self.cur().kind != Kind::Dedent {
            return Err(diagnostic(
                "E_RECORD_UNCLOSED",
                "record declaration is not closed",
                self.cur().span,
            ));
        }
        self.advance();
        Ok(RecordAst {
            name: name.text,
            fields,
            private_fields,
            span: start,
            public,
            module_id: self.module_id,
        })
    }
    fn program(mut self) -> Result<Program, Diagnostic> {
        let mut functions = Vec::new();
        let mut externals = Vec::new();
        let mut records = Vec::new();
        let mut imports = Vec::new();
        while self.cur().kind != Kind::Eof {
            while self.cur().kind == Kind::Newline {
                self.advance();
            }
            if self.cur().kind == Kind::Eof {
                break;
            }
            let reexport = self.cur().text == "pub"
                && self
                    .tokens
                    .get(self.at + 1)
                    .is_some_and(|token| token.text == "import");
            if self.cur().text == "import" || reexport {
                if reexport {
                    self.advance();
                }
                let start = self.advance().span;
                let path = self.advance();
                if path.kind != Kind::Text || path.text.is_empty() {
                    return Err(diagnostic(
                        "E_IMPORT_PATH",
                        "import requires a nonempty quoted relative path",
                        path.span,
                    ));
                }
                self.expect_newline()?;
                if imports
                    .iter()
                    .any(|import: &ImportAst| import.path == path.text)
                {
                    return Err(diagnostic(
                        "E_IMPORT_DUPLICATE",
                        format!("module `{}` is imported more than once", path.text),
                        start,
                    ));
                }
                imports.push(ImportAst {
                    path: path.text,
                    span: start,
                    reexport,
                });
                continue;
            }
            let public = self.eat("pub").is_some();
            if self.cur().text == "record" {
                records.push(self.record(public)?);
                continue;
            }
            if self.cur().text == "extern" {
                if public {
                    return Err(diagnostic(
                        "E_EXTERN_PUBLIC",
                        "Python external declarations cannot be exported yet",
                        self.cur().span,
                    ));
                }
                let start = self.advance().span;
                let language = self.name()?;
                if language.text != "python" {
                    return Err(diagnostic(
                        "E_EXTERN_LANGUAGE",
                        "only `extern python` declarations are supported",
                        language.span,
                    ));
                }
                let target = self.advance();
                if target.kind != Kind::Text {
                    return Err(diagnostic(
                        "E_EXTERN_TARGET",
                        "expected a quoted Python module and callable",
                        target.span,
                    ));
                }
                let (module, callable) = target.text.split_once(':').ok_or_else(|| {
                    diagnostic(
                        "E_EXTERN_TARGET",
                        "Python target must use `module:callable`",
                        target.span,
                    )
                })?;
                if module.is_empty()
                    || callable.is_empty()
                    || callable.split('.').any(str::is_empty)
                {
                    return Err(diagnostic(
                        "E_EXTERN_TARGET",
                        "Python module and callable must be nonempty",
                        target.span,
                    ));
                }
                self.expect("fn")?;
                let name = self.name()?;
                let params = self.parameters()?;
                self.expect("->")?;
                let result = self.ty()?;
                self.expect_newline()?;
                externals.push(ExternalAst {
                    name: name.text,
                    module: module.to_owned(),
                    callable: callable.to_owned(),
                    params,
                    result,
                    span: start,
                    module_id: self.module_id,
                });
                continue;
            }
            let start = self.expect("fn")?.span;
            let name = self.name()?;
            self.expect("(")?;
            let params = self.parameters_after_open()?;
            self.expect("->")?;
            let result = self.ty()?;
            self.expect(":")?;
            self.expect_newline()?;
            let body = self.block()?;
            functions.push(FunctionAst {
                name: name.text,
                params,
                result,
                body,
                span: start,
                public,
                module_id: self.module_id,
            });
        }
        if functions.is_empty() && externals.is_empty() && records.is_empty() {
            return Err(diagnostic(
                "E_MODULE_EMPTY",
                "module must define at least one function, external, or record",
                self.cur().span,
            ));
        }
        Ok(Program {
            functions,
            externals,
            records,
            imports,
            array_elements: std::mem::take(&mut self.array_elements),
        })
    }
    fn parameters(&mut self) -> Result<Vec<(String, Type, Span)>, Diagnostic> {
        self.expect("(")?;
        self.parameters_after_open()
    }
    fn parameters_after_open(&mut self) -> Result<Vec<(String, Type, Span)>, Diagnostic> {
        let mut params = Vec::new();
        if self.cur().text != ")" {
            loop {
                let parameter = self.name()?;
                self.expect(":")?;
                let ty = self.ty()?;
                params.push((parameter.text, ty, parameter.span));
                if self.eat(",").is_none() {
                    break;
                }
            }
        }
        self.expect(")")?;
        Ok(params)
    }
    fn expect_newline(&mut self) -> Result<(), Diagnostic> {
        if self.cur().kind == Kind::Newline {
            self.advance();
            Ok(())
        } else {
            Err(diagnostic(
                "E_SYNTAX",
                "expected end of line",
                self.cur().span,
            ))
        }
    }
    fn block(&mut self) -> Result<Vec<Stmt>, Diagnostic> {
        if self.cur().kind != Kind::Indent {
            return Err(diagnostic(
                "E_BLOCK_EMPTY",
                "expected an indented block",
                self.cur().span,
            ));
        }
        self.advance();
        let mut body = Vec::new();
        while self.cur().kind != Kind::Dedent && self.cur().kind != Kind::Eof {
            if self.cur().kind == Kind::Newline {
                self.advance();
                continue;
            }
            body.push(self.statement()?);
        }
        if self.cur().kind != Kind::Dedent {
            return Err(diagnostic(
                "E_BLOCK_UNCLOSED",
                "block is not closed",
                self.cur().span,
            ));
        }
        self.advance();
        if body.is_empty() {
            return Err(diagnostic(
                "E_BLOCK_EMPTY",
                "blocks must contain a statement",
                self.cur().span,
            ));
        }
        Ok(body)
    }
    fn statement(&mut self) -> Result<Stmt, Diagnostic> {
        let t = self.cur().clone();
        match t.text.as_str() {
            "let" | "var" => {
                let mutable = self.advance().text == "var";
                let n = self.name()?;
                let annotation = if self.eat(":").is_some() {
                    Some(self.ty()?)
                } else {
                    None
                };
                self.expect("=")?;
                let e = self.expr(0)?;
                self.expect_newline()?;
                Ok(Stmt::Let(n.text, mutable, annotation, e, t.span))
            }
            "set" => {
                self.advance();
                let n = self.name()?;
                // `set record.field = value` and `set record.field.inner = value`: the target is
                // the dotted path, which the compiler resolves to a hidden local per field.
                let mut target = n.text.clone();
                while self.eat(".").is_some() {
                    let field = self.name()?;
                    target.push('.');
                    target.push_str(&field.text);
                }
                // `set x += e` (also `-=`, `*=`, `/=`, `//=`, `%=`) is `set x = x + e`. The two
                // characters must touch, so `set x + = e` stays an error.
                let compound = ["+", "-", "*", "//", "/", "%"].into_iter().find(|operator| {
                    self.cur().text == *operator
                        && self.tokens.get(self.at + 1).is_some_and(|next| {
                            next.text == "=" && next.span.start == self.cur().span.end
                        })
                });
                if let Some(operator) = compound {
                    let operator_span = self.advance().span;
                    self.expect("=")?;
                    let operand = self.expr(0)?;
                    self.expect_newline()?;
                    let mut read: Expr = Expr::Name(n.text.clone(), n.span);
                    for field in target[n.text.len()..].split('.').filter(|part| !part.is_empty()) {
                        read = Expr::Field(Box::new(read), field.to_owned(), n.span);
                    }
                    let combined = Expr::Binary(
                        operator.to_owned(),
                        Box::new(read),
                        Box::new(operand),
                        operator_span,
                    );
                    return Ok(Stmt::Set(target, combined, t.span));
                }
                self.expect("=")?;
                let e = self.expr(0)?;
                self.expect_newline()?;
                Ok(Stmt::Set(target, e, t.span))
            }
            "return" => {
                self.advance();
                let e = self.expr(0)?;
                self.expect_newline()?;
                Ok(Stmt::Return(e, t.span))
            }
            "if" => {
                self.advance();
                let c = self.expr(0)?;
                self.expect(":")?;
                self.expect_newline()?;
                let yes = self.block()?;
                let no = if self.eat("else").is_some() {
                    if self.cur().text == "if" {
                        // `else if condition:` is an `if` that is the whole `else` branch; it
                        // reads its own `else if` / `else` chain.
                        vec![self.statement()?]
                    } else {
                        self.expect(":")?;
                        self.expect_newline()?;
                        self.block()?
                    }
                } else {
                    Vec::new()
                };
                Ok(Stmt::If(c, yes, no, t.span))
            }
            "break" | "continue" => {
                self.advance();
                self.expect_newline()?;
                Ok(if t.text == "break" {
                    Stmt::Break(t.span)
                } else {
                    Stmt::Continue(t.span)
                })
            }
            "while" => {
                self.advance();
                let c = self.expr(0)?;
                self.expect(":")?;
                self.expect_newline()?;
                let body = self.block()?;
                Ok(Stmt::While(c, body, t.span))
            }
            "for" => {
                self.advance();
                let name = self.name()?;
                self.expect("in")?;
                let values = self.expr(0)?;
                self.expect(":")?;
                self.expect_newline()?;
                let body = self.block()?;
                Ok(Stmt::For(name.text, values, body, t.span))
            }
            "match" => {
                self.advance();
                let value = self.expr(0)?;
                self.expect(":")?;
                self.expect_newline()?;
                if self.cur().kind != Kind::Indent {
                    return Err(diagnostic(
                        "E_MATCH_EMPTY",
                        "match requires indented variant arms",
                        self.cur().span,
                    ));
                }
                self.advance();
                let mut arms = Vec::new();
                while self.cur().kind != Kind::Dedent && self.cur().kind != Kind::Eof {
                    if self.cur().kind == Kind::Newline {
                        self.advance();
                        continue;
                    }
                    let variant = self.name()?;
                    let pattern = match variant.text.as_str() {
                        "Some" | "Ok" | "Err" => {
                            self.expect("(")?;
                            let binding = self.name()?;
                            self.expect(")")?;
                            match variant.text.as_str() {
                                "Some" => MatchPattern::Some(binding.text, binding.span),
                                "Ok" => MatchPattern::Ok(binding.text, binding.span),
                                _ => MatchPattern::Err(binding.text, binding.span),
                            }
                        }
                        "None" => MatchPattern::None,
                        _ => {
                            return Err(diagnostic(
                                "E_MATCH_PATTERN",
                                "expected Some(name), None, Ok(name), or Err(name)",
                                variant.span,
                            ));
                        }
                    };
                    self.expect(":")?;
                    self.expect_newline()?;
                    let body = self.block()?;
                    arms.push(MatchArm {
                        pattern,
                        body,
                        span: variant.span,
                    });
                }
                if self.cur().kind != Kind::Dedent {
                    return Err(diagnostic(
                        "E_MATCH_UNCLOSED",
                        "match block is not closed",
                        self.cur().span,
                    ));
                }
                self.advance();
                if arms.is_empty() {
                    return Err(diagnostic(
                        "E_MATCH_EMPTY",
                        "match requires at least one variant arm",
                        t.span,
                    ));
                }
                Ok(Stmt::Match(value, arms, t.span))
            }
            _ => {
                let e = self.expr(0)?;
                if !matches!(e, Expr::Call(_, _, _, _)) {
                    return Err(diagnostic(
                        "E_STMT_EXPR",
                        "only a function call may be used as a statement",
                        e.span(),
                    ));
                }
                self.expect_newline()?;
                Ok(Stmt::Expr(e, t.span))
            }
        }
    }
    fn expr(&mut self, min: u8) -> Result<Expr, Diagnostic> {
        let t = self.advance();
        let mut left = match t.kind {
            Kind::Number if t.text.contains(['.', 'e', 'E']) => {
                let value = t
                    .text
                    .parse::<f64>()
                    .ok()
                    .and_then(Float64::new)
                    .ok_or_else(|| {
                        diagnostic(
                            "E_FLOAT_LITERAL",
                            "Float64 literal must be finite and in range",
                            t.span,
                        )
                    })?;
                Expr::Float(value, t.span)
            }
            Kind::Number => Expr::Int(
                BigInt::parse(&t.text)
                    .map_err(|_| diagnostic("E_INT_LITERAL", "invalid integer literal", t.span))?,
                t.span,
            ),
            Kind::Text => Expr::Text(t.text, t.span),
            Kind::FormatText => self.format_text(&t)?,
            Kind::Name if t.text == "true" => Expr::Bool(true, t.span),
            Kind::Name if t.text == "false" => Expr::Bool(false, t.span),
            Kind::Name if t.text == "fn" => {
                return Err(diagnostic(
                    "E_FN_NO_CLOSURES",
                    FN_NO_CLOSURES_MESSAGE,
                    t.span,
                ));
            }
            Kind::Name if t.text == "not" || t.text == "-" => {
                Expr::Unary(t.text, Box::new(self.expr(6)?), t.span)
            }
            Kind::Name => {
                if self.eat("(").is_some() {
                    let mut args = Vec::new();
                    let mut keywords = Vec::new();
                    let mut saw_keyword = false;
                    if self.cur().text != ")" {
                        loop {
                            let is_keyword = self.cur().kind == Kind::Name
                                && self
                                    .tokens
                                    .get(self.at + 1)
                                    .is_some_and(|next| next.text == "=");
                            if is_keyword {
                                let name = self.advance();
                                self.expect("=")?;
                                keywords.push((name.text, self.expr(0)?, name.span));
                                saw_keyword = true;
                            } else {
                                if saw_keyword {
                                    return Err(diagnostic(
                                        "E_PYTHON_ARGUMENT_ORDER",
                                        "positional arguments must come before keyword arguments",
                                        self.cur().span,
                                    ));
                                }
                                args.push(self.expr(0)?);
                            }
                            if self.eat(",").is_none() {
                                break;
                            }
                        }
                    }
                    self.expect(")")?;
                    Expr::Call(t.text, args, keywords, t.span)
                } else {
                    Expr::Name(t.text, t.span)
                }
            }
            Kind::Symbol if t.text == "-" => Expr::Unary(t.text, Box::new(self.expr(6)?), t.span),
            Kind::Symbol if t.text == "(" => {
                let e = self.expr(0)?;
                self.expect(")")?;
                e
            }
            Kind::Symbol if t.text == "[" => {
                let mut values = Vec::new();
                if self.cur().text != "]" {
                    loop {
                        values.push(self.expr(0)?);
                        if self.eat(",").is_none() {
                            break;
                        }
                    }
                }
                self.expect("]")?;
                Expr::List(values, t.span)
            }
            Kind::Symbol if t.text == "{" => {
                let mut values = Vec::new();
                if self.cur().text != "}" {
                    loop {
                        let key = self.expr(0)?;
                        self.expect(":")?;
                        let value = self.expr(0)?;
                        values.push((key, value));
                        if self.eat(",").is_none() {
                            break;
                        }
                    }
                }
                self.expect("}")?;
                Expr::Map(values, t.span)
            }
            _ => return Err(diagnostic("E_SYNTAX", "expected an expression", t.span)),
        };
        loop {
            if min <= 6 && self.eat("[").is_some() {
                let index = self.expr(0)?;
                self.expect("]")?;
                let span = left.span();
                left = Expr::Index(Box::new(left), Box::new(index), span);
                continue;
            }
            if min <= 6 && self.eat(".").is_some() {
                let field = self.name()?;
                let span = left.span();
                left = Expr::Field(Box::new(left), field.text, span);
                continue;
            }
            if min <= 6 && self.eat("?").is_some() {
                let span = left.span();
                left = Expr::Try(Box::new(left), span);
                continue;
            }
            let op = self.cur().text.as_str();
            let precedence = match op {
                "or" => 1,
                "and" => 2,
                "==" | "!=" | "<" | "<=" | ">" | ">=" => 3,
                "+" | "-" => 4,
                "*" | "/" | "//" | "%" => 5,
                _ => 0,
            };
            if precedence == 0 || precedence < min {
                break;
            }
            let tok = self.advance();
            let right = self.expr(precedence + 1)?;
            let span = tok.span;
            left = Expr::Binary(tok.text, Box::new(left), Box::new(right), span);
        }
        Ok(left)
    }
}

/// The function argument of a higher-order builtin: a named function the compiler resolves to
/// an index, or an expression of function type whose value is on the stack.
enum FunctionArgument {
    Named {
        index: usize,
        parameters: Vec<Type>,
        result: Type,
        span: Span,
    },
    Value {
        parameters: Vec<Type>,
        result: Type,
        span: Span,
    },
}

impl FunctionArgument {
    fn span(&self) -> Span {
        match self {
            FunctionArgument::Named { span, .. } | FunctionArgument::Value { span, .. } => *span,
        }
    }
    fn signature(&self) -> (Vec<Type>, Type) {
        match self {
            FunctionArgument::Named {
                parameters, result, ..
            }
            | FunctionArgument::Value {
                parameters, result, ..
            } => (parameters.clone(), result.clone()),
        }
    }
    fn index(&self) -> Option<usize> {
        match self {
            FunctionArgument::Named { index, .. } => Some(*index),
            FunctionArgument::Value { .. } => None,
        }
    }
}

/// Every dotted prefix, including the root name, of the target of each `set record.field = ...`
/// statement in `body` (nested blocks included): `set a.b.c = x` yields `a` and `a.b`.
fn split_targets_of(body: &[Stmt]) -> HashSet<String> {
    fn walk(body: &[Stmt], out: &mut HashSet<String>) {
        for statement in body {
            match statement {
                // `set b = Bag(list_push(b.items, x), ...)` also profits from the split.
                Stmt::Set(target, Expr::Call(_, arguments, _, _), _)
                    if !target.contains('.')
                        && arguments.iter().any(|argument| {
                            matches!(argument, Expr::Call(call, inner, _, _)
                                if call == "list_push"
                                    && inner.first().and_then(dotted_path).is_some_and(|path| {
                                        path.starts_with(&format!("{target}."))
                                    }))
                        }) =>
                {
                    out.insert(target.clone());
                }
                Stmt::Set(target, _, _) if target.contains('.') => {
                    let mut end = 0;
                    for part in target.split('.') {
                        end += part.len();
                        if end < target.len() {
                            out.insert(target[..end].to_owned());
                        }
                        end += 1;
                    }
                }
                Stmt::If(_, yes, no, _) => {
                    walk(yes, out);
                    walk(no, out);
                }
                Stmt::While(_, inner, _) | Stmt::For(_, _, inner, _) => walk(inner, out),
                Stmt::Match(_, arms, _) => {
                    for arm in arms {
                        walk(&arm.body, out);
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = HashSet::new();
    walk(body, &mut out);
    out
}

/// The dotted path an expression of names and field reads spells (`a`, `a.b`), if it is one.
fn dotted_path(expression: &Expr) -> Option<String> {
    match expression {
        Expr::Name(name, _) => Some(name.clone()),
        Expr::Field(inner, field, _) => Some(format!("{}.{field}", dotted_path(inner)?)),
        _ => None,
    }
}

/// Whether `expression` might read the variable or field at `path` (the variable itself, or any
/// field path that starts with it or is a prefix of it). Unknown shapes say yes.
fn expression_reads_path(expression: &Expr, path: &str) -> bool {
    let overlaps = |other: &str| {
        other == path
            || other.starts_with(&format!("{path}."))
            || path.starts_with(&format!("{other}."))
    };
    if let Some(spelled) = dotted_path(expression) {
        return overlaps(&spelled);
    }
    match expression {
        Expr::Int(..) | Expr::Float(..) | Expr::Bool(..) | Expr::Text(..) => false,
        Expr::Unary(_, inner, _) | Expr::Try(inner, _) => expression_reads_path(inner, path),
        Expr::Binary(_, left, right, _) | Expr::Index(left, right, _) => {
            expression_reads_path(left, path) || expression_reads_path(right, path)
        }
        Expr::Call(_, arguments, keywords, _) => {
            arguments.iter().any(|argument| expression_reads_path(argument, path))
                || keywords.iter().any(|(_, value, _)| expression_reads_path(value, path))
        }
        Expr::List(items, _) => items.iter().any(|item| expression_reads_path(item, path)),
        Expr::Map(pairs, _) => pairs.iter().any(|(key, value)| {
            expression_reads_path(key, path) || expression_reads_path(value, path)
        }),
        Expr::Field(inner, _, _) => expression_reads_path(inner, path),
        Expr::Interpolated(parts, _) => parts.iter().any(|part| match part {
            FormatPart::Literal(_) => false,
            FormatPart::Hole(inner) => expression_reads_path(inner, path),
        }),
        Expr::Name(..) => true,
    }
}

#[derive(Clone)]
struct Local {
    slot: usize,
    ty: Type,
    mutable: bool,
    /// For a record variable the program assigns fields of: the record's index. Its fields then
    /// live in hidden locals named `name.field`, and the whole record is built from them where
    /// the program uses it as a value.
    split: Option<usize>,
}
type ExternalSignature = (usize, Vec<(String, Type)>, Type);
struct FnCompiler<'a> {
    ast: &'a FunctionAst,
    signatures: &'a HashMap<String, (usize, Vec<Type>, Type)>,
    external_signatures: &'a HashMap<String, ExternalSignature>,
    records: &'a [RecordAst],
    visible_functions: &'a HashMap<String, String>,
    visible_externals: &'a HashMap<String, String>,
    visible_records: &'a HashSet<String>,
    /// Symbols of functions declared in standard-library modules.
    standard_functions: &'a HashSet<String>,
    locals: HashMap<String, Local>,
    next_local: usize,
    code: Vec<Instruction>,
    loops: Vec<LoopFrame>,
    /// Arguments of a builtin call that sit on a serialization boundary, by identity, with the
    /// boundary's name: a type holding a function value may not reach them.
    boundary_args: Vec<(*const Expr, String)>,
    /// Dotted names (`record`, `record.inner`) that some `set` statement assigns a field of; a
    /// record variable with such a name is split into one hidden local per field.
    split_targets: HashSet<String>,
}
/// Jumps emitted by `break` and `continue` inside one loop, patched when the loop ends.
#[derive(Default)]
struct LoopFrame {
    breaks: Vec<usize>,
    continues: Vec<usize>,
}
impl<'a> FnCompiler<'a> {
    fn emit(&mut self, op: Op, span: Span) -> usize {
        let i = self.code.len();
        self.code.push(Instruction { op, span });
        i
    }
    /// Refuses a private field of a record declared in another module.
    fn check_private_field(
        &self,
        record_name: &str,
        field_name: &str,
        span: Span,
    ) -> Result<(), Diagnostic> {
        if let Some(record) = self.records.iter().find(|record| record.name == record_name) {
            if record.module_id != self.ast.module_id
                && record.private_fields.iter().any(|field| field == field_name)
            {
                return Err(diagnostic(
                    "E_FIELD_PRIVATE",
                    format!("field `{field_name}` of `{record_name}` is private to the module that declares it"),
                    span,
                ));
            }
        }
        Ok(())
    }
    fn record_index_of(&self, record_name: &str) -> Option<usize> {
        self.records.iter().position(|record| record.name == record_name)
    }
    /// Spreads the record held in local `whole_slot` over the hidden field locals of `name`,
    /// creating them on first use, and recurses into field records that are assigned too.
    fn split_record(&mut self, name: &str, record_index: usize, whole_slot: usize, span: Span) {
        let fields = self.records[record_index].fields.clone();
        for (field_index, (field_name, field_type, _)) in fields.iter().enumerate() {
            let hidden = format!("{name}.{field_name}");
            let field_slot = match self.locals.get(&hidden) {
                Some(existing) => existing.slot,
                None => {
                    let slot = self.next_local;
                    self.next_local += 1;
                    slot
                }
            };
            self.emit(Op::Load(whole_slot), span);
            self.emit(Op::GetField(record_index, field_index), span);
            self.emit(Op::Store(field_slot), span);
            let nested = match field_type {
                Type::Record(inner) if self.split_targets.contains(&hidden) => {
                    self.record_index_of(inner)
                }
                _ => None,
            };
            self.locals.insert(
                hidden.clone(),
                Local {
                    slot: field_slot,
                    ty: field_type.clone(),
                    mutable: true,
                    split: nested,
                },
            );
            if let Some(inner_index) = nested {
                self.split_record(&hidden, inner_index, field_slot, span);
                self.clear_local(field_slot, span);
            }
        }
    }
    /// Drops the value a local holds so a list inside it is no longer shared with the hidden
    /// field locals that took it over.
    fn clear_local(&mut self, slot: usize, span: Span) {
        self.emit(Op::Const(Value::Unit), span);
        self.emit(Op::Store(slot), span);
    }
    /// Pushes the record variable `name` as a value, built from its hidden field locals.
    fn materialize_record(&mut self, name: &str, record_index: usize, span: Span) {
        let fields = self.records[record_index].fields.clone();
        for (field_name, _, _) in &fields {
            let hidden = format!("{name}.{field_name}");
            let local = self.locals.get(&hidden).cloned().expect("hidden field local");
            match local.split {
                Some(inner) => self.materialize_record(&hidden, inner, span),
                None => {
                    self.emit(Op::Load(local.slot), span);
                }
            }
        }
        self.emit(Op::MakeRecord(record_index, fields.len()), span);
    }
    /// `set name = value` for a record variable that is split into field locals. A record
    /// constructor is assigned field by field, so `Bag(list_push(b.items, x), ...)` appends in
    /// place; any other value is spread over the field locals.
    fn assign_split_record(
        &mut self,
        name: &str,
        record_index: usize,
        value: &Expr,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let record = self.records[record_index].clone();
        if let Expr::Call(constructor, args, keywords, _) = value {
            if *constructor == record.name && keywords.is_empty() && args.len() == record.fields.len()
            {
                // An argument `list_push(name.field, x)` for its own field is applied in place
                // when no other argument reads that field or the whole record.
                let in_place = |index: usize, argument: &Expr| -> Option<Expr> {
                    let (field_name, _, _) = &record.fields[index];
                    let hidden = format!("{name}.{field_name}");
                    let Expr::Call(call, call_args, call_keywords, _) = argument else {
                        return None;
                    };
                    if call != "list_push"
                        || !call_keywords.is_empty()
                        || call_args.len() != 2
                        || dotted_path(&call_args[0]).as_deref() != Some(hidden.as_str())
                        || expression_reads_path(&call_args[1], &hidden)
                        || args
                            .iter()
                            .enumerate()
                            .any(|(other, other_argument)| {
                                other != index && expression_reads_path(other_argument, &hidden)
                            })
                    {
                        return None;
                    }
                    Some(call_args[1].clone())
                };
                let mut pending = Vec::new();
                for (index, (argument, (_, expected, _))) in
                    args.iter().zip(&record.fields).enumerate()
                {
                    let (field_name, _, _) = &record.fields[index];
                    let hidden = format!("{name}.{field_name}");
                    let local = self.locals.get(&hidden).cloned().expect("hidden field local");
                    if local.split.is_none() {
                        if let (Some(pushed), Type::List(item)) = (in_place(index, argument), expected)
                        {
                            let actual = self.expr_with_expected(&pushed, Some(item.as_ref()))?;
                            if actual != **item {
                                return Err(diagnostic(
                                    "E_TYPE_ARGUMENT",
                                    "list_push value must match the List item type",
                                    pushed.span(),
                                ));
                            }
                            self.emit(Op::ListPushLocal(local.slot), span);
                            continue;
                        }
                    }
                    let actual = self.expr_with_expected(argument, Some(expected))?;
                    if actual != *expected {
                        return Err(diagnostic(
                            "E_TYPE_ARGUMENT",
                            format!(
                                "field of `{}` expects {expected:?}, got {actual:?}",
                                record.name
                            ),
                            argument.span(),
                        ));
                    }
                    pending.push((index, local));
                }
                for (_, local) in pending.into_iter().rev() {
                    match local.split {
                        Some(inner) => {
                            // A field record that is itself split: spread the new value.
                            let temporary = self.next_local;
                            self.next_local += 1;
                            self.emit(Op::Store(temporary), span);
                            let hidden = self
                                .locals
                                .iter()
                                .find(|(_, candidate)| candidate.slot == local.slot)
                                .map(|(key, _)| key.clone())
                                .expect("hidden field name");
                            self.split_record(&hidden, inner, temporary, span);
                            self.clear_local(temporary, span);
                        }
                        None => {
                            self.emit(Op::Store(local.slot), span);
                        }
                    }
                }
                return Ok(());
            }
        }
        let ty = self.expr_with_expected(value, Some(&Type::Record(record.name.clone())))?;
        if ty != Type::Record(record.name.clone()) {
            return Err(diagnostic(
                "E_TYPE_ASSIGN",
                format!("`{name}` holds {:?}, but this expression is {ty:?}", Type::Record(record.name.clone())),
                span,
            ));
        }
        let temporary = self.next_local;
        self.next_local += 1;
        self.emit(Op::Store(temporary), span);
        self.split_record(name, record_index, temporary, span);
        self.clear_local(temporary, span);
        Ok(())
    }
    fn patch(&mut self, i: usize, target: usize) {
        match &mut self.code[i].op {
            Op::Jump(x) | Op::JumpIfFalse(x) => *x = target,
            _ => unreachable!(),
        }
    }
    fn expr(&mut self, e: &Expr) -> Result<Type, Diagnostic> {
        self.expr_with_expected(e, None)
    }

    /// Resolve the function argument of a higher-order builtin. A bare name that is not a local
    /// is a named function and emits nothing (the operation carries its index); anything else
    /// must have a function type, and its value is left on the stack.
    fn function_argument(
        &mut self,
        arg: &Expr,
        not_a_function: (&str, &str),
        label: &str,
    ) -> Result<FunctionArgument, Diagnostic> {
        if let Expr::Name(name, span) = arg {
            if !self.locals.contains_key(name) {
                let Some((index, parameters, result)) = self
                    .visible_functions
                    .get(name)
                    .and_then(|symbol| self.signatures.get(symbol))
                    .cloned()
                else {
                    return Err(diagnostic(
                        "E_NAME_UNKNOWN",
                        format!("unknown {label} `{name}`"),
                        *span,
                    ));
                };
                return Ok(FunctionArgument::Named {
                    index,
                    parameters,
                    result,
                    span: *span,
                });
            }
        }
        match self.expr(arg)? {
            Type::Fn(parameters, result) => Ok(FunctionArgument::Value {
                parameters,
                result: *result,
                span: arg.span(),
            }),
            _ => Err(diagnostic(not_a_function.0, not_a_function.1, arg.span())),
        }
    }

    fn catalog_fixed_builtin(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
    ) -> Result<Option<Type>, Diagnostic> {
        let Some(signature) = builtin_catalog::get(name) else {
            return Ok(None);
        };
        let Some(op) = builtin_catalog::operation(&signature.operation) else {
            return Ok(None);
        };
        let Some(parameters) = signature
            .parameters
            .iter()
            .map(|spec| builtin_catalog::concrete_type(spec))
            .collect::<Option<Vec<_>>>()
        else {
            return Ok(None);
        };
        let Some(result) = builtin_catalog::concrete_type(&signature.result) else {
            return Ok(None);
        };
        if args.len() != parameters.len() {
            return Err(diagnostic(
                "E_CALL_ARITY",
                format!(
                    "builtin `{name}` expects {} arguments, got {}",
                    parameters.len(),
                    args.len()
                ),
                span,
            ));
        }
        for (argument, expected) in args.iter().zip(parameters.iter()) {
            let actual = self.expr_with_expected(argument, Some(expected))?;
            if actual != *expected {
                return Err(diagnostic(
                    "E_TYPE_ARGUMENT",
                    format!("argument to `{name}` expects {expected:?}, got {actual:?}"),
                    argument.span(),
                ));
            }
        }
        self.emit(op, span);
        Ok(Some(result))
    }

    /// Types a generic array builtin from its catalog signature. The first parameter that
    /// mentions an element variable binds it from the argument's `Array[e]` type; later
    /// parameters and the result are checked after substitution.
    fn catalog_array_builtin(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
    ) -> Result<Option<Type>, Diagnostic> {
        let Some(signature) = builtin_catalog::get(name) else {
            return Ok(None);
        };
        let Some((function, fixed)) = builtin_catalog::array_operation(&signature.operation) else {
            return Ok(None);
        };
        let variable = ["$A", "$N", "$F"].into_iter().find(|variable| {
            signature
                .parameters
                .iter()
                .any(|spec| spec.contains(variable))
        });
        // Every type this signature can produce must resolve before any argument is compiled:
        // returning `Ok(None)` after emitting argument code would leave that code behind.
        if (variable.is_none() && fixed.is_none()) || args.len() != signature.parameters.len() {
            return Ok(None);
        }
        let candidates: Vec<Option<guion_model::ElementType>> = match variable {
            Some(variable) => guion_model::ElementType::ALL
                .into_iter()
                .filter(|element| {
                    builtin_catalog::element_variable_accepts(variable, *element) == Some(true)
                })
                .map(Some)
                .collect(),
            None => vec![None],
        };
        for candidate in candidates {
            let resolves = signature
                .parameters
                .iter()
                .chain([&signature.result])
                .all(|spec| {
                    let spec = match (variable, candidate) {
                        (Some(variable), Some(element)) => {
                            builtin_catalog::substitute_element(spec, variable, element)
                        }
                        _ => spec.clone(),
                    };
                    builtin_catalog::concrete_type(&spec).is_some()
                });
            if !resolves {
                return Ok(None);
            }
        }
        let mut bound: Option<guion_model::ElementType> = None;
        for (argument, spec) in args.iter().zip(&signature.parameters) {
            let expected = match (variable, bound) {
                (Some(variable), Some(element)) => {
                    Some(builtin_catalog::substitute_element(spec, variable, element))
                }
                (Some(variable), None) if spec.contains(variable) => None,
                _ => Some(spec.clone()),
            };
            let Some(expected) = expected else {
                let actual = self.expr_with_expected(argument, None)?;
                let element = match (&actual, spec.starts_with("List[")) {
                    (Type::Array(element), false) => Some(*element),
                    (Type::List(item), true) => match item.as_ref() {
                        Type::Array(element) => Some(*element),
                        _ => None,
                    },
                    _ => None,
                };
                let Some(element) = element else {
                    let wanted = if spec.starts_with("List[") {
                        "a List of arrays"
                    } else {
                        "an array"
                    };
                    return Err(diagnostic(
                        "E_TYPE_ARGUMENT",
                        format!("argument to `{name}` must be {wanted}, got {actual:?}"),
                        argument.span(),
                    ));
                };
                let variable = variable.expect("a binding parameter names its variable");
                if builtin_catalog::element_variable_accepts(variable, element) != Some(true) {
                    let class = if variable == "$F" {
                        "a float (f32 or f64)"
                    } else {
                        "a numeric"
                    };
                    return Err(diagnostic(
                        "E_TYPE_ARGUMENT",
                        format!(
                            "`{name}` requires {class} array, got Array[{}]",
                            element.name()
                        ),
                        argument.span(),
                    ));
                }
                bound = Some(element);
                continue;
            };
            let expected = builtin_catalog::concrete_type(&expected)
                .expect("signature types were resolved before compiling arguments");
            let actual = self.expr_with_expected(argument, Some(&expected))?;
            if actual != expected {
                return Err(diagnostic(
                    "E_TYPE_ARGUMENT",
                    format!("argument to `{name}` expects {expected:?}, got {actual:?}"),
                    argument.span(),
                ));
            }
        }
        let element = fixed
            .or(bound)
            .expect("an array builtin has a fixed element or binds one from an argument");
        let result = match (variable, bound) {
            (Some(variable), Some(bound)) => {
                builtin_catalog::substitute_element(&signature.result, variable, bound)
            }
            _ => signature.result.clone(),
        };
        let result = builtin_catalog::concrete_type(&result)
            .expect("signature types were resolved before compiling arguments");
        self.emit(Op::Array(function, element), span);
        Ok(Some(result))
    }

    /// Whether a type is, or holds, a function value, looking through record fields.
    fn type_has_fn(&self, ty: &Type) -> bool {
        fn walk(ty: &Type, records: &[RecordAst], seen: &mut Vec<String>) -> bool {
            match ty {
                Type::Fn(_, _) => true,
                Type::List(item) | Type::Map(item) | Type::Option(item) => {
                    walk(item, records, seen)
                }
                Type::Result(ok, err) => walk(ok, records, seen) || walk(err, records, seen),
                Type::Record(name) => {
                    if seen.contains(name) {
                        return false;
                    }
                    seen.push(name.clone());
                    records
                        .iter()
                        .find(|record| record.name == *name)
                        .is_some_and(|record| {
                            record
                                .fields
                                .iter()
                                .any(|(_, field, _)| walk(field, records, seen))
                        })
                }
                _ => false,
            }
        }
        walk(ty, self.records, &mut Vec::new())
    }

    fn expr_with_expected(
        &mut self,
        e: &Expr,
        expected: Option<&Type>,
    ) -> Result<Type, Diagnostic> {
        let mark = self.boundary_args.len();
        let result = self.expr_with_expected_inner(e, expected);
        self.boundary_args.truncate(mark);
        let ty = result?;
        if mark > 0 {
            let key = e as *const Expr;
            if let Some((_, boundary)) = self.boundary_args[..mark]
                .iter()
                .find(|(pointer, _)| *pointer == key)
            {
                if self.type_has_fn(&ty) {
                    return Err(diagnostic(
                        "E_FN_NOT_SERIALIZABLE",
                        format!("a type that holds a function value cannot cross {boundary}"),
                        e.span(),
                    ));
                }
            }
        }
        Ok(ty)
    }

    fn expr_with_expected_inner(
        &mut self,
        e: &Expr,
        expected: Option<&Type>,
    ) -> Result<Type, Diagnostic> {
        match e {
            Expr::Int(i, s) => {
                self.emit(Op::Const(Value::Int(i.clone())), *s);
                Ok(Type::Int)
            }
            Expr::Float(value, s) => {
                self.emit(Op::Const(Value::Float64(*value)), *s);
                Ok(Type::Float64)
            }
            Expr::Bool(b, s) => {
                self.emit(Op::Const(Value::Bool(*b)), *s);
                Ok(Type::Bool)
            }
            Expr::Text(v, s) => {
                self.emit(Op::Const(Value::Text(v.clone().into())), *s);
                Ok(Type::Text)
            }
            Expr::List(values, span) => {
                let expected_item = match expected {
                    Some(Type::List(item)) => Some(item.as_ref()),
                    _ => None,
                };
                let Some(first) = values.first() else {
                    let Some(item_type) = expected_item else {
                        return Err(diagnostic(
                            "E_TYPE_LIST_EMPTY",
                            "an empty list needs an explicit List[T] type",
                            *span,
                        ));
                    };
                    self.emit(Op::MakeTypedList(item_type.clone()), *span);
                    return Ok(Type::List(Box::new(item_type.clone())));
                };
                let item_type = self.expr_with_expected(first, expected_item)?;
                for value in values.iter().skip(1) {
                    let actual = self.expr_with_expected(value, expected_item)?;
                    if actual != item_type {
                        return Err(diagnostic(
                            "E_TYPE_LIST_ITEM",
                            "all list items must have the same type",
                            value.span(),
                        ));
                    }
                }
                self.emit(Op::MakeList(values.len()), *span);
                Ok(Type::List(Box::new(item_type)))
            }
            Expr::Map(values, span) => {
                let expected_item = match expected {
                    Some(Type::Map(item)) => Some(item.as_ref()),
                    _ => None,
                };
                if values.is_empty() {
                    let Some(item_type) = expected_item else {
                        return Err(diagnostic(
                            "E_TYPE_MAP_EMPTY",
                            "an empty map needs an explicit Map[Text, T] type",
                            *span,
                        ));
                    };
                    self.emit(Op::MakeTypedMap(item_type.clone()), *span);
                    return Ok(Type::Map(Box::new(item_type.clone())));
                }
                let mut seen_keys = std::collections::HashSet::new();
                let mut item_type = None;
                for (key, value) in values {
                    let Expr::Text(key_text, key_span) = key else {
                        return Err(diagnostic(
                            "E_TYPE_MAP_KEY",
                            "map literal keys must be Text literals",
                            key.span(),
                        ));
                    };
                    if !seen_keys.insert(key_text) {
                        return Err(diagnostic(
                            "E_MAP_DUPLICATE_KEY",
                            format!("duplicate map key `{key_text}`"),
                            *key_span,
                        ));
                    }
                    self.emit(Op::Const(Value::Text(key_text.clone().into())), *key_span);
                    let actual = self.expr_with_expected(value, expected_item)?;
                    if item_type
                        .as_ref()
                        .is_some_and(|expected| expected != &actual)
                    {
                        return Err(diagnostic(
                            "E_TYPE_MAP_VALUE",
                            "all map values must have the same type",
                            value.span(),
                        ));
                    }
                    item_type = Some(actual);
                }
                self.emit(Op::MakeMap(values.len()), *span);
                Ok(Type::Map(Box::new(item_type.unwrap())))
            }
            Expr::Try(result, span) => {
                let Type::Result(ok, err) = self.expr(result)? else {
                    return Err(diagnostic(
                        "E_RESULT_TRY_TYPE",
                        "`?` requires a Result expression",
                        *span,
                    ));
                };
                let Type::Result(_, function_error) = &self.ast.result else {
                    return Err(diagnostic(
                        "E_RESULT_TRY_FUNCTION",
                        "`?` can only be used in a function that returns Result",
                        *span,
                    ));
                };
                if err != *function_error {
                    return Err(diagnostic(
                        "E_RESULT_TRY_ERROR",
                        "the error type must match the enclosing function's Result error type",
                        *span,
                    ));
                }
                self.emit(Op::TryResult, *span);
                Ok(*ok)
            }
            Expr::Index(values, index, span) => {
                let item_type = match self.expr(values)? {
                    Type::List(item) => {
                        if self.expr(index)? != Type::Int {
                            return Err(diagnostic(
                                "E_TYPE_INDEX",
                                "list index must be Int",
                                index.span(),
                            ));
                        }
                        *item
                    }
                    Type::Map(item) => {
                        if self.expr(index)? != Type::Text {
                            return Err(diagnostic(
                                "E_TYPE_INDEX",
                                "map index must be Text",
                                index.span(),
                            ));
                        }
                        *item
                    }
                    Type::Text => {
                        if self.expr(index)? != Type::Int {
                            return Err(diagnostic(
                                "E_TYPE_INDEX",
                                "text index must be Int",
                                index.span(),
                            ));
                        }
                        Type::Text
                    }
                    _ => {
                        return Err(diagnostic(
                            "E_TYPE_INDEX",
                            "indexing requires a List, Map, or Text",
                            values.span(),
                        ))
                    }
                };
                self.emit(Op::Index, *span);
                Ok(item_type)
            }
            Expr::Field(value, field_name, span) => {
                if let Some(path) = dotted_path(value) {
                    let hidden = format!("{path}.{field_name}");
                    if self.locals.get(&path).is_some_and(|local| local.split.is_some()) {
                        if let Some(Local { ty: Type::Record(record_name), .. }) =
                            self.locals.get(&path)
                        {
                            self.check_private_field(record_name, field_name, *span)?;
                        }
                        if let Some(local) = self.locals.get(&hidden).cloned() {
                            match local.split {
                                Some(inner) => self.materialize_record(&hidden, inner, *span),
                                None => {
                                    self.emit(Op::Load(local.slot), *span);
                                }
                            }
                            return Ok(local.ty);
                        }
                    }
                }
                let Type::Record(record_name) = self.expr(value)? else {
                    return Err(diagnostic(
                        "E_TYPE_FIELD",
                        "field access requires a record value",
                        value.span(),
                    ));
                };
                if !self.visible_records.contains(&record_name) {
                    return Err(diagnostic(
                        "E_NAME_UNKNOWN",
                        format!("record type `{record_name}` is not imported into this module"),
                        *span,
                    ));
                }
                let Some((record_index, record)) = self
                    .records
                    .iter()
                    .enumerate()
                    .find(|(_, record)| record.name == record_name)
                else {
                    return Err(diagnostic(
                        "E_TYPE_UNKNOWN",
                        format!("unknown record type `{record_name}`"),
                        *span,
                    ));
                };
                let Some((field_index, (_, field_type, _))) = record
                    .fields
                    .iter()
                    .enumerate()
                    .find(|(_, (name, _, _))| name == field_name)
                else {
                    return Err(diagnostic(
                        "E_FIELD_UNKNOWN",
                        format!("record `{record_name}` has no field `{field_name}`"),
                        *span,
                    ));
                };
                self.check_private_field(&record_name, field_name, *span)?;
                self.emit(Op::GetField(record_index, field_index), *span);
                Ok(field_type.clone())
            }
            Expr::Name(n, s) => {
                if n == "None" {
                    let option_type = match expected {
                        Some(option @ Type::Option(_)) => option,
                        None if matches!(self.ast.result, Type::Option(_)) => &self.ast.result,
                        _ => {
                            return Err(diagnostic(
                                "E_OPTION_CONSTRUCTOR_CONTEXT",
                                "None requires an expected Option type or an Option-returning function",
                                *s,
                            ));
                        }
                    };
                    if *option_type != self.ast.result {
                        return Err(diagnostic(
                            "E_OPTION_CONSTRUCTOR_CONTEXT",
                            "None is only supported in a function that returns that Option type; build it in a small helper function that returns it",
                            *s,
                        ));
                    }
                    self.emit(Op::MakeNone, *s);
                    return Ok(option_type.clone());
                }
                let Some(l) = self.locals.get(n).cloned() else {
                    // A bare function name denotes the function value; a local of the same
                    // name wins.
                    if let Some((index, parameters, result)) = self
                        .visible_functions
                        .get(n)
                        .and_then(|symbol| self.signatures.get(symbol))
                        .cloned()
                    {
                        self.emit(Op::MakeFn(index), *s);
                        return Ok(Type::Fn(parameters, Box::new(result)));
                    }
                    return Err(diagnostic(
                        "E_NAME_UNKNOWN",
                        format!("unknown name `{n}`"),
                        *s,
                    ));
                };
                if let Some(record_index) = l.split {
                    self.materialize_record(n, record_index, *s);
                    return Ok(l.ty);
                }
                self.emit(Op::Load(l.slot), *s);
                Ok(l.ty)
            }
            Expr::Unary(op, v, s) => {
                let ty = self.expr(v)?;
                match (op.as_str(), ty) {
                    ("-", Type::Int) => {
                        self.emit(Op::Negate, *s);
                        Ok(Type::Int)
                    }
                    ("-", Type::Float64) => {
                        self.emit(Op::Negate, *s);
                        Ok(Type::Float64)
                    }
                    ("not", Type::Bool) => {
                        self.emit(Op::Not, *s);
                        Ok(Type::Bool)
                    }
                    _ => Err(diagnostic(
                        "E_TYPE_OPERATOR",
                        format!("operator `{op}` does not accept this value"),
                        *s,
                    )),
                }
            }
            Expr::Binary(op, a, b, s) => {
                if op == "and" || op == "or" {
                    let lt = self.expr(a)?;
                    if lt != Type::Bool {
                        return Err(diagnostic(
                            "E_TYPE_OPERATOR",
                            "logical operators require Bool values",
                            a.span(),
                        ));
                    }
                    let jump = self.emit(Op::JumpIfFalse(0), *s);
                    if op == "or" {
                        self.emit(Op::Const(Value::Bool(true)), *s);
                        let end = self.emit(Op::Jump(0), *s);
                        let rhs = self.code.len();
                        self.patch(jump, rhs);
                        let rt = self.expr(b)?;
                        if rt != Type::Bool {
                            return Err(diagnostic(
                                "E_TYPE_OPERATOR",
                                "logical operators require Bool values",
                                b.span(),
                            ));
                        }
                        let end_pc = self.code.len();
                        self.patch(end, end_pc);
                    } else {
                        let rt = self.expr(b)?;
                        if rt != Type::Bool {
                            return Err(diagnostic(
                                "E_TYPE_OPERATOR",
                                "logical operators require Bool values",
                                b.span(),
                            ));
                        }
                        let end = self.emit(Op::Jump(0), *s);
                        let false_pc = self.code.len();
                        self.patch(jump, false_pc);
                        self.emit(Op::Const(Value::Bool(false)), *s);
                        let end_pc = self.code.len();
                        self.patch(end, end_pc);
                    }
                    return Ok(Type::Bool);
                }
                let lt = self.expr(a)?;
                let rt = self.expr(b)?;
                if (op == "==" || op == "!=") && (self.type_has_fn(&lt) || self.type_has_fn(&rt)) {
                    return Err(diagnostic(
                        "E_FN_COMPARE",
                        "function values cannot be compared",
                        *s,
                    ));
                }
                let op_code = match op.as_str() {
                    "+" if lt == Type::Int && rt == Type::Int => Some(Op::Add),
                    "+" if lt == Type::Text && rt == Type::Text => Some(Op::Add),
                    "+" if lt == Type::Float64 && rt == Type::Float64 => Some(Op::Add),
                    "-" if lt == Type::Int && rt == Type::Int => Some(Op::Subtract),
                    "-" if lt == Type::Float64 && rt == Type::Float64 => Some(Op::Subtract),
                    "*" if lt == Type::Int && rt == Type::Int => Some(Op::Multiply),
                    "*" if lt == Type::Float64 && rt == Type::Float64 => Some(Op::Multiply),
                    "/" if lt == Type::Float64 && rt == Type::Float64 => Some(Op::Divide),
                    "/" if lt == Type::Int && rt == Type::Int => Some(Op::IntegerDivide),
                    "//" if lt == Type::Int && rt == Type::Int => Some(Op::FloorDivide),
                    "%" if lt == Type::Int && rt == Type::Int => Some(Op::Remainder),
                    "==" if lt == rt => Some(Op::Equal),
                    "!=" if lt == rt => Some(Op::NotEqual),
                    "<" if lt == rt && matches!(lt, Type::Int | Type::Text | Type::Float64) => {
                        Some(Op::Less)
                    }
                    "<=" if lt == rt && matches!(lt, Type::Int | Type::Text | Type::Float64) => {
                        Some(Op::LessEqual)
                    }
                    ">" if lt == rt && matches!(lt, Type::Int | Type::Text | Type::Float64) => {
                        Some(Op::Greater)
                    }
                    ">=" if lt == rt && matches!(lt, Type::Int | Type::Text | Type::Float64) => {
                        Some(Op::GreaterEqual)
                    }
                    _ => None,
                };
                let Some(code) = op_code else {
                    return Err(diagnostic(
                        "E_TYPE_OPERATOR",
                        format!("operator `{op}` does not accept these operand types"),
                        *s,
                    ));
                };
                self.emit(code, *s);
                if ["==", "!=", "<", "<=", ">", ">="].contains(&op.as_str()) {
                    Ok(Type::Bool)
                } else {
                    Ok(lt)
                }
            }
            Expr::Interpolated(parts, span) => {
                let mut converted: Vec<Expr> = Vec::new();
                for part in parts {
                    match part {
                        FormatPart::Literal(text) => converted.push(Expr::Text(text.clone(), *span)),
                        FormatPart::Hole(hole) => {
                            // Type the hole, then drop the code: the converted call below is
                            // what gets compiled.
                            let mark = self.code.len();
                            let ty = self.expr(hole)?;
                            self.code.truncate(mark);
                            converted.push(match ty {
                                Type::Text => hole.clone(),
                                Type::Int => Expr::Call(
                                    "int_to_text".to_owned(),
                                    vec![hole.clone()],
                                    Vec::new(),
                                    *span,
                                ),
                                Type::Float64 => Expr::Call(
                                    "float_to_text".to_owned(),
                                    vec![hole.clone()],
                                    Vec::new(),
                                    *span,
                                ),
                                other => {
                                    return Err(diagnostic(
                                        "E_FSTRING_TYPE",
                                        format!("a hole in f-text holds Int, Float64 or Text, not {other:?}; convert it first"),
                                        hole.span(),
                                    ))
                                }
                            });
                        }
                    }
                }
                let mut chain = converted.remove(0);
                for next in converted {
                    chain = Expr::Binary("+".to_owned(), Box::new(chain), Box::new(next), *span);
                }
                self.expr_with_expected(&chain, expected)
            }
            Expr::Call(name, _, _, s) if name == "range" && !self.visible_functions.contains_key(name) => {
                Err(diagnostic(
                    "E_RANGE_CONTEXT",
                    "`range(...)` is only valid as the sequence of a `for` loop: `for i in range(3):`; to get a list use a `while` loop",
                    *s,
                ))
            }
            Expr::Call(name, args, keywords, s) => {
                // A local of function type is called through its value; a local of any other
                // type leaves the name to the functions and builtins as before.
                if let Some(local) = self.locals.get(name).cloned() {
                    if let Type::Fn(parameters, result) = &local.ty {
                        if args.len() != parameters.len() {
                            return Err(diagnostic(
                                "E_CALL_ARITY",
                                format!(
                                    "function value `{name}` expects {} arguments, got {}",
                                    parameters.len(),
                                    args.len()
                                ),
                                *s,
                            ));
                        }
                        if !keywords.is_empty() {
                            return Err(diagnostic(
                                "E_CALL_KEYWORD",
                                "keyword arguments are supported only for declared Python calls",
                                keywords[0].2,
                            ));
                        }
                        for (arg, expected) in args.iter().zip(parameters.iter()) {
                            let actual = self.expr_with_expected(arg, Some(expected))?;
                            if actual != *expected {
                                return Err(diagnostic(
                                    "E_TYPE_ARGUMENT",
                                    format!(
                                        "argument to `{name}` expects {expected:?}, got {actual:?}"
                                    ),
                                    arg.span(),
                                ));
                            }
                        }
                        self.emit(Op::Load(local.slot), *s);
                        self.emit(Op::CallFn(args.len()), *s);
                        return Ok(result.as_ref().clone());
                    }
                }
                if let Some(signature) = builtin_catalog::get(name) {
                    for (position, arg) in args.iter().enumerate() {
                        if let Some(parameter) = signature.parameters.get(position) {
                            if !(parameter.contains('$')
                                || parameter.contains("fn(")
                                || parameter.contains("Named")
                                || parameter == "Sized")
                            {
                                self.boundary_args
                                    .push((arg as *const Expr, boundary_label(name)));
                            }
                        }
                    }
                    if args.len() != signature.parameters.len() {
                        return Err(diagnostic(
                            "E_CALL_ARITY",
                            format!(
                                "builtin `{name}` expects {} arguments, got {}",
                                signature.parameters.len(),
                                args.len()
                            ),
                            *s,
                        ));
                    }
                    if !keywords.is_empty() {
                        return Err(diagnostic(
                            "E_CALL_KEYWORD",
                            "keyword arguments are supported only for declared Python calls",
                            keywords[0].2,
                        ));
                    }
                    if let Some(result) = self.catalog_fixed_builtin(name, args, *s)? {
                        return Ok(result);
                    }
                    if let Some(result) = self.catalog_array_builtin(name, args, *s)? {
                        return Ok(result);
                    }
                }
                if !keywords.is_empty()
                    && [
                        "Some",
                        "Ok",
                        "Err",
                        "read_text",
                        "write_text",
                        "read_bytes",
                        "write_bytes",
                        "len",
                        "map_set",
                        "map_record_get",
                        "map_record_set",
                        "map_has",
                        "map_get",
                        "list_push",
                        "list_set",
                        "list_get",
                        "sqrt",
                        "print",
                        "eprint",
                        "http_request_with_timeout",
                    ]
                    .contains(&name.as_str())
                {
                    return Err(diagnostic(
                        "E_CALL_KEYWORD",
                        "keyword arguments are supported only for declared Python calls",
                        keywords[0].2,
                    ));
                }
                if name == "Some" {
                    if args.len() != 1 {
                        return Err(diagnostic("E_CALL_ARITY", "Some expects one value", *s));
                    }
                    let option_type = match expected {
                        Some(option @ Type::Option(_)) => option.clone(),
                        None => self.ast.result.clone(),
                        _ => Type::Unit,
                    };
                    let Type::Option(item) = &option_type else {
                        return Err(diagnostic(
                            "E_OPTION_CONSTRUCTOR_CONTEXT",
                            "Some requires an expected Option type or an Option-returning function",
                            *s,
                        ));
                    };
                    if self.expr_with_expected(&args[0], Some(item))? != **item {
                        return Err(diagnostic(
                            "E_OPTION_CONSTRUCTOR_TYPE",
                            format!("Some value must have type {item:?}"),
                            args[0].span(),
                        ));
                    }
                    if option_type != self.ast.result {
                        return Err(diagnostic(
                            "E_OPTION_CONSTRUCTOR_CONTEXT",
                            "Some is only supported in a function that returns that Option type; build it in a small helper function that returns it",
                            *s,
                        ));
                    }
                    self.emit(Op::MakeSome, *s);
                    return Ok(option_type);
                }
                if name == "Ok" || name == "Err" {
                    if args.len() != 1 {
                        return Err(diagnostic(
                            "E_CALL_ARITY",
                            format!("{name} expects one value"),
                            *s,
                        ));
                    }
                    let result_type = match expected {
                        Some(result @ Type::Result(_, _)) => result.clone(),
                        None => self.ast.result.clone(),
                        _ => Type::Unit,
                    };
                    let Type::Result(ok, err) = &result_type else {
                        return Err(diagnostic(
                            "E_RESULT_CONSTRUCTOR_CONTEXT",
                            "Ok and Err require an expected Result type or a Result-returning function",
                            *s,
                        ));
                    };
                    let payload_type = if name == "Ok" {
                        ok.as_ref()
                    } else {
                        err.as_ref()
                    };
                    if self.expr_with_expected(&args[0], Some(payload_type))? != *payload_type {
                        return Err(diagnostic(
                            "E_RESULT_CONSTRUCTOR_TYPE",
                            format!("{name} value must have type {payload_type:?}"),
                            args[0].span(),
                        ));
                    }
                    if result_type != self.ast.result {
                        return Err(diagnostic(
                            "E_RESULT_CONSTRUCTOR_CONTEXT",
                            format!("{name} is only supported in a function that returns that Result type; build it in a small helper function that returns it"),
                            *s,
                        ));
                    }
                    self.emit(
                        if name == "Ok" {
                            Op::MakeOk
                        } else {
                            Op::MakeErr
                        },
                        *s,
                    );
                    return Ok(result_type);
                }
                if name == "parallel_map" {
                    if args.len() != 3 {
                        return Err(diagnostic("E_CALL_ARITY", "parallel_map expects a list, a unary function name, and a worker limit", *s));
                    }
                    let item_type = match self.expr(&args[0])? {
                        Type::List(item) => *item,
                        _ => {
                            return Err(diagnostic(
                                "E_TYPE_ARGUMENT",
                                "parallel_map first argument must be a List",
                                args[0].span(),
                            ))
                        }
                    };
                    let worker = self.function_argument(
                        &args[1],
                        (
                            "E_TYPE_ARGUMENT",
                            "parallel_map worker must be a named function or a function value",
                        ),
                        "worker function",
                    )?;
                    let worker_span = worker.span();
                    let (worker_params, worker_result) = worker.signature();
                    if worker_params.iter().any(type_contains_native_resource)
                        || type_contains_native_resource(&worker_result)
                    {
                        return Err(diagnostic(
                            "E_PARALLEL_RESOURCE",
                            "parallel workers cannot accept or return native resources",
                            worker_span,
                        ));
                    }
                    if worker_params != vec![item_type] {
                        return Err(diagnostic(
                            "E_TYPE_ARGUMENT",
                            "parallel_map worker must accept one item of the list's item type",
                            worker_span,
                        ));
                    }
                    if self.expr(&args[2])? != Type::Int {
                        return Err(diagnostic(
                            "E_TYPE_ARGUMENT",
                            "parallel_map worker limit must be Int",
                            args[2].span(),
                        ));
                    }
                    match worker.index() {
                        Some(worker_index) => self.emit(Op::ParallelMap(worker_index), *s),
                        None => self.emit(Op::HigherOrder(HigherOrderKind::ParallelMap), *s),
                    };
                    return Ok(Type::Result(
                        Box::new(Type::List(Box::new(worker_result))),
                        Box::new(Type::TaskError),
                    ));
                }
                if name == "sort_by" {
                    if args.len() != 2 {
                        return Err(diagnostic(
                            "E_CALL_ARITY",
                            "sort_by expects a list and a named key function or comparator",
                            *s,
                        ));
                    }
                    let item_type = match self.expr(&args[0])? {
                        Type::List(item) => *item,
                        _ => {
                            return Err(diagnostic(
                                "E_TYPE_ARGUMENT",
                                "sort_by first argument must be a List",
                                args[0].span(),
                            ))
                        }
                    };
                    let comparator = self.function_argument(
                        &args[1],
                        (
                            "E_SORT_COMPARATOR",
                            "sort_by comparator must be a named function or a function value",
                        ),
                        "comparator",
                    )?;
                    let comparator_span = &comparator.span();
                    let (parameters, result) = comparator.signature();
                    // A named function with one parameter is a key function `(T) -> K`
                    // (artifact format 65); with two it is the comparator `(T, T) -> Int`
                    // that format 36 introduced and that keeps compiling to `SortBy`.
                    if parameters.len() == 1 {
                        if parameters != vec![item_type.clone()] {
                            return Err(diagnostic(
                                "E_SORT_KEY",
                                "sort_by key function must have type (T) -> K for the list item type",
                                *comparator_span,
                            ));
                        }
                        if !matches!(result, Type::Text | Type::Int | Type::Float64) {
                            return Err(diagnostic(
                                "E_SORT_KEY",
                                "sort_by key function must return Text, Int or Float64",
                                *comparator_span,
                            ));
                        }
                        match comparator.index() {
                            Some(index) => self.emit(Op::SortByKey(index), *s),
                            None => self.emit(Op::HigherOrder(HigherOrderKind::SortByKey), *s),
                        };
                        return Ok(Type::List(Box::new(item_type)));
                    }
                    if parameters != vec![item_type.clone(), item_type.clone()]
                        || result != Type::Int
                    {
                        return Err(diagnostic("E_SORT_COMPARATOR", "sort_by comparator must have type (T, T) -> Int for the list item type", *comparator_span));
                    }
                    match comparator.index() {
                        Some(index) => self.emit(Op::SortBy(index), *s),
                        None => self.emit(Op::HigherOrder(HigherOrderKind::SortBy), *s),
                    };
                    return Ok(Type::List(Box::new(item_type)));
                }
                if name == "sort" {
                    if args.len() != 1 {
                        return Err(diagnostic(
                            "E_CALL_ARITY",
                            "sort expects one list of Text, Int or Float64",
                            *s,
                        ));
                    }
                    let item_type = match self.expr(&args[0])? {
                        Type::List(item) => *item,
                        _ => {
                            return Err(diagnostic(
                                "E_TYPE_ARGUMENT",
                                "sort argument must be a List",
                                args[0].span(),
                            ))
                        }
                    };
                    if !matches!(item_type, Type::Text | Type::Int | Type::Float64) {
                        return Err(diagnostic(
                            "E_SORT_KEY",
                            "sort requires a List of Text, Int or Float64; use sort_by with a key function for other items",
                            args[0].span(),
                        ));
                    }
                    self.emit(Op::SortNatural, *s);
                    return Ok(Type::List(Box::new(item_type)));
                }
                if matches!(
                    name.as_str(),
                    "map_list" | "filter_list" | "fold_list" | "any_list" | "all_list"
                ) {
                    let fold = name == "fold_list";
                    let expected_arguments = if fold { 3 } else { 2 };
                    if args.len() != expected_arguments {
                        return Err(diagnostic(
                            "E_CALL_ARITY",
                            format!("{name} expects {expected_arguments} arguments"),
                            *s,
                        ));
                    }
                    let item_type = match self.expr(&args[0])? {
                        Type::List(item) => *item,
                        _ => {
                            return Err(diagnostic(
                                "E_TYPE_ARGUMENT",
                                format!("{name} first argument must be a List"),
                                args[0].span(),
                            ))
                        }
                    };
                    let accumulator = if fold {
                        Some(self.expr(&args[1])?)
                    } else {
                        None
                    };
                    let function_position = expected_arguments - 1;
                    let function = self.function_argument(
                        &args[function_position],
                        (
                            "E_TYPE_ARGUMENT",
                            "the last argument must be a named function or a function value",
                        ),
                        "function",
                    )?;
                    let function_span = function.span();
                    let (parameters, result) = function.signature();
                    let (expected_parameters, expected_result) = match name.as_str() {
                        "map_list" => (vec![item_type.clone()], None),
                        "filter_list" | "any_list" | "all_list" => {
                            (vec![item_type.clone()], Some(Type::Bool))
                        }
                        _ => {
                            let accumulator =
                                accumulator.clone().expect("fold has an initial value");
                            (
                                vec![accumulator.clone(), item_type.clone()],
                                Some(accumulator),
                            )
                        }
                    };
                    if parameters != expected_parameters
                        || expected_result
                            .as_ref()
                            .is_some_and(|expected| *expected != result)
                    {
                        return Err(diagnostic(
                            "E_TYPE_ARGUMENT",
                            format!(
                                "{name} function must have type {expected_parameters:?} -> {:?}",
                                expected_result.as_ref().unwrap_or(&result)
                            ),
                            function_span,
                        ));
                    }
                    if let Some(index) = function.index() {
                        self.emit(Op::MakeFn(index), function_span);
                    }
                    let (kind, result_type) = match name.as_str() {
                        "map_list" => (HigherOrderKind::MapList, Type::List(Box::new(result))),
                        "filter_list" => {
                            (HigherOrderKind::FilterList, Type::List(Box::new(item_type)))
                        }
                        "fold_list" => (HigherOrderKind::FoldList, result),
                        "any_list" => (HigherOrderKind::AnyList, Type::Bool),
                        _ => (HigherOrderKind::AllList, Type::Bool),
                    };
                    self.emit(Op::HigherOrder(kind), *s);
                    return Ok(result_type);
                }
                if name == "sort_by_int_key" {
                    if args.len() != 2 {
                        return Err(diagnostic(
                            "E_CALL_ARITY",
                            "sort_by_int_key expects a list and a named key function",
                            *s,
                        ));
                    }
                    let item_type = match self.expr(&args[0])? {
                        Type::List(item) => *item,
                        _ => {
                            return Err(diagnostic(
                                "E_TYPE_ARGUMENT",
                                "sort_by_int_key first argument must be a List",
                                args[0].span(),
                            ))
                        }
                    };
                    let key = self.function_argument(
                        &args[1],
                        (
                            "E_SORT_KEY",
                            "sort_by_int_key key function must be a named function or a function value",
                        ),
                        "key function",
                    )?;
                    let key_span = &key.span();
                    let (parameters, result) = key.signature();
                    if parameters != vec![item_type.clone()] || result != Type::Int {
                        return Err(diagnostic(
                            "E_SORT_KEY",
                            "sort_by_int_key key function must have type (T) -> Int for the list item type",
                            *key_span,
                        ));
                    }
                    match key.index() {
                        Some(key_index) => self.emit(Op::SortByIntKey(key_index), *s),
                        None => self.emit(Op::HigherOrder(HigherOrderKind::SortByIntKey), *s),
                    };
                    return Ok(Type::List(Box::new(item_type)));
                }
                if name == "merge_sorted_by_int_key" {
                    if args.len() != 2 {
                        return Err(diagnostic(
                            "E_CALL_ARITY",
                            "merge_sorted_by_int_key expects sorted feeds and a named key function",
                            *s,
                        ));
                    }
                    let item_type = match self.expr(&args[0])? {
                        Type::List(feed) => match *feed {
                            Type::List(item) => *item,
                            _ => return Err(diagnostic(
                                "E_TYPE_ARGUMENT",
                                "merge_sorted_by_int_key first argument must be a List of Lists",
                                args[0].span(),
                            )),
                        },
                        _ => {
                            return Err(diagnostic(
                                "E_TYPE_ARGUMENT",
                                "merge_sorted_by_int_key first argument must be a List of Lists",
                                args[0].span(),
                            ))
                        }
                    };
                    let key = self.function_argument(
                        &args[1],
                        (
                            "E_SORT_KEY",
                            "merge_sorted_by_int_key key function must be a named function or a function value",
                        ),
                        "key function",
                    )?;
                    let key_span = &key.span();
                    let (parameters, result) = key.signature();
                    if parameters != vec![item_type.clone()] || result != Type::Int {
                        return Err(diagnostic(
                            "E_SORT_KEY",
                            "merge_sorted_by_int_key key function must have type (T) -> Int for the feed item type",
                            *key_span,
                        ));
                    }
                    match key.index() {
                        Some(key_index) => self.emit(Op::MergeSortedByIntKey(key_index), *s),
                        None => {
                            self.emit(Op::HigherOrder(HigherOrderKind::MergeSortedByIntKey), *s)
                        }
                    };
                    return Ok(Type::List(Box::new(item_type)));
                }
                if name == "list_get" || name == "map_get" {
                    if args.len() != 2 {
                        return Err(diagnostic(
                            "E_CALL_ARITY",
                            format!("{name} expects a collection and a key or index"),
                            *s,
                        ));
                    }
                    let collection = self.expr(&args[0])?;
                    let item = match (&collection, name.as_str()) {
                        (Type::List(item), "list_get") => item.as_ref().clone(),
                        (Type::Map(item), "map_get") => item.as_ref().clone(),
                        (Type::List(_), "map_get") => {
                            return Err(diagnostic(
                                "E_TYPE_ARGUMENT",
                                "map_get requires a Map",
                                args[0].span(),
                            ))
                        }
                        (Type::Map(_), "list_get") => {
                            return Err(diagnostic(
                                "E_TYPE_ARGUMENT",
                                "list_get requires a List",
                                args[0].span(),
                            ))
                        }
                        _ => {
                            return Err(diagnostic(
                                "E_TYPE_ARGUMENT",
                                format!("{name} requires a List or Map as its first argument"),
                                args[0].span(),
                            ))
                        }
                    };
                    let expected_key = if name == "list_get" {
                        Type::Int
                    } else {
                        Type::Text
                    };
                    if self.expr_with_expected(&args[1], Some(&expected_key))? != expected_key {
                        return Err(diagnostic(
                            "E_TYPE_ARGUMENT",
                            format!("{name} key or index has an incompatible type"),
                            args[1].span(),
                        ));
                    }
                    self.emit(
                        if name == "list_get" {
                            Op::ListGetOption
                        } else {
                            Op::MapGetOption
                        },
                        *s,
                    );
                    return Ok(Type::Option(Box::new(item)));
                }
                if ["is_ok", "is_some", "unwrap", "error"].contains(&name.as_str()) {
                    if args.len() != 1 {
                        return Err(diagnostic(
                            "E_CALL_ARITY",
                            format!("{name} expects one Result or Option argument"),
                            *s,
                        ));
                    }
                    let argument_type = self.expr(&args[0])?;
                    return match (name.as_str(), argument_type) {
                        ("is_ok", Type::Result(_, _)) => {
                            self.emit(Op::IsOk, *s);
                            Ok(Type::Bool)
                        }
                        ("is_some", Type::Option(_)) => {
                            self.emit(Op::IsSome, *s);
                            Ok(Type::Bool)
                        }
                        ("unwrap", Type::Result(ok, _)) | ("unwrap", Type::Option(ok)) => {
                            self.emit(Op::Unwrap, *s);
                            Ok(*ok)
                        }
                        ("error", Type::Result(_, err)) => {
                            self.emit(Op::GetError, *s);
                            Ok(*err)
                        }
                        _ => Err(diagnostic(
                            "E_TYPE_ARGUMENT",
                            format!("{name} does not accept this value"),
                            args[0].span(),
                        )),
                    };
                }
                if name == "len" {
                    if args.len() != 1 {
                        return Err(diagnostic("E_CALL_ARITY", "len expects one argument", *s));
                    }
                    if !matches!(
                        self.expr(&args[0])?,
                        Type::List(_) | Type::Map(_) | Type::Bytes | Type::Text
                    ) {
                        return Err(diagnostic(
                            "E_TYPE_ARGUMENT",
                            "len expects a List, Map, Bytes, or Text",
                            args[0].span(),
                        ));
                    }
                    self.emit(Op::Length, *s);
                    return Ok(Type::Int);
                }
                if name == "map_set" {
                    if args.len() != 3 {
                        return Err(diagnostic(
                            "E_CALL_ARITY",
                            "map_set expects a Map, Text key, and value",
                            *s,
                        ));
                    }
                    let map_type = self.expr(&args[0])?;
                    let Type::Map(item) = map_type else {
                        return Err(diagnostic(
                            "E_TYPE_ARGUMENT",
                            "map_set first argument must be a Map",
                            args[0].span(),
                        ));
                    };
                    if self.expr(&args[1])? != Type::Text {
                        return Err(diagnostic(
                            "E_TYPE_ARGUMENT",
                            "map_set key must be Text",
                            args[1].span(),
                        ));
                    }
                    if self.expr_with_expected(&args[2], Some(item.as_ref()))? != *item {
                        return Err(diagnostic(
                            "E_TYPE_ARGUMENT",
                            "map_set value must match the Map value type",
                            args[2].span(),
                        ));
                    }
                    self.emit(Op::MapSet, *s);
                    return Ok(Type::Map(item));
                }
                if name == "map_record_get" || name == "map_record_set" {
                    let is_get = name == "map_record_get";
                    if !(if is_get {
                        [3, 4].contains(&args.len())
                    } else {
                        [4, 5].contains(&args.len())
                    }) {
                        return Err(diagnostic(
                            "E_CALL_ARITY",
                            if name == "map_record_get" {
                                "map_record_get expects a Map, one or two Text keys, and a literal field name"
                            } else {
                                "map_record_set expects a Map, one or two Text keys, a literal field name, and a value"
                            },
                            *s,
                        ));
                    }
                    let nested = if is_get {
                        args.len() == 4
                    } else {
                        args.len() == 5
                    };
                    let map_type = self.expr(&args[0])?;
                    let Type::Map(item_type) = map_type else {
                        return Err(diagnostic(
                            "E_TYPE_ARGUMENT",
                            format!("{name} first argument must be a Map of records"),
                            args[0].span(),
                        ));
                    };
                    let record_type = if nested {
                        let Type::Map(record_type) = item_type.as_ref() else {
                            return Err(diagnostic(
                                "E_TYPE_ARGUMENT",
                                format!("{name} nested key form requires a Map of Maps of records"),
                                args[0].span(),
                            ));
                        };
                        record_type.as_ref()
                    } else {
                        item_type.as_ref()
                    };
                    let Type::Record(record_name) = record_type else {
                        return Err(diagnostic(
                            "E_TYPE_ARGUMENT",
                            format!("{name} requires a Map of records or a nested Map of records"),
                            args[0].span(),
                        ));
                    };
                    if self.expr_with_expected(&args[1], Some(&Type::Text))? != Type::Text {
                        return Err(diagnostic(
                            "E_TYPE_ARGUMENT",
                            format!("{name} key must be Text"),
                            args[1].span(),
                        ));
                    }
                    if nested && self.expr_with_expected(&args[2], Some(&Type::Text))? != Type::Text
                    {
                        return Err(diagnostic(
                            "E_TYPE_ARGUMENT",
                            format!("{name} nested key must be Text"),
                            args[2].span(),
                        ));
                    }
                    let field_arg = if nested { 3 } else { 2 };
                    let Expr::Text(field_name, field_span) = &args[field_arg] else {
                        return Err(diagnostic(
                            "E_FIELD_LITERAL",
                            format!("{name} field name must be a Text literal"),
                            args[field_arg].span(),
                        ));
                    };
                    let Some((record_index, record)) = self
                        .records
                        .iter()
                        .enumerate()
                        .find(|(_, record)| record.name == *record_name)
                    else {
                        return Err(diagnostic(
                            "E_TYPE_UNKNOWN",
                            format!("unknown record type `{record_name}`"),
                            *field_span,
                        ));
                    };
                    let Some((field_index, (_, field_type, _))) = record
                        .fields
                        .iter()
                        .enumerate()
                        .find(|(_, (name, _, _))| name == field_name)
                    else {
                        return Err(diagnostic(
                            "E_FIELD_UNKNOWN",
                            format!("record `{record_name}` has no field `{field_name}`"),
                            *field_span,
                        ));
                    };
                    if is_get {
                        self.emit(Op::MapRecordGet(record_index, field_index, nested), *s);
                        return Ok(field_type.clone());
                    }
                    let value_arg = field_arg + 1;
                    if self.expr_with_expected(&args[value_arg], Some(field_type))? != *field_type {
                        return Err(diagnostic(
                            "E_TYPE_ARGUMENT",
                            format!("map_record_set value must match `{record_name}.{field_name}`"),
                            args[value_arg].span(),
                        ));
                    }
                    self.emit(Op::MapRecordSet(record_index, field_index, nested), *s);
                    return Ok(Type::Map(item_type));
                }
                if name == "list_push" {
                    if args.len() != 2 {
                        return Err(diagnostic(
                            "E_CALL_ARITY",
                            "list_push expects a List and a value",
                            *s,
                        ));
                    }
                    let list_type = self.expr(&args[0])?;
                    let Type::List(item) = list_type else {
                        return Err(diagnostic(
                            "E_TYPE_ARGUMENT",
                            "list_push first argument must be a List",
                            args[0].span(),
                        ));
                    };
                    if self.expr_with_expected(&args[1], Some(item.as_ref()))? != *item {
                        return Err(diagnostic(
                            "E_TYPE_ARGUMENT",
                            "list_push value must match the List item type",
                            args[1].span(),
                        ));
                    }
                    self.emit(Op::ListPush, *s);
                    return Ok(Type::List(item));
                }
                if name == "list_set" {
                    if args.len() != 3 {
                        return Err(diagnostic(
                            "E_CALL_ARITY",
                            "list_set expects a List, Int index, and value",
                            *s,
                        ));
                    }
                    let list_type = self.expr(&args[0])?;
                    let Type::List(item) = list_type else {
                        return Err(diagnostic(
                            "E_TYPE_ARGUMENT",
                            "list_set first argument must be a List",
                            args[0].span(),
                        ));
                    };
                    if self.expr(&args[1])? != Type::Int {
                        return Err(diagnostic(
                            "E_TYPE_ARGUMENT",
                            "list_set index must be Int",
                            args[1].span(),
                        ));
                    }
                    if self.expr_with_expected(&args[2], Some(item.as_ref()))? != *item {
                        return Err(diagnostic(
                            "E_TYPE_ARGUMENT",
                            "list_set value must match the List item type",
                            args[2].span(),
                        ));
                    }
                    self.emit(Op::ListSet, *s);
                    return Ok(Type::List(item));
                }
                if name == "map_has" {
                    if args.len() != 2 {
                        return Err(diagnostic(
                            "E_CALL_ARITY",
                            "map_has expects a Map and Text key",
                            *s,
                        ));
                    }
                    if !matches!(self.expr(&args[0])?, Type::Map(_)) {
                        return Err(diagnostic(
                            "E_TYPE_ARGUMENT",
                            "map_has first argument must be a Map",
                            args[0].span(),
                        ));
                    }
                    if self.expr(&args[1])? != Type::Text {
                        return Err(diagnostic(
                            "E_TYPE_ARGUMENT",
                            "map_has key must be Text",
                            args[1].span(),
                        ));
                    }
                    self.emit(Op::MapHas, *s);
                    return Ok(Type::Bool);
                }
                if name == "print" {
                    return Err(diagnostic(
                        "E_PRINT_VALUE",
                        "print is a statement and cannot be used as a value",
                        *s,
                    ));
                }
                if let Some((record_index, record)) =
                    self.records.iter().enumerate().find(|(_, record)| {
                        record.name == *name && self.visible_records.contains(name)
                    })
                {
                    if !keywords.is_empty() {
                        return Err(diagnostic(
                            "E_CALL_KEYWORD",
                            "record constructors accept positional fields only",
                            keywords[0].2,
                        ));
                    }
                    if record.module_id != self.ast.module_id && !record.private_fields.is_empty() {
                        return Err(diagnostic(
                            "E_FIELD_PRIVATE",
                            format!("record `{name}` has private fields, so it is built only by functions in the module that declares it"),
                            *s,
                        ));
                    }
                    if args.len() != record.fields.len() {
                        return Err(diagnostic(
                            "E_CALL_ARITY",
                            format!(
                                "record `{name}` expects {} fields, got {}",
                                record.fields.len(),
                                args.len()
                            ),
                            *s,
                        ));
                    }
                    for (argument, (_, expected, _)) in args.iter().zip(&record.fields) {
                        let actual = self.expr_with_expected(argument, Some(expected))?;
                        if actual != *expected {
                            return Err(diagnostic(
                                "E_TYPE_ARGUMENT",
                                format!("field of `{name}` expects {expected:?}, got {actual:?}"),
                                argument.span(),
                            ));
                        }
                    }
                    self.emit(Op::MakeRecord(record_index, args.len()), *s);
                    return Ok(Type::Record(name.clone()));
                }
                let (index, params, parameter_names, result, external) = if let Some(symbol) =
                    self.visible_functions.get(name)
                {
                    let Some((index, params, result)) = self.signatures.get(symbol).cloned() else {
                        return Err(diagnostic(
                            "E_NAME_UNKNOWN",
                            format!("unknown function `{name}`"),
                            *s,
                        ));
                    };
                    (index, params, None, result, false)
                } else if let Some(symbol) = self.visible_externals.get(name) {
                    let Some((index, params, result)) =
                        self.external_signatures.get(symbol).cloned()
                    else {
                        return Err(diagnostic(
                            "E_NAME_UNKNOWN",
                            format!("unknown external function `{name}`"),
                            *s,
                        ));
                    };
                    let (names, types): (Vec<_>, Vec<_>) = params.into_iter().unzip();
                    (index, types, Some(names), result, true)
                } else {
                    let mut candidates: Vec<&str> = builtin_catalog::names().collect();
                    candidates.extend(["map_record_get", "map_record_set"]);
                    let own: Vec<&str> = self
                        .visible_functions
                        .keys()
                        .chain(self.visible_externals.keys())
                        .map(String::as_str)
                        .collect();
                    candidates.extend(own);
                    return Err(diagnostic(
                        "E_NAME_UNKNOWN",
                        format!("unknown function `{name}`"),
                        *s,
                    )
                    .with_suggestions(suggest_names(name, &candidates)));
                };
                if !external && args.len() != params.len() {
                    return Err(diagnostic(
                        "E_CALL_ARITY",
                        format!(
                            "function `{name}` expects {} arguments, got {}",
                            params.len(),
                            args.len()
                        ),
                        *s,
                    ));
                }
                if external && args.len() > params.len() {
                    return Err(diagnostic(
                        "E_CALL_ARITY",
                        format!(
                            "Python function `{name}` accepts at most {} declared arguments",
                            params.len()
                        ),
                        *s,
                    ));
                }
                for (position, arg) in args.iter().enumerate() {
                    let expected = &params[position];
                    let actual = self.expr_with_expected(arg, Some(expected))?;
                    if actual != *expected {
                        let standard = !external
                            && self
                                .visible_functions
                                .get(name)
                                .is_some_and(|symbol| self.standard_functions.contains(symbol));
                        if standard && self.type_has_fn(&actual) && !self.type_has_fn(expected) {
                            return Err(diagnostic(
                                "E_FN_NOT_SERIALIZABLE",
                                format!(
                                    "a type that holds a function value cannot cross the standard-library function `{name}`"
                                ),
                                arg.span(),
                            ));
                        }
                        return Err(diagnostic(
                            "E_TYPE_ARGUMENT",
                            format!("argument to `{name}` expects {expected:?}, got {actual:?}"),
                            arg.span(),
                        ));
                    }
                }
                if !external && !keywords.is_empty() {
                    return Err(diagnostic(
                        "E_CALL_KEYWORD",
                        "keyword arguments are supported only for declared Python calls",
                        keywords[0].2,
                    ));
                }
                let mut keyword_names = std::collections::HashSet::new();
                for (keyword, value, span) in keywords {
                    let parameter_index = parameter_names
                        .as_ref()
                        .and_then(|names| names.iter().position(|name| name == keyword))
                        .ok_or_else(|| diagnostic(
                            "E_PYTHON_KEYWORD",
                            format!("`{keyword}` is not a declared parameter of Python function `{name}`"),
                            *span,
                        ))?;
                    if !keyword_names.insert(keyword.as_str()) {
                        return Err(diagnostic(
                            "E_PYTHON_KEYWORD_DUPLICATE",
                            format!("Python parameter `{keyword}` is supplied more than once"),
                            *span,
                        ));
                    }
                    if parameter_index < args.len() {
                        return Err(diagnostic(
                            "E_PYTHON_KEYWORD_DUPLICATE",
                            format!(
                                "Python parameter `{keyword}` is supplied positionally and by name"
                            ),
                            *span,
                        ));
                    }
                    let expected = &params[parameter_index];
                    let actual = self.expr_with_expected(value, Some(expected))?;
                    if actual != *expected {
                        return Err(diagnostic(
                            "E_TYPE_ARGUMENT",
                            format!("argument `{keyword}` to `{name}` expects {expected:?}, got {actual:?}"),
                            value.span(),
                        ));
                    }
                }
                let op = if external {
                    if keywords.is_empty() && args.len() == params.len() {
                        Op::PythonCall(index, args.len())
                    } else {
                        Op::PythonCallKw(
                            index,
                            args.len(),
                            keywords.iter().map(|(name, _, _)| name.clone()).collect(),
                        )
                    }
                } else {
                    Op::Call(index, args.len())
                };
                self.emit(op, *s);
                Ok(result)
            }
        }
    }
    fn stmts(&mut self, body: &[Stmt]) -> Result<(), Diagnostic> {
        for s in body {
            match s {
                Stmt::Let(name, mutable, annotation, value, span) => {
                    if self.locals.contains_key(name) {
                        return Err(diagnostic(
                            "E_NAME_DUPLICATE",
                            format!("`{name}` is already defined in this function"),
                            *span,
                        ));
                    }
                    let ty = self.expr_with_expected(value, annotation.as_ref())?;
                    if let Some(expected) = annotation {
                        if ty != *expected {
                            return Err(diagnostic(
                                "E_TYPE_ANNOTATION",
                                format!("annotation for `{name}` expects {expected:?}, got {ty:?}"),
                                *span,
                            ));
                        }
                    }
                    let slot = self.next_local;
                    self.next_local += 1;
                    self.emit(Op::Store(slot), *span);
                    let split = match &ty {
                        Type::Record(record_name)
                            if *mutable && self.split_targets.contains(name) =>
                        {
                            self.record_index_of(record_name)
                        }
                        _ => None,
                    };
                    self.locals.insert(
                        name.clone(),
                        Local {
                            slot,
                            ty,
                            mutable: *mutable,
                            split,
                        },
                    );
                    if let Some(record_index) = split {
                        self.split_record(name, record_index, slot, *span);
                        self.clear_local(slot, *span);
                    }
                }
                Stmt::Set(name, value, span) => {
                    if name.contains('.') && !self.locals.contains_key(name) {
                        let root = name.split('.').next().unwrap_or(name);
                        let root_local = self.locals.get(root).ok_or_else(|| {
                            diagnostic("E_NAME_UNKNOWN", format!("unknown name `{root}`"), *span)
                        })?;
                        return Err(if !root_local.mutable {
                            diagnostic(
                                "E_ASSIGN_IMMUTABLE",
                                format!("`{root}` is immutable; declare it with `var` to assign its fields"),
                                *span,
                            )
                        } else {
                            diagnostic(
                                "E_FIELD_UNKNOWN",
                                format!("`{name}` is not a field you can assign: `{root}` must be a record variable and each name after a dot a field of it"),
                                *span,
                            )
                        });
                    }
                    // `set record.field = list_push(record.field, x)` and the like: the call's
                    // first argument names the hidden local being assigned.
                    let normalized;
                    let value = match value {
                        Expr::Call(call, args, keywords, call_span)
                            if name.contains('.')
                                && !args.is_empty()
                                && dotted_path(&args[0]).as_deref() == Some(name.as_str()) =>
                        {
                            let mut args = args.clone();
                            args[0] = Expr::Name(name.clone(), args[0].span());
                            normalized = Expr::Call(call.clone(), args, keywords.clone(), *call_span);
                            &normalized
                        }
                        other => other,
                    };
                    if name.contains('.') {
                        // Every record along `a.b.c` must allow its field to be assigned here.
                        let mut parts = name.split('.');
                        let root = parts.next().unwrap_or(name);
                        let mut current = self.locals.get(root).map(|local| local.ty.clone());
                        for field in parts {
                            let Some(Type::Record(record_name)) = current.clone() else {
                                break;
                            };
                            self.check_private_field(&record_name, field, *span)?;
                            current = self
                                .records
                                .iter()
                                .find(|record| record.name == record_name)
                                .and_then(|record| {
                                    record
                                        .fields
                                        .iter()
                                        .find(|(field_name, _, _)| field_name == field)
                                        .map(|(_, ty, _)| ty.clone())
                                });
                        }
                    }
                    let local = self.locals.get(name).cloned().ok_or_else(|| {
                        diagnostic("E_NAME_UNKNOWN", format!("unknown name `{name}`"), *span)
                    })?;
                    if let (Some(record_index), true) = (local.split, local.mutable) {
                        self.assign_split_record(name, record_index, value, *span)?;
                        continue;
                    }
                    if !local.mutable {
                        return Err(diagnostic(
                            "E_ASSIGN_IMMUTABLE",
                            format!("`{name}` is immutable; declare it with `var` to assign it"),
                            *span,
                        ));
                    }
                    let optimized_list_push = match value {
                        Expr::Call(call, args, keywords, _)
                            if call == "list_push"
                                && keywords.is_empty()
                                && args.len() == 2
                                && matches!(&args[0], Expr::Name(source, _) if source == name) =>
                        {
                            let Type::List(item) = &local.ty else {
                                return Err(diagnostic(
                                    "E_TYPE_ASSIGN",
                                    "list_push assignment requires a List local",
                                    *span,
                                ));
                            };
                            if self.expr_with_expected(&args[1], Some(item.as_ref()))? != **item {
                                return Err(diagnostic(
                                    "E_TYPE_ARGUMENT",
                                    "list_push value must match the List item type",
                                    args[1].span(),
                                ));
                            }
                            Some(Op::ListPushLocal(local.slot))
                        }
                        _ => None,
                    };
                    let optimized_json_push = match value {
                        Expr::Call(call, args, keywords, _)
                            if call == "json_push"
                                && keywords.is_empty()
                                && args.len() == 2
                                && matches!(&args[0], Expr::Name(source, _) if source == name) =>
                        {
                            if local.ty != Type::Json {
                                return Err(diagnostic(
                                    "E_TYPE_ASSIGN",
                                    "json_push assignment requires a Json local",
                                    *span,
                                ));
                            }
                            if self.expr_with_expected(&args[1], Some(&Type::Json))? != Type::Json {
                                return Err(diagnostic(
                                    "E_TYPE_ARGUMENT",
                                    "json_push value must be Json",
                                    args[1].span(),
                                ));
                            }
                            Some(Op::JsonPushLocal(local.slot))
                        }
                        _ => None,
                    };
                    let optimized_json_set = match value {
                        Expr::Call(call, args, keywords, _)
                            if call == "json_set"
                                && keywords.is_empty()
                                && args.len() == 3
                                && matches!(&args[0], Expr::Name(source, _) if source == name) =>
                        {
                            if local.ty != Type::Json {
                                return Err(diagnostic(
                                    "E_TYPE_ASSIGN",
                                    "json_set assignment requires a Json local",
                                    *span,
                                ));
                            }
                            if self.expr_with_expected(&args[1], Some(&Type::Text))? != Type::Text {
                                return Err(diagnostic(
                                    "E_TYPE_ARGUMENT",
                                    "json_set key must be Text",
                                    args[1].span(),
                                ));
                            }
                            if self.expr_with_expected(&args[2], Some(&Type::Json))? != Type::Json {
                                return Err(diagnostic(
                                    "E_TYPE_ARGUMENT",
                                    "json_set value must be Json",
                                    args[2].span(),
                                ));
                            }
                            Some(Op::JsonSetLocal(local.slot))
                        }
                        _ => None,
                    };
                    let optimized_list_set = match value {
                        Expr::Call(call, args, keywords, _)
                            if call == "list_set"
                                && keywords.is_empty()
                                && args.len() == 3
                                && matches!(&args[0], Expr::Name(source, _) if source == name) =>
                        {
                            let Type::List(item) = &local.ty else {
                                return Err(diagnostic(
                                    "E_TYPE_ASSIGN",
                                    "list_set assignment requires a List local",
                                    *span,
                                ));
                            };
                            if self.expr(&args[1])? != Type::Int {
                                return Err(diagnostic(
                                    "E_TYPE_ARGUMENT",
                                    "list_set index must be Int",
                                    args[1].span(),
                                ));
                            }
                            if self.expr_with_expected(&args[2], Some(item.as_ref()))? != **item {
                                return Err(diagnostic(
                                    "E_TYPE_ARGUMENT",
                                    "list_set value must match the List item type",
                                    args[2].span(),
                                ));
                            }
                            Some(Op::ListSetLocal(local.slot))
                        }
                        _ => None,
                    };
                    let optimized_array_set = match value {
                        Expr::Call(call, args, keywords, _)
                            if call == "array_set"
                                && keywords.is_empty()
                                && args.len() == 3
                                && matches!(&local.ty, Type::Array(_))
                                && matches!(&args[0], Expr::Name(source, _) if source == name) =>
                        {
                            let Type::Array(element) = &local.ty else {
                                unreachable!("guarded above")
                            };
                            if self.expr(&args[1])? != Type::Int {
                                return Err(diagnostic(
                                    "E_TYPE_ARGUMENT",
                                    "array_set index must be Int",
                                    args[1].span(),
                                ));
                            }
                            let scalar = element.scalar_type();
                            if self.expr_with_expected(&args[2], Some(&scalar))? != scalar {
                                return Err(diagnostic(
                                    "E_TYPE_ARGUMENT",
                                    format!(
                                        "array_set value must be {scalar:?} for Array[{}]",
                                        element.name()
                                    ),
                                    args[2].span(),
                                ));
                            }
                            Some(Op::ArraySetLocal(local.slot))
                        }
                        _ => None,
                    };
                    let optimized_local_list_update = optimized_list_push.is_some()
                        || optimized_list_set.is_some()
                        || optimized_array_set.is_some();
                    let optimized_local_update = optimized_local_list_update
                        || optimized_json_push.is_some()
                        || optimized_json_set.is_some();
                    let ty = if optimized_local_update {
                        local.ty.clone()
                    } else if let Some(regrouped) = (local.ty == Type::Text)
                        .then(|| regroup_local_text_append(value, name))
                        .flatten()
                    {
                        let start = self.code.len();
                        match self.expr_with_expected(&regrouped, Some(&local.ty)) {
                            Ok(ty) => ty,
                            // Report errors against the expression as written.
                            Err(_) => {
                                self.code.truncate(start);
                                self.expr_with_expected(value, Some(&local.ty))?
                            }
                        }
                    } else {
                        self.expr_with_expected(value, Some(&local.ty))?
                    };
                    if ty != local.ty {
                        return Err(diagnostic(
                            "E_TYPE_ASSIGN",
                            format!("cannot assign {ty:?} to {name}: {:?}", local.ty),
                            *span,
                        ));
                    }
                    self.emit(
                        optimized_json_set
                            .or(optimized_json_push)
                            .or(optimized_list_push)
                            .or(optimized_list_set)
                            .or(optimized_array_set)
                            .unwrap_or(Op::Store(local.slot)),
                        *span,
                    );
                }
                Stmt::Return(value, span) => {
                    let ty = self.expr_with_expected(value, Some(&self.ast.result))?;
                    if ty != self.ast.result {
                        return Err(diagnostic(
                            "E_TYPE_RETURN",
                            format!(
                                "function returns {:?}, but this expression is {ty:?}",
                                self.ast.result
                            ),
                            *span,
                        ));
                    }
                    self.emit(Op::Return, *span);
                }
                Stmt::Expr(Expr::Call(name, args, keywords, span), _)
                    if name == "print" || name == "eprint" =>
                {
                    if !keywords.is_empty() {
                        return Err(diagnostic(
                            "E_CALL_KEYWORD",
                            format!("{name} accepts positional arguments only"),
                            keywords[0].2,
                        ));
                    }
                    if args.len() != 1 {
                        return Err(diagnostic(
                            "E_CALL_ARITY",
                            format!("{name} expects one argument"),
                            *span,
                        ));
                    }
                    let printed = self.expr(&args[0])?;
                    if self.type_has_fn(&printed) {
                        return Err(diagnostic(
                            "E_FN_NOT_SERIALIZABLE",
                            "a type that holds a function value cannot cross program output",
                            args[0].span(),
                        ));
                    }
                    self.emit(
                        if name == "eprint" {
                            Op::PrintErr
                        } else {
                            Op::Print
                        },
                        *span,
                    );
                }
                Stmt::Expr(e, span) => {
                    self.expr(e)?;
                    self.emit(Op::Pop, *span);
                }
                Stmt::If(cond, yes, no, span) => {
                    let ty = self.expr(cond)?;
                    if ty != Type::Bool {
                        return Err(diagnostic(
                            "E_TYPE_CONDITION",
                            "if condition must be Bool",
                            cond.span(),
                        ));
                    }
                    let branch = self.emit(Op::JumpIfFalse(0), *span);
                    let before = self.locals.clone();
                    self.stmts(yes)?;
                    let jump = self.emit(Op::Jump(0), *span);
                    self.patch(branch, self.code.len());
                    self.locals = before.clone();
                    if !no.is_empty() {
                        self.stmts(no)?;
                    }
                    self.patch(jump, self.code.len());
                    self.locals = before;
                }
                Stmt::While(cond, body, span) => {
                    let start = self.code.len();
                    let ty = self.expr(cond)?;
                    if ty != Type::Bool {
                        return Err(diagnostic(
                            "E_TYPE_CONDITION",
                            "while condition must be Bool",
                            cond.span(),
                        ));
                    }
                    let exit = self.emit(Op::JumpIfFalse(0), *span);
                    let before = self.locals.clone();
                    self.loops.push(LoopFrame::default());
                    self.stmts(body)?;
                    let frame = self.loops.pop().expect("loop frame was pushed above");
                    self.locals = before;
                    self.emit(Op::Jump(start), *span);
                    let end = self.code.len();
                    self.patch(exit, end);
                    for jump in frame.breaks {
                        self.patch(jump, end);
                    }
                    for jump in frame.continues {
                        self.patch(jump, start);
                    }
                }
                Stmt::For(name, Expr::Call(callee, range_args, range_keywords, call_span), body, span)
                    if callee == "range" =>
                {
                    if !range_keywords.is_empty() || range_args.is_empty() || range_args.len() > 3 {
                        return Err(diagnostic(
                            "E_CALL_ARITY",
                            "range takes one to three integers: range(end), range(start, end) or range(start, end, step)",
                            *call_span,
                        ));
                    }
                    if self.locals.contains_key(name) {
                        return Err(diagnostic(
                            "E_NAME_DUPLICATE",
                            format!("`{name}` is already defined in this function"),
                            *span,
                        ));
                    }
                    let step = match range_args.get(2) {
                        None => 1i64,
                        Some(Expr::Int(value, _)) => value.to_string().parse::<i64>().unwrap_or(0),
                        Some(Expr::Unary(op, inner, _)) if op == "-" => match inner.as_ref() {
                            Expr::Int(value, _) => {
                                -value.to_string().parse::<i64>().unwrap_or(0)
                            }
                            _ => 0,
                        },
                        Some(_) => 0,
                    };
                    if step == 0 {
                        return Err(diagnostic(
                            "E_RANGE_STEP",
                            "the step of range must be a non-zero whole number written in the source, such as 2 or -1",
                            range_args.get(2).map_or(*call_span, Expr::span),
                        ));
                    }
                    let current_slot = self.next_local;
                    let end_slot = current_slot + 1;
                    self.next_local += 2;
                    let (start_expr, end_expr) = if range_args.len() == 1 {
                        (None, &range_args[0])
                    } else {
                        (Some(&range_args[0]), &range_args[1])
                    };
                    match start_expr {
                        Some(start) => {
                            if self.expr_with_expected(start, Some(&Type::Int))? != Type::Int {
                                return Err(diagnostic(
                                    "E_TYPE_ARGUMENT",
                                    "range bounds must be Int",
                                    start.span(),
                                ));
                            }
                        }
                        None => {
                            self.emit(Op::Const(Value::Int(BigInt::zero())), *span);
                        }
                    }
                    self.emit(Op::Store(current_slot), *span);
                    if self.expr_with_expected(end_expr, Some(&Type::Int))? != Type::Int {
                        return Err(diagnostic(
                            "E_TYPE_ARGUMENT",
                            "range bounds must be Int",
                            end_expr.span(),
                        ));
                    }
                    self.emit(Op::Store(end_slot), *span);
                    let start_ip = self.code.len();
                    self.emit(Op::Load(current_slot), *span);
                    self.emit(Op::Load(end_slot), *span);
                    self.emit(if step > 0 { Op::Less } else { Op::Greater }, *span);
                    let exit = self.emit(Op::JumpIfFalse(0), *span);
                    let old_locals = self.locals.clone();
                    let item_slot = self.next_local;
                    self.next_local += 1;
                    self.emit(Op::Load(current_slot), *span);
                    self.emit(Op::Store(item_slot), *span);
                    self.locals.insert(
                        name.clone(),
                        Local {
                            slot: item_slot,
                            ty: Type::Int,
                            mutable: false,
                            split: None,
                        },
                    );
                    self.loops.push(LoopFrame::default());
                    self.stmts(body)?;
                    let frame = self.loops.pop().expect("loop frame was pushed above");
                    self.locals = old_locals;
                    let advance = self.code.len();
                    self.emit(Op::Load(current_slot), *span);
                    self.emit(Op::Const(Value::Int(BigInt::from_i64(step))), *span);
                    self.emit(Op::Add, *span);
                    self.emit(Op::Store(current_slot), *span);
                    self.emit(Op::Jump(start_ip), *span);
                    let end = self.code.len();
                    self.patch(exit, end);
                    for jump in frame.breaks {
                        self.patch(jump, end);
                    }
                    for jump in frame.continues {
                        self.patch(jump, advance);
                    }
                }
                Stmt::For(name, values, body, span) => {
                    let item_type = match self.expr(values)? {
                        Type::List(item) => *item,
                        Type::Map(_) => {
                            self.emit(Op::MapKeys, values.span());
                            Type::Text
                        }
                        _ => {
                            return Err(diagnostic(
                                "E_TYPE_ITERATION",
                                "for loops iterate over a List",
                                values.span(),
                            ))
                        }
                    };
                    let list_slot = self.next_local;
                    let index_slot = list_slot + 1;
                    self.next_local += 2;
                    self.emit(Op::Store(list_slot), *span);
                    self.emit(Op::Const(Value::Int(BigInt::zero())), *span);
                    self.emit(Op::Store(index_slot), *span);
                    let start = self.code.len();
                    self.emit(Op::Load(index_slot), *span);
                    self.emit(Op::Load(list_slot), *span);
                    self.emit(Op::Length, *span);
                    self.emit(Op::Less, *span);
                    let exit = self.emit(Op::JumpIfFalse(0), *span);
                    self.emit(Op::Load(list_slot), *span);
                    self.emit(Op::Load(index_slot), *span);
                    self.emit(Op::Index, *span);
                    let old_locals = self.locals.clone();
                    if self.locals.contains_key(name) {
                        return Err(diagnostic(
                            "E_NAME_DUPLICATE",
                            format!("`{name}` is already defined in this function"),
                            *span,
                        ));
                    }
                    let item_slot = self.next_local;
                    self.next_local += 1;
                    self.emit(Op::Store(item_slot), *span);
                    self.locals.insert(
                        name.clone(),
                        Local {
                            slot: item_slot,
                            ty: item_type,
                            mutable: false,
                            split: None,
                        },
                    );
                    self.loops.push(LoopFrame::default());
                    self.stmts(body)?;
                    let frame = self.loops.pop().expect("loop frame was pushed above");
                    self.locals = old_locals;
                    let advance = self.code.len();
                    self.emit(Op::Load(index_slot), *span);
                    self.emit(Op::Const(Value::Int(BigInt::one())), *span);
                    self.emit(Op::Add, *span);
                    self.emit(Op::Store(index_slot), *span);
                    self.emit(Op::Jump(start), *span);
                    let end = self.code.len();
                    self.patch(exit, end);
                    for jump in frame.breaks {
                        self.patch(jump, end);
                    }
                    for jump in frame.continues {
                        self.patch(jump, advance);
                    }
                }
                Stmt::Break(span) | Stmt::Continue(span) => {
                    let is_break = matches!(s, Stmt::Break(_));
                    if self.loops.is_empty() {
                        let word = if is_break { "break" } else { "continue" };
                        return Err(diagnostic(
                            "E_NO_LOOP",
                            format!("`{word}` outside a loop"),
                            *span,
                        ));
                    }
                    let jump = self.emit(Op::Jump(0), *span);
                    let frame = self.loops.last_mut().expect("checked above");
                    if is_break {
                        frame.breaks.push(jump);
                    } else {
                        frame.continues.push(jump);
                    }
                }
                Stmt::Match(value, arms, span) => {
                    let value_type = self.expr(value)?;
                    if !matches!(value_type, Type::Option(_) | Type::Result(_, _)) {
                        return Err(diagnostic(
                            "E_MATCH_TYPE",
                            "match currently supports Option and Result values",
                            value.span(),
                        ));
                    }
                    let mut true_arm = None;
                    let mut false_arm = None;
                    for (index, arm) in arms.iter().enumerate() {
                        let is_true = match (&value_type, &arm.pattern) {
                            (Type::Option(_), MatchPattern::Some(_, _))
                            | (Type::Result(_, _), MatchPattern::Ok(_, _)) => true,
                            (Type::Option(_), MatchPattern::None)
                            | (Type::Result(_, _), MatchPattern::Err(_, _)) => false,
                            _ => {
                                return Err(diagnostic(
                                    "E_MATCH_VARIANT",
                                    "match arm variant does not belong to the matched Option or Result",
                                    arm.span,
                                ));
                            }
                        };
                        let slot = if is_true {
                            &mut true_arm
                        } else {
                            &mut false_arm
                        };
                        if slot.replace(index).is_some() {
                            return Err(diagnostic(
                                "E_MATCH_DUPLICATE",
                                "match contains the same variant more than once",
                                arm.span,
                            ));
                        }
                    }
                    let Some(false_index) = false_arm else {
                        return Err(diagnostic(
                            "E_MATCH_NONEXHAUSTIVE",
                            "Option matches require Some and None; Result matches require Ok and Err",
                            *span,
                        ));
                    };
                    if true_arm.is_none() || arms.len() != 2 {
                        return Err(diagnostic(
                            "E_MATCH_NONEXHAUSTIVE",
                            "match must contain exactly one arm for each variant",
                            *span,
                        ));
                    }

                    let value_slot = self.next_local;
                    self.next_local += 1;
                    self.emit(Op::Store(value_slot), *span);
                    self.emit(Op::Load(value_slot), *span);
                    self.emit(
                        if matches!(value_type, Type::Option(_)) {
                            Op::IsSome
                        } else {
                            Op::IsOk
                        },
                        *span,
                    );
                    if false_index == 0 {
                        self.emit(Op::Not, *span);
                    }
                    let branch = self.emit(Op::JumpIfFalse(0), *span);
                    let before = self.locals.clone();
                    self.match_arm(&arms[0], value_slot, &value_type)?;
                    self.locals = before.clone();
                    let skip_false = self.emit(Op::Jump(0), *span);
                    self.patch(branch, self.code.len());
                    self.match_arm(&arms[1], value_slot, &value_type)?;
                    self.patch(skip_false, self.code.len());
                    self.locals = before;
                }
            }
        }
        Ok(())
    }

    fn match_arm(
        &mut self,
        arm: &MatchArm,
        value_slot: usize,
        value_type: &Type,
    ) -> Result<(), Diagnostic> {
        let binding = match (&arm.pattern, value_type) {
            (MatchPattern::Some(name, span), Type::Option(item)) => {
                Some((name, *span, item.as_ref().clone(), Op::Unwrap))
            }
            (MatchPattern::Ok(name, span), Type::Result(ok, _)) => {
                Some((name, *span, ok.as_ref().clone(), Op::Unwrap))
            }
            (MatchPattern::Err(name, span), Type::Result(_, err)) => {
                Some((name, *span, err.as_ref().clone(), Op::GetError))
            }
            (MatchPattern::None, Type::Option(_)) => None,
            _ => unreachable!("match arms are type-checked before lowering"),
        };
        let before = self.locals.clone();
        if let Some((name, binding_span, ty, extract)) = binding {
            if self.locals.contains_key(name) {
                return Err(diagnostic(
                    "E_NAME_DUPLICATE",
                    format!("match binding `{name}` shadows an existing local"),
                    binding_span,
                ));
            }
            let slot = self.next_local;
            self.next_local += 1;
            self.emit(Op::Load(value_slot), binding_span);
            self.emit(extract, binding_span);
            self.emit(Op::Store(slot), binding_span);
            self.locals.insert(
                name.clone(),
                Local {
                    slot,
                    ty,
                    mutable: false,
                    split: None,
                },
            );
        }
        self.stmts(&arm.body)?;
        self.locals = before;
        Ok(())
    }
}

pub fn compile(source_name: &str, source: &str) -> Result<Artifact, Diagnostic> {
    let program = parse_program(source, 0).map_err(|error| error.with_source_name(source_name))?;
    if !program.imports.is_empty() {
        return Err(diagnostic(
            "E_IMPORT_PROJECT_REQUIRED",
            "imports require compiling from a project entry file",
            Span::default(),
        )
        .with_source_name(source_name));
    }
    let scope = module_scope(&program);
    let mut artifact = compile_program(
        source_name,
        vec![source_name.to_owned()],
        program,
        HashMap::from([(0, scope)]),
    )
    .map_err(|error| error.with_source_name(source_name))?;
    artifact.compiled_source_sha256 = Some(digest_sources(vec![(source_name.to_owned(), source)]));
    Ok(artifact)
}

#[derive(Default)]
struct ModuleScope {
    functions: HashMap<String, usize>,
    externals: HashMap<String, usize>,
    records: HashSet<String>,
}

#[derive(Clone, Default)]
struct ModuleExports {
    functions: HashMap<String, usize>,
    records: HashMap<String, usize>,
}

fn public_module_exports(
    module_id: usize,
    nodes: &[ModuleNode],
    sources: &[String],
    memo: &mut HashMap<usize, ModuleExports>,
) -> Result<ModuleExports, Diagnostic> {
    if let Some(exports) = memo.get(&module_id) {
        return Ok(exports.clone());
    }

    let node = &nodes[module_id];
    let local_functions = node
        .program
        .functions
        .iter()
        .filter(|function| function.public)
        .map(|function| function.name.clone())
        .collect::<HashSet<_>>();
    let mut exports = ModuleExports {
        functions: node
            .program
            .functions
            .iter()
            .filter(|function| function.public)
            .map(|function| (function.name.clone(), function.module_id))
            .collect(),
        records: node
            .program
            .records
            .iter()
            .filter(|record| record.public)
            .map(|record| (record.name.clone(), record.module_id))
            .collect(),
    };

    for (imported_id, import_span, reexport) in &node.imports {
        if !reexport {
            continue;
        }
        let imported = public_module_exports(*imported_id, nodes, sources, memo)?;
        for (name, owner) in imported.functions {
            if local_functions.contains(&name) {
                continue;
            }
            if let Some(previous_owner) = exports.functions.get(&name) {
                if *previous_owner != owner {
                    return Err(diagnostic(
                        "E_NAME_AMBIGUOUS",
                        format!(
                            "public function {name} is exported by more than one public import"
                        ),
                        *import_span,
                    )
                    .with_source_name(sources[module_id].clone()));
                }
            } else {
                exports.functions.insert(name, owner);
            }
        }
        for (name, owner) in imported.records {
            if let Some(previous_owner) = exports.records.get(&name) {
                if *previous_owner != owner {
                    return Err(diagnostic(
                        "E_RECORD_NAME",
                        format!("record name `{name}` is exported by multiple modules"),
                        *import_span,
                    )
                    .with_source_name(sources[module_id].clone()));
                }
            } else {
                exports.records.insert(name, owner);
            }
        }
    }

    memo.insert(module_id, exports.clone());
    Ok(exports)
}

fn function_symbol(module_id: usize, name: &str) -> String {
    format!("$module{module_id}::{name}")
}

fn module_scope(program: &Program) -> ModuleScope {
    ModuleScope {
        functions: program
            .functions
            .iter()
            .map(|f| (f.name.clone(), f.module_id))
            .collect(),
        externals: program
            .externals
            .iter()
            .map(|f| (f.name.clone(), f.module_id))
            .collect(),
        records: program.records.iter().map(|r| r.name.clone()).collect(),
    }
}

fn parse_program(source: &str, source_id: u32) -> Result<Program, Diagnostic> {
    let tokens = lex(source, source_id)?;
    if let Some(error) = python_recovery_diagnostic(&tokens) {
        return Err(error);
    }
    Parser {
        tokens,
        at: 0,
        module_id: source_id as usize,
        array_elements: Vec::new(),
    }
    .program()
}

struct ModuleNode {
    path: PathBuf,
    module_root: PathBuf,
    root_label: String,
    source_name: String,
    source: String,
    program: Program,
    imports: Vec<(usize, Span, bool)>,
}

const MAX_MODULE_COUNT: usize = 512;
const MAX_MODULE_DEPTH: usize = 128;
const MAX_MODULE_SOURCE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_GRAPH_SOURCE_BYTES: usize = 64 * 1024 * 1024;

struct ModuleLoader {
    stdlib_root: Option<PathBuf>,
    package_roots: HashMap<String, PathBuf>,
    nodes: Vec<ModuleNode>,
    by_path: HashMap<PathBuf, usize>,
    active: HashSet<PathBuf>,
    total_source_bytes: usize,
    profile_enabled: bool,
    module_profiles: Vec<ModuleLoadProfile>,
}

impl ModuleLoader {
    fn load(
        &mut self,
        path: &Path,
        module_root: &Path,
        root_label: &str,
        source_name: String,
        importer: Option<(u32, String)>,
        depth: usize,
    ) -> Result<usize, Diagnostic> {
        let path_started = self.profile_enabled.then(Instant::now);
        let canonical = path.canonicalize().map_err(|error| {
            diagnostic(
                "E_IMPORT_READ",
                format!("cannot resolve imported module {}: {error}", path.display()),
                Span {
                    source_id: importer.as_ref().map_or(0, |(source_id, _)| *source_id),
                    ..Span::default()
                },
            )
            .with_source_name(
                importer
                    .as_ref()
                    .map_or_else(|| path.display().to_string(), |(_, name)| name.clone()),
            )
        })?;
        if !canonical.starts_with(module_root) || !canonical.is_file() {
            return Err(diagnostic(
                "E_IMPORT_ESCAPE",
                "imported module must be a file inside its permitted module root",
                Span {
                    source_id: importer.as_ref().map_or(0, |(source_id, _)| *source_id),
                    ..Span::default()
                },
            )
            .with_source_name(
                importer
                    .as_ref()
                    .map_or_else(|| path.display().to_string(), |(_, name)| name.clone()),
            ));
        }
        if self.active.contains(&canonical) {
            return Err(diagnostic(
                "E_MODULE_CYCLE",
                format!("module import cycle includes {}", canonical.display()),
                Span {
                    source_id: importer.as_ref().map_or(0, |(source_id, _)| *source_id),
                    ..Span::default()
                },
            )
            .with_source_name(
                importer
                    .as_ref()
                    .map_or_else(|| canonical.display().to_string(), |(_, name)| name.clone()),
            ));
        }
        if let Some(index) = self.by_path.get(&canonical) {
            return Ok(*index);
        }

        let source_id = importer.as_ref().map_or(0, |(source_id, _)| *source_id);
        let diagnostic_source_name = importer
            .as_ref()
            .map_or_else(|| canonical.display().to_string(), |(_, name)| name.clone());
        if depth > MAX_MODULE_DEPTH {
            return Err(diagnostic(
                "E_MODULE_DEPTH",
                format!("module import depth exceeds {MAX_MODULE_DEPTH}"),
                Span {
                    source_id,
                    ..Span::default()
                },
            )
            .with_source_name(diagnostic_source_name.clone()));
        }
        if self.nodes.len() >= MAX_MODULE_COUNT {
            return Err(diagnostic(
                "E_MODULE_COUNT",
                format!("module graph exceeds {MAX_MODULE_COUNT} files"),
                Span {
                    source_id,
                    ..Span::default()
                },
            )
            .with_source_name(diagnostic_source_name.clone()));
        }
        let file_bytes = fs::metadata(&canonical)
            .map_err(|error| {
                diagnostic(
                    "E_IMPORT_READ",
                    format!("cannot inspect module {}: {error}", canonical.display()),
                    Span {
                        source_id,
                        ..Span::default()
                    },
                )
                .with_source_name(diagnostic_source_name.clone())
            })?
            .len();
        if file_bytes > MAX_MODULE_SOURCE_BYTES {
            return Err(diagnostic(
                "E_MODULE_SIZE",
                format!("module exceeds the {MAX_MODULE_SOURCE_BYTES}-byte source limit"),
                Span {
                    source_id,
                    ..Span::default()
                },
            )
            .with_source_name(diagnostic_source_name.clone()));
        }
        self.total_source_bytes = self
            .total_source_bytes
            .checked_add(file_bytes as usize)
            .ok_or_else(|| {
                diagnostic(
                    "E_MODULE_SIZE",
                    "module graph source size overflow",
                    Span::default(),
                )
            })?;
        if self.total_source_bytes > MAX_GRAPH_SOURCE_BYTES {
            return Err(diagnostic(
                "E_MODULE_SIZE",
                format!("module graph exceeds the {MAX_GRAPH_SOURCE_BYTES}-byte source limit"),
                Span {
                    source_id,
                    ..Span::default()
                },
            )
            .with_source_name(diagnostic_source_name));
        }

        let path_validation_ns = path_started
            .map(|started| started.elapsed().as_nanos())
            .unwrap_or_default();
        let read_started = self.profile_enabled.then(Instant::now);
        let source = fs::read_to_string(&canonical).map_err(|error| {
            diagnostic(
                "E_IMPORT_READ",
                format!("cannot read module {}: {error}", canonical.display()),
                Span::default(),
            )
        })?;
        let source_read_ns = read_started
            .map(|started| started.elapsed().as_nanos())
            .unwrap_or_default();
        let index = self.nodes.len();
        let parse_started = self.profile_enabled.then(Instant::now);
        let mut program = parse_program(&source, index as u32)
            .map_err(|error| error.with_source_name(source_name.clone()))?;
        let parse_ns = parse_started
            .map(|started| started.elapsed().as_nanos())
            .unwrap_or_default();
        let import_paths = std::mem::take(&mut program.imports);
        let import_count = import_paths.len();
        let profile_source_name = source_name.clone();
        self.by_path.insert(canonical.clone(), index);
        self.nodes.push(ModuleNode {
            path: canonical.clone(),
            module_root: module_root.to_path_buf(),
            root_label: root_label.to_string(),
            source_name,
            source: source.clone(),
            program,
            imports: Vec::new(),
        });
        self.active.insert(canonical.clone());
        let mut imports = Vec::new();
        let mut import_resolution_ns = 0u128;
        for import in import_paths {
            let resolve_started = self.profile_enabled.then(Instant::now);
            let (imported, child_root, child_label, child_name) =
                self.resolve_import(index, &import.path, import.span)?;
            if let Some(started) = resolve_started {
                import_resolution_ns =
                    import_resolution_ns.saturating_add(started.elapsed().as_nanos());
            }
            let importer_name = self.nodes[index].source_name.clone();
            let child_index = self.load(
                &imported,
                &child_root,
                &child_label,
                child_name,
                Some((index as u32, importer_name)),
                depth + 1,
            )?;
            imports.push((child_index, import.span, import.reexport));
        }
        self.active.remove(&canonical);
        self.nodes[index].imports = imports;
        if self.profile_enabled {
            self.module_profiles.push(ModuleLoadProfile {
                source_name: profile_source_name,
                source_bytes: source.len(),
                import_count,
                path_validation_ns,
                source_read_ns,
                parse_ns,
                import_resolution_ns,
            });
        }
        Ok(index)
    }

    fn resolve_import(
        &self,
        importer_index: usize,
        value: &str,
        span: Span,
    ) -> Result<(PathBuf, PathBuf, String, String), Diagnostic> {
        let importer = &self.nodes[importer_index];
        let fail = |code: &str, message: String| {
            diagnostic(code, message, span).with_source_name(importer.source_name.clone())
        };
        if let Some(module) = value.strip_prefix("std:") {
            if module.is_empty()
                || module.starts_with('/')
                || module
                    .split('/')
                    .any(|part| part.is_empty() || part == "." || part == "..")
                || module.contains('\\')
            {
                return Err(fail("E_IMPORT_PATH", "standard-library import must name a module inside the shipped standard library".into()));
            }
            let root = self.stdlib_root.as_ref().ok_or_else(|| {
                fail(
                    "E_IMPORT_ROOT",
                    "no verified standard-library root was supplied".into(),
                )
            })?;
            let path = if module.ends_with(".gu") || module.ends_with(".vr") {
                root.join(module)
            } else {
                root.join(format!("{module}.gu"))
            };
            return Ok((path, root.clone(), "std".into(), format!("std:{module}")));
        }
        if let Some(package_path) = value.strip_prefix("pkg:") {
            let Some((package, relative)) = package_path.split_once('/') else {
                return Err(fail(
                    "E_IMPORT_PATH",
                    "package import must use `pkg:name/path.gu`".into(),
                ));
            };
            if package.starts_with('@') {
                return Err(fail(
                    "E_PACKAGE_SCOPE_RESERVED",
                    "scoped package names (`@owner/name`) are reserved and not available yet"
                        .into(),
                ));
            }
            let requested = Path::new(relative);
            if package.is_empty()
                || requested.is_absolute()
                || requested
                    .components()
                    .any(|part| !matches!(part, Component::Normal(_)))
            {
                return Err(fail(
                    "E_IMPORT_PATH",
                    "package import path must stay inside the named package root".into(),
                ));
            }
            let root = self.package_roots.get(package).ok_or_else(|| {
                fail(
                    "E_PACKAGE_NOT_LOCKED",
                    format!("package `{package}` is not present in the resolved dependency roots"),
                )
            })?;
            return Ok((
                root.join(requested),
                root.clone(),
                format!("pkg:{package}"),
                value.to_string(),
            ));
        }
        let requested = Path::new(value);
        if requested.is_absolute()
            || requested
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(fail(
                "E_IMPORT_PATH",
                "import paths must be relative and cannot contain `.` or `..` components".into(),
            ));
        }
        let path = importer
            .path
            .parent()
            .unwrap_or(&importer.module_root)
            .join(requested);
        let relative = path
            .strip_prefix(&importer.module_root)
            .unwrap_or(&path)
            .to_string_lossy()
            .into_owned();
        let source_name = if importer.root_label.is_empty() {
            relative
        } else if importer.root_label == "std" {
            format!("std:{relative}")
        } else {
            format!("{}/{relative}", importer.root_label)
        };
        Ok((
            path,
            importer.module_root.clone(),
            importer.root_label.clone(),
            source_name,
        ))
    }
}

/// Compile a project entry point and every explicit relative import beneath `project_root`.
/// Imported functions and records must be declared `pub`; their dependencies remain scoped to
/// their own module and direct imports.
pub fn compile_project(entry: &Path, project_root: &Path) -> Result<Artifact, Diagnostic> {
    compile_project_with_roots(entry, project_root, None, &HashMap::new())
}

/// Compile a project with explicit, independently supplied standard-library and package roots.
/// All imported paths are canonicalized and confined to their selected root before parsing.
pub fn compile_project_with_roots(
    entry: &Path,
    project_root: &Path,
    stdlib_root: Option<&Path>,
    package_roots: &HashMap<String, PathBuf>,
) -> Result<Artifact, Diagnostic> {
    compile_project_with_roots_inner(entry, project_root, stdlib_root, package_roots, false)
        .map(|(artifact, _)| artifact)
}

/// Compile a project and return opt-in timings for the resolved source modules.
///
/// The ordinary [`compile_project_with_roots`] path does not read the clock or collect profiles.
pub fn compile_project_with_roots_profiled(
    entry: &Path,
    project_root: &Path,
    stdlib_root: Option<&Path>,
    package_roots: &HashMap<String, PathBuf>,
) -> Result<(Artifact, CompileProfile), Diagnostic> {
    compile_project_with_roots_inner(entry, project_root, stdlib_root, package_roots, true)
}

fn compile_project_with_roots_inner(
    entry: &Path,
    project_root: &Path,
    stdlib_root: Option<&Path>,
    package_roots: &HashMap<String, PathBuf>,
    profile_enabled: bool,
) -> Result<(Artifact, CompileProfile), Diagnostic> {
    let root = project_root.canonicalize().map_err(|error| {
        diagnostic(
            "E_PROJECT_ROOT",
            format!("cannot resolve project root: {error}"),
            Span::default(),
        )
    })?;
    if !root.is_dir() {
        return Err(diagnostic(
            "E_PROJECT_ROOT",
            "project root must be a directory",
            Span::default(),
        ));
    }
    let entry = entry.canonicalize().map_err(|error| {
        diagnostic(
            "E_PROJECT_ENTRY",
            format!("cannot resolve project entry: {error}"),
            Span::default(),
        )
    })?;
    if !entry.starts_with(&root) || !entry.is_file() {
        return Err(diagnostic(
            "E_PROJECT_ENTRY",
            "project entry must be a file inside its root",
            Span::default(),
        ));
    }
    let stdlib_root = stdlib_root
        .map(Path::canonicalize)
        .transpose()
        .map_err(|error| {
            diagnostic(
                "E_IMPORT_ROOT",
                format!("cannot resolve standard-library root: {error}"),
                Span::default(),
            )
        })?;
    let mut canonical_packages = HashMap::new();
    for (name, path) in package_roots {
        if name.is_empty() || name.contains('/') || name.contains('\\') {
            return Err(diagnostic(
                "E_IMPORT_ROOT",
                format!("invalid package root name `{name}`"),
                Span::default(),
            ));
        }
        let canonical = path.canonicalize().map_err(|error| {
            diagnostic(
                "E_IMPORT_ROOT",
                format!("cannot resolve package root `{name}`: {error}"),
                Span::default(),
            )
        })?;
        if !canonical.is_dir() {
            return Err(diagnostic(
                "E_IMPORT_ROOT",
                format!("package root `{name}` is not a directory"),
                Span::default(),
            ));
        }
        canonical_packages.insert(name.clone(), canonical);
    }
    let mut loader = ModuleLoader {
        stdlib_root: stdlib_root.clone(),
        package_roots: canonical_packages,
        nodes: Vec::new(),
        by_path: HashMap::new(),
        active: HashSet::new(),
        total_source_bytes: 0,
        profile_enabled,
        module_profiles: Vec::new(),
    };
    let entry_name = entry
        .strip_prefix(&root)
        .unwrap_or(&entry)
        .to_string_lossy()
        .into_owned();
    loader.load(&entry, &root, "", entry_name, None, 0)?;
    let mut module_profiles = loader.module_profiles;
    module_profiles.sort_by(|left, right| left.source_name.cmp(&right.source_name));
    let compiled_source_sha256 = digest_sources(
        loader
            .nodes
            .iter()
            .map(|node| (node.source_name.clone(), node.source.as_str()))
            .collect(),
    );
    let nodes = loader.nodes;

    let sources = nodes
        .iter()
        .map(|node| node.source_name.clone())
        .collect::<Vec<_>>();
    let mut program = Program {
        functions: Vec::new(),
        externals: Vec::new(),
        records: Vec::new(),
        imports: Vec::new(),
        array_elements: Vec::new(),
    };
    for node in &nodes {
        program.functions.extend(node.program.functions.clone());
        program.externals.extend(node.program.externals.clone());
        program.records.extend(node.program.records.clone());
        program
            .array_elements
            .extend(node.program.array_elements.clone());
    }
    let mut public_exports = HashMap::new();
    for module_id in 0..nodes.len() {
        public_module_exports(module_id, &nodes, &sources, &mut public_exports)?;
    }

    let mut scopes = HashMap::new();
    for (module_id, node) in nodes.iter().enumerate() {
        let mut scope = module_scope(&node.program);
        let mut imported_functions = HashMap::<String, usize>::new();
        for (imported_id, import_span, _) in &node.imports {
            let imported = &public_exports[imported_id];
            for (name, owner) in &imported.functions {
                if let Some(previous_owner) = imported_functions.get(name) {
                    if *previous_owner != *owner
                        && !module_scope(&node.program).functions.contains_key(name)
                    {
                        return Err(diagnostic(
                            "E_NAME_AMBIGUOUS",
                            format!(
                                "public function {name} is exported by more than one direct import"
                            ),
                            *import_span,
                        )
                        .with_source_name(sources[module_id].clone()));
                    }
                } else {
                    imported_functions.insert(name.clone(), *owner);
                }
                if !scope.functions.contains_key(name) {
                    scope.functions.insert(name.clone(), *owner);
                }
            }
            scope.records.extend(imported.records.keys().cloned());
        }
        scopes.insert(module_id, scope);
    }
    let compile_program_started = profile_enabled.then(Instant::now);
    let mut artifact =
        compile_program(&sources[0], sources.clone(), program, scopes).map_err(|error| {
            let name = sources
                .get(error.span.source_id as usize)
                .cloned()
                .unwrap_or_else(|| sources[0].clone());
            error.with_source_name(name)
        })?;
    let compile_program_ns = compile_program_started
        .map(|started| started.elapsed().as_nanos())
        .unwrap_or_default();
    artifact.compiled_source_sha256 = Some(compiled_source_sha256);
    Ok((
        artifact,
        CompileProfile {
            modules: module_profiles,
            compile_program_ns,
        },
    ))
}

fn digest_sources(mut sources: Vec<(String, &str)>) -> String {
    sources.sort_by(|left, right| left.0.cmp(&right.0));
    let mut digest = Sha256::new();
    digest.update((sources.len() as u64).to_be_bytes());
    for (name, source) in sources {
        digest.update((name.len() as u64).to_be_bytes());
        digest.update(name.as_bytes());
        digest.update((source.len() as u64).to_be_bytes());
        digest.update(source.as_bytes());
    }
    format!("{:x}", digest.finalize())
}

fn compile_program(
    source_name: &str,
    sources: Vec<String>,
    program: Program,
    scopes: HashMap<usize, ModuleScope>,
) -> Result<Artifact, Diagnostic> {
    let reserved = [
        "Int",
        "Float64",
        "Bool",
        "Text",
        "Bytes",
        "Unit",
        "Json",
        "JsonError",
        "FileError",
        "TaskError",
        "List",
        "Map",
        "Option",
        "Result",
    ];
    let mut record_names = std::collections::HashSet::new();
    for record in &program.records {
        if reserved.contains(&record.name.as_str()) || !record_names.insert(record.name.clone()) {
            return Err(diagnostic(
                "E_RECORD_NAME",
                format!(
                    "record name `{}` is reserved or already declared",
                    record.name
                ),
                record.span,
            ));
        }
    }
    if let Some((module_id, name, span)) = program.array_elements.first() {
        return Err(if scopes[module_id].records.contains(name) {
            diagnostic("E_TYPE_ANNOTATION", ARRAY_ELEMENT_TYPES_MESSAGE, *span)
        } else {
            diagnostic("E_TYPE_UNKNOWN", format!("unknown type `{name}`"), *span)
        });
    }
    for record in &program.records {
        let visible = &scopes[&record.module_id].records;
        for (_, ty, span) in &record.fields {
            if !type_is_known(ty, &record_names) || !type_is_visible(ty, visible) {
                return Err(diagnostic(
                    "E_TYPE_UNKNOWN",
                    format!("unknown type `{ty:?}`"),
                    *span,
                ));
            }
        }
    }
    let mut signatures: HashMap<String, (usize, Vec<Type>, Type)> = HashMap::new();
    let mut declared_function_names = HashSet::new();
    for (idx, f) in program.functions.iter().enumerate() {
        let scope = &scopes[&f.module_id];
        if record_names.contains(&f.name) {
            return Err(diagnostic(
                "E_NAME_DUPLICATE",
                format!("`{}` is both a function and a record", f.name),
                f.span,
            ));
        }
        for (_, ty, span) in &f.params {
            if !type_is_known(ty, &record_names) || !type_is_visible(ty, &scope.records) {
                return Err(diagnostic(
                    "E_TYPE_UNKNOWN",
                    format!("unknown type `{ty:?}`"),
                    *span,
                ));
            }
        }
        if !type_is_known(&f.result, &record_names) || !type_is_visible(&f.result, &scope.records) {
            return Err(diagnostic(
                "E_TYPE_UNKNOWN",
                format!("unknown type `{:?}`", f.result),
                f.span,
            ));
        }
        if !declared_function_names.insert((f.module_id, f.name.clone())) {
            return Err(diagnostic(
                "E_NAME_DUPLICATE",
                format!(
                    "function {} is declared more than once in this module",
                    f.name
                ),
                f.span,
            ));
        }
        signatures.insert(
            function_symbol(f.module_id, &f.name),
            (
                idx,
                f.params.iter().map(|p| p.1.clone()).collect(),
                f.result.clone(),
            ),
        );
    }
    let mut external_signatures: HashMap<String, ExternalSignature> = HashMap::new();
    let mut declared_external_names = HashSet::new();
    for (idx, external) in program.externals.iter().enumerate() {
        let name = &external.name;
        let module_scope = &scopes[&external.module_id];
        if declared_function_names.contains(&(external.module_id, name.clone()))
            || record_names.contains(name)
            || !declared_external_names.insert((external.module_id, name.clone()))
        {
            return Err(diagnostic(
                "E_NAME_DUPLICATE",
                format!(
                    "function {} is declared more than once in this module",
                    name
                ),
                external.span,
            ));
        }
        if module_scope
            .functions
            .get(name)
            .is_some_and(|owner| *owner != external.module_id)
        {
            return Err(diagnostic(
                "E_NAME_AMBIGUOUS",
                format!(
                    "external function {} conflicts with an imported function",
                    name
                ),
                external.span,
            ));
        }
        external_signatures.insert(
            function_symbol(external.module_id, name),
            (
                idx,
                external
                    .params
                    .iter()
                    .map(|p| (p.0.clone(), p.1.clone()))
                    .collect(),
                external.result.clone(),
            ),
        );
        let mut names = HashMap::new();
        if external
            .params
            .iter()
            .any(|(_, ty, _)| !type_is_known(ty, &record_names))
            || !type_is_known(&external.result, &record_names)
        {
            return Err(diagnostic(
                "E_TYPE_UNKNOWN",
                "external declaration refers to an unknown type",
                external.span,
            ));
        }
        if !external
            .params
            .iter()
            .all(|parameter| python_bridge_type(&parameter.1))
            || !python_bridge_type(&external.result)
        {
            return Err(diagnostic(
                "E_EXTERN_TYPE",
                "current Python declarations support scalar values, Json, and nested List/Map values",
                external.span,
            ));
        }
        for (parameter, _, span) in &external.params {
            if names.insert(parameter, ()).is_some() {
                return Err(diagnostic(
                    "E_NAME_DUPLICATE",
                    format!("duplicate external parameter `{parameter}`"),
                    *span,
                ));
            }
        }
    }
    let Some(entry) = program
        .functions
        .iter()
        .position(|function| function.module_id == 0 && function.name == "main")
    else {
        return Err(diagnostic(
            "E_ENTRY_MISSING",
            "module must define main",
            Span {
                line: 1,
                column: 1,
                ..Span::default()
            },
        ));
    };
    let entry_function = &program.functions[entry];
    if !entry_function.params.is_empty() || !matches!(entry_function.result, Type::Int | Type::Unit)
    {
        return Err(diagnostic(
            "E_ENTRY_SIGNATURE",
            "main must have no parameters and return Int or Unit",
            entry_function.span,
        ));
    }
    let visible_functions_by_module: HashMap<usize, HashMap<String, String>> = scopes
        .iter()
        .map(|(module_id, scope)| {
            (
                *module_id,
                scope
                    .functions
                    .iter()
                    .map(|(name, owner)| (name.clone(), function_symbol(*owner, name)))
                    .collect(),
            )
        })
        .collect();
    let visible_externals_by_module: HashMap<usize, HashMap<String, String>> = scopes
        .iter()
        .map(|(module_id, scope)| {
            (
                *module_id,
                scope
                    .externals
                    .iter()
                    .map(|(name, owner)| (name.clone(), function_symbol(*owner, name)))
                    .collect(),
            )
        })
        .collect();
    let mut global_name_counts = HashMap::<&str, usize>::new();
    for function in &program.functions {
        *global_name_counts.entry(&function.name).or_default() += 1;
    }
    let standard_functions: HashSet<String> = program
        .functions
        .iter()
        .filter(|function| {
            sources
                .get(function.module_id)
                .is_some_and(|identity| identity.starts_with("std:"))
        })
        .map(|function| function_symbol(function.module_id, &function.name))
        .collect();
    let mut functions = Vec::new();
    for f in &program.functions {
        let mut compiler = FnCompiler {
            ast: f,
            signatures: &signatures,
            external_signatures: &external_signatures,
            records: &program.records,
            visible_functions: &visible_functions_by_module[&f.module_id],
            visible_externals: &visible_externals_by_module[&f.module_id],
            visible_records: &scopes[&f.module_id].records,
            standard_functions: &standard_functions,
            locals: HashMap::new(),
            next_local: 0,
            code: Vec::new(),
            loops: Vec::new(),
            boundary_args: Vec::new(),
            split_targets: split_targets_of(&f.body),
        };
        for (name, ty, span) in &f.params {
            let slot = compiler.next_local;
            compiler.next_local += 1;
            if compiler
                .locals
                .insert(
                    name.clone(),
                    Local {
                        slot,
                        ty: ty.clone(),
                        mutable: false,
                        split: None,
                    },
                )
                .is_some()
            {
                return Err(diagnostic(
                    "E_NAME_DUPLICATE",
                    format!("duplicate parameter `{name}`"),
                    *span,
                ));
            }
        }
        compiler.stmts(&f.body)?;
        if f.result == Type::Unit {
            compiler.emit(Op::Const(Value::Unit), f.span);
            compiler.emit(Op::Return, f.span);
        } else if !block_returns(&f.body) {
            return Err(diagnostic(
                "E_RETURN_MISSING",
                format!("function `{}` must return a value on every path", f.name),
                f.span,
            ));
        }
        let display_name = if global_name_counts
            .get(f.name.as_str())
            .copied()
            .unwrap_or(0)
            > 1
            && !(f.module_id == 0 && f.name == "main")
        {
            format!("@module/{}/{}", f.module_id, f.name)
        } else {
            f.name.clone()
        };
        functions.push(Function {
            name: display_name,
            parameters: f.params.iter().map(|p| p.1.clone()).collect(),
            result: f.result.clone(),
            local_count: compiler.next_local,
            code: compiler.code,
        });
    }
    let artifact = Artifact {
        format_version: Artifact::CURRENT_VERSION,
        build_metadata: None,
        compiled_source_sha256: None,
        source_name: source_name.into(),
        sources,
        entry: "main".into(),
        functions,
        externals: program
            .externals
            .iter()
            .map(|external| ExternalFunction {
                name: external.name.clone(),
                module: external.module.clone(),
                callable: external.callable.clone(),
                parameter_names: external
                    .params
                    .iter()
                    .map(|parameter| parameter.0.clone())
                    .collect(),
                parameters: external
                    .params
                    .iter()
                    .map(|parameter| parameter.1.clone())
                    .collect(),
                result: external.result.clone(),
            })
            .collect(),
        records: program
            .records
            .iter()
            .map(|record| RecordDefinition {
                name: record.name.clone().into(),
                fields: record
                    .fields
                    .iter()
                    .map(|(name, ty, _)| RecordField {
                        name: name.clone(),
                        ty: ty.clone(),
                    })
                    .collect(),
            })
            .collect(),
    };
    guion_model::validate_sort_comparator_effects(&artifact)
        .map_err(|message| diagnostic("E_SORT_EFFECT", message, Span::default()))?;
    Ok(artifact)
}

/// What a builtin's non-generic argument is, for the `E_FN_NOT_SERIALIZABLE` message.
fn boundary_label(name: &str) -> String {
    let label = if name.starts_with("json_") {
        "a JSON conversion"
    } else if name.starts_with("csv_") {
        "a CSV conversion"
    } else if name.starts_with("native_toml_") {
        "a TOML conversion"
    } else if name.starts_with("native_sqlite_") {
        "a SQLite parameter or result"
    } else if name.starts_with("native_process_") || name == "args" || name == "env" {
        "process arguments or the environment"
    } else if name.starts_with("http_") {
        "an HTTP request or response"
    } else if name == "print" || name == "eprint" {
        "program output"
    } else if name.starts_with("read_") || name.starts_with("write_") {
        "a file"
    } else {
        return format!("the builtin `{name}`");
    };
    label.to_owned()
}

fn type_is_known(ty: &Type, records: &std::collections::HashSet<String>) -> bool {
    match ty {
        Type::Record(name) => records.contains(name),
        Type::NativeResource(name) => matches!(
            name.as_str(),
            "FileReader" | "Db" | "Transaction" | "TomlDocument"
        ),
        Type::List(item) | Type::Map(item) | Type::Option(item) => type_is_known(item, records),
        Type::Result(ok, err) => type_is_known(ok, records) && type_is_known(err, records),
        Type::Fn(parameters, result) => {
            parameters
                .iter()
                .all(|parameter| type_is_known(parameter, records))
                && type_is_known(result, records)
        }
        _ => true,
    }
}

fn type_is_visible(ty: &Type, records: &HashSet<String>) -> bool {
    match ty {
        Type::Record(name) => records.contains(name),
        Type::NativeResource(name) => matches!(
            name.as_str(),
            "FileReader" | "Db" | "Transaction" | "TomlDocument"
        ),
        Type::List(item) | Type::Map(item) | Type::Option(item) => type_is_visible(item, records),
        Type::Result(ok, err) => type_is_visible(ok, records) && type_is_visible(err, records),
        Type::Fn(parameters, result) => {
            parameters
                .iter()
                .all(|parameter| type_is_visible(parameter, records))
                && type_is_visible(result, records)
        }
        _ => true,
    }
}

fn python_bridge_type(ty: &Type) -> bool {
    match ty {
        Type::Int
        | Type::Float64
        | Type::Bool
        | Type::Text
        | Type::Bytes
        | Type::Unit
        | Type::Json => true,
        Type::List(item) => python_bridge_type(item),
        Type::Map(item) => python_bridge_type(item),
        Type::Record(_) => false,
        Type::NativeResource(_) | Type::Array(_) | Type::Fn(_, _) => false,
        Type::Option(_)
        | Type::Result(_, _)
        | Type::TaskError
        | Type::JsonError
        | Type::FileError
        | Type::HttpError => false,
    }
}

fn type_contains_native_resource(ty: &Type) -> bool {
    match ty {
        Type::NativeResource(_) => true,
        Type::List(item) | Type::Map(item) | Type::Option(item) => {
            type_contains_native_resource(item)
        }
        Type::Result(ok, err) => {
            type_contains_native_resource(ok) || type_contains_native_resource(err)
        }
        Type::Fn(parameters, result) => {
            parameters.iter().any(type_contains_native_resource)
                || type_contains_native_resource(result)
        }
        _ => false,
    }
}

fn block_returns(body: &[Stmt]) -> bool {
    match body.last() {
        Some(Stmt::Return(_, _)) => true,
        Some(Stmt::If(_, yes, no, _)) => !no.is_empty() && block_returns(yes) && block_returns(no),
        Some(Stmt::Match(_, arms, _)) => {
            arms.len() == 2 && arms.iter().all(|arm| block_returns(&arm.body))
        }
        _ => false,
    }
}
