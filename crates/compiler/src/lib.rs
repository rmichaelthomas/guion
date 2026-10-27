pub use guion_model::Diagnostic;
use guion_model::{
    Artifact, BigInt, ExternalFunction, Float64, Function, Instruction, Op, RecordDefinition,
    RecordField, Span, Type, Value,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Component, Path, PathBuf},
};

mod builtin_catalog;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Kind {
    Name,
    Number,
    Text,
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
            if let Some(prefix_len) = python_fstring_prefix(bytes, i) {
                return Err(diagnostic(
                    "E_PYTHON_FSTRING",
                    "Python f-strings are not supported; use text concatenation with `+` for `Text` values. General numeric interpolation has no built-in text formatter yet.",
                    Span {
                        source_id,
                        start: offset + i,
                        end: offset + i + prefix_len,
                        line: row + 1,
                        column: line[..i].chars().count() + 1,
                    },
                ));
            } else if c.is_ascii_alphabetic() || c == '_' {
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
            } else if c == '"' {
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
                kind = Kind::Text;
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
                "Python `elif` is not Guion syntax; write a nested `if` inside `else`.",
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

    for window in tokens.windows(5) {
        if window[0].kind == Kind::Name
            && window[0].text == "for"
            && window[1].kind == Kind::Name
            && window[2].kind == Kind::Name
            && window[2].text == "in"
            && window[3].kind == Kind::Name
            && window[3].text == "range"
            && window[4].kind == Kind::Symbol
            && window[4].text == "("
        {
            return Some(diagnostic(
                "E_PYTHON_RANGE",
                "Python `range()` is not a built-in; use `for i in [0, 1, 2]` for a fixed sequence, or a `while` loop with an explicit counter.",
                window[3].span,
            ));
        }
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
}
#[derive(Clone, Debug)]
struct ImportAst {
    path: String,
    span: Span,
}
#[derive(Clone, Debug)]
struct RecordAst {
    name: String,
    fields: Vec<(String, Type, Span)>,
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
        }
    }
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
    module_id: usize,
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
            _ => Ok(Type::Record(t.text)),
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
        let mut names = std::collections::HashSet::new();
        while self.cur().kind != Kind::Dedent && self.cur().kind != Kind::Eof {
            if self.cur().kind == Kind::Newline {
                self.advance();
                continue;
            }
            let field = self.name()?;
            self.expect(":")?;
            let ty = self.ty()?;
            self.expect_newline()?;
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
            if self.cur().text == "import" {
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
        if functions.is_empty() {
            return Err(diagnostic(
                "E_MODULE_EMPTY",
                "module must define at least one function",
                self.cur().span,
            ));
        }
        Ok(Program {
            functions,
            externals,
            records,
            imports,
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
                self.expect("=")?;
                let e = self.expr(0)?;
                self.expect_newline()?;
                Ok(Stmt::Set(n.text, e, t.span))
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
                    self.expect(":")?;
                    self.expect_newline()?;
                    self.block()?
                } else {
                    Vec::new()
                };
                Ok(Stmt::If(c, yes, no, t.span))
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
            Kind::Name if t.text == "true" => Expr::Bool(true, t.span),
            Kind::Name if t.text == "false" => Expr::Bool(false, t.span),
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

#[derive(Clone)]
struct Local {
    slot: usize,
    ty: Type,
    mutable: bool,
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
    locals: HashMap<String, Local>,
    next_local: usize,
    code: Vec<Instruction>,
}
impl<'a> FnCompiler<'a> {
    fn emit(&mut self, op: Op, span: Span) -> usize {
        let i = self.code.len();
        self.code.push(Instruction { op, span });
        i
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

    fn expr_with_expected(
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
                    self.emit(Op::MakeNone, *s);
                    return Ok(option_type.clone());
                }
                let l = self.locals.get(n).cloned().ok_or_else(|| {
                    diagnostic("E_NAME_UNKNOWN", format!("unknown name `{n}`"), *s)
                })?;
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
            Expr::Call(name, args, keywords, s) => {
                if let Some(signature) = builtin_catalog::get(name) {
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
                    let Expr::Name(worker_name, worker_span) = &args[1] else {
                        return Err(diagnostic(
                            "E_TYPE_ARGUMENT",
                            "parallel_map worker must be a named function",
                            args[1].span(),
                        ));
                    };
                    let Some(worker_symbol) = self.visible_functions.get(worker_name) else {
                        return Err(diagnostic(
                            "E_NAME_UNKNOWN",
                            format!("unknown worker function `{worker_name}`"),
                            *worker_span,
                        ));
                    };
                    let Some((worker_index, worker_params, worker_result)) =
                        self.signatures.get(worker_symbol).cloned()
                    else {
                        return Err(diagnostic(
                            "E_NAME_UNKNOWN",
                            format!("unknown worker function `{worker_name}`"),
                            *worker_span,
                        ));
                    };
                    if worker_params.iter().any(type_contains_native_resource)
                        || type_contains_native_resource(&worker_result)
                    {
                        return Err(diagnostic(
                            "E_PARALLEL_RESOURCE",
                            "parallel workers cannot accept or return native resources",
                            *worker_span,
                        ));
                    }
                    if worker_params != vec![item_type] {
                        return Err(diagnostic(
                            "E_TYPE_ARGUMENT",
                            "parallel_map worker must accept one item of the list's item type",
                            *worker_span,
                        ));
                    }
                    if self.expr(&args[2])? != Type::Int {
                        return Err(diagnostic(
                            "E_TYPE_ARGUMENT",
                            "parallel_map worker limit must be Int",
                            args[2].span(),
                        ));
                    }
                    self.emit(Op::ParallelMap(worker_index), *s);
                    return Ok(Type::Result(
                        Box::new(Type::List(Box::new(worker_result))),
                        Box::new(Type::TaskError),
                    ));
                }
                if name == "sort_by" {
                    if args.len() != 2 {
                        return Err(diagnostic(
                            "E_CALL_ARITY",
                            "sort_by expects a list and a named comparator",
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
                    let Expr::Name(comparator_name, comparator_span) = &args[1] else {
                        return Err(diagnostic(
                            "E_SORT_COMPARATOR",
                            "sort_by comparator must be a named function",
                            args[1].span(),
                        ));
                    };
                    let Some(comparator_symbol) = self.visible_functions.get(comparator_name)
                    else {
                        return Err(diagnostic(
                            "E_NAME_UNKNOWN",
                            format!("unknown comparator `{comparator_name}`"),
                            *comparator_span,
                        ));
                    };
                    let Some((comparator_index, parameters, result)) =
                        self.signatures.get(comparator_symbol).cloned()
                    else {
                        return Err(diagnostic(
                            "E_NAME_UNKNOWN",
                            format!("unknown comparator `{comparator_name}`"),
                            *comparator_span,
                        ));
                    };
                    if parameters != vec![item_type.clone(), item_type.clone()]
                        || result != Type::Int
                    {
                        return Err(diagnostic("E_SORT_COMPARATOR", "sort_by comparator must have type (T, T) -> Int for the list item type", *comparator_span));
                    }
                    self.emit(Op::SortBy(comparator_index), *s);
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
                    return Err(diagnostic(
                        "E_NAME_UNKNOWN",
                        format!("unknown function `{name}`"),
                        *s,
                    ));
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
                    self.locals.insert(
                        name.clone(),
                        Local {
                            slot,
                            ty,
                            mutable: *mutable,
                        },
                    );
                }
                Stmt::Set(name, value, span) => {
                    let local = self.locals.get(name).cloned().ok_or_else(|| {
                        diagnostic("E_NAME_UNKNOWN", format!("unknown name `{name}`"), *span)
                    })?;
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
                    let optimized_local_list_update =
                        optimized_list_push.is_some() || optimized_list_set.is_some();
                    let optimized_local_update =
                        optimized_local_list_update || optimized_json_push.is_some();
                    let ty = if optimized_local_update {
                        local.ty.clone()
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
                        optimized_json_push
                            .or(optimized_list_push)
                            .or(optimized_list_set)
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
                    self.expr(&args[0])?;
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
                    self.stmts(body)?;
                    self.locals = before;
                    self.emit(Op::Jump(start), *span);
                    self.patch(exit, self.code.len());
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
                        },
                    );
                    self.stmts(body)?;
                    self.locals = old_locals;
                    self.emit(Op::Load(index_slot), *span);
                    self.emit(Op::Const(Value::Int(BigInt::one())), *span);
                    self.emit(Op::Add, *span);
                    self.emit(Op::Store(index_slot), *span);
                    self.emit(Op::Jump(start), *span);
                    self.patch(exit, self.code.len());
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
    imports: Vec<(usize, Span)>,
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

        let source = fs::read_to_string(&canonical).map_err(|error| {
            diagnostic(
                "E_IMPORT_READ",
                format!("cannot read module {}: {error}", canonical.display()),
                Span::default(),
            )
        })?;
        let index = self.nodes.len();
        let mut program = parse_program(&source, index as u32)
            .map_err(|error| error.with_source_name(source_name.clone()))?;
        let import_paths = std::mem::take(&mut program.imports);
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
        for import in import_paths {
            let (imported, child_root, child_label, child_name) =
                self.resolve_import(index, &import.path, import.span)?;
            let importer_name = self.nodes[index].source_name.clone();
            let child_index = self.load(
                &imported,
                &child_root,
                &child_label,
                child_name,
                Some((index as u32, importer_name)),
                depth + 1,
            )?;
            imports.push((child_index, import.span));
        }
        self.active.remove(&canonical);
        self.nodes[index].imports = imports;
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
    };
    let entry_name = entry
        .strip_prefix(&root)
        .unwrap_or(&entry)
        .to_string_lossy()
        .into_owned();
    loader.load(&entry, &root, "", entry_name, None, 0)?;
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
    };
    for node in &nodes {
        program.functions.extend(node.program.functions.clone());
        program.externals.extend(node.program.externals.clone());
        program.records.extend(node.program.records.clone());
    }
    let mut scopes = HashMap::new();
    for (module_id, node) in nodes.iter().enumerate() {
        let mut scope = module_scope(&node.program);
        let mut imported_functions = HashMap::<String, usize>::new();
        for (imported_id, import_span) in &node.imports {
            let imported = &nodes[*imported_id].program;
            for function in imported.functions.iter().filter(|function| function.public) {
                if let Some(previous_owner) = imported_functions.get(&function.name) {
                    if *previous_owner != function.module_id
                        && !module_scope(&node.program)
                            .functions
                            .contains_key(&function.name)
                    {
                        return Err(diagnostic(
                            "E_NAME_AMBIGUOUS",
                            format!(
                                "public function {} is exported by more than one direct import",
                                function.name
                            ),
                            *import_span,
                        )
                        .with_source_name(sources[module_id].clone()));
                    }
                } else {
                    imported_functions.insert(function.name.clone(), function.module_id);
                }
                if !scope.functions.contains_key(&function.name) {
                    scope
                        .functions
                        .insert(function.name.clone(), function.module_id);
                }
            }
            scope.records.extend(
                imported
                    .records
                    .iter()
                    .filter(|record| record.public)
                    .map(|record| record.name.clone()),
            );
        }
        scopes.insert(module_id, scope);
    }
    let mut artifact =
        compile_program(&sources[0], sources.clone(), program, scopes).map_err(|error| {
            let name = sources
                .get(error.span.source_id as usize)
                .cloned()
                .unwrap_or_else(|| sources[0].clone());
            error.with_source_name(name)
        })?;
    artifact.compiled_source_sha256 = Some(compiled_source_sha256);
    Ok(artifact)
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
            locals: HashMap::new(),
            next_local: 0,
            code: Vec::new(),
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
                name: record.name.clone(),
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

fn type_is_known(ty: &Type, records: &std::collections::HashSet<String>) -> bool {
    match ty {
        Type::Record(name) => records.contains(name),
        Type::NativeResource(name) => matches!(
            name.as_str(),
            "FileReader" | "Db" | "Transaction" | "TomlDocument"
        ),
        Type::List(item) | Type::Map(item) | Type::Option(item) => type_is_known(item, records),
        Type::Result(ok, err) => type_is_known(ok, records) && type_is_known(err, records),
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
        Type::NativeResource(_) => false,
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
