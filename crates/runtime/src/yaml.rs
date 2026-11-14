//! A read-only reader for a deliberately small subset of YAML, behind `std:yaml`.
//!
//! Supported: block mappings and block sequences, plain, single-quoted and double-quoted
//! scalars (quoted scalars may span lines), literal `|` and folded `>` block scalars with the
//! chomping and indentation indicators, flow sequences and flow mappings, comments, nesting,
//! and one optional leading `---` and trailing `...`.
//!
//! Rejected with `YAML_UNSUPPORTED`, naming the feature and its line and column and returning
//! no partial value: anchors, aliases, tags, the merge key `<<`, complex `?` keys, directives,
//! and a stream of more than one document. Malformed input is `YAML_SYNTAX`, a repeated key in
//! one mapping is `YAML_DUPLICATE_KEY`, and nesting past 64 levels is `YAML_LIMIT`.
//!
//! Scalar policy: a plain scalar is text, except the lowercase spellings `true` and `false`
//! (booleans), `null` and `~` (null), and an integer or decimal that reads back unchanged
//! (`42`, `-7`, `2.75`; `1.10` would become `1.1`, so it stays text, as do `007`, `1e3`, `0x1F`
//! and integers that do not fit 64 bits). `NO`, `yes`, `on` and `Null` are text. A quoted
//! scalar and every mapping key is always text.

use serde_json::{Map, Number, Value as Json};

const MAX_DEPTH: usize = 64;

/// A typed failure with a 1-based position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct YamlError {
    pub code: &'static str,
    pub message: String,
    pub line: usize,
    pub column: usize,
}

fn error(code: &'static str, line: usize, column: usize, message: impl Into<String>) -> YamlError {
    YamlError {
        code,
        message: message.into(),
        line: line + 1,
        column: column + 1,
    }
}

fn syntax(line: usize, column: usize, message: impl Into<String>) -> YamlError {
    error("YAML_SYNTAX", line, column, message)
}

fn unsupported(line: usize, column: usize, feature: impl AsRef<str>) -> YamlError {
    error(
        "YAML_UNSUPPORTED",
        line,
        column,
        format!("unsupported YAML feature: {}", feature.as_ref()),
    )
}

fn too_deep(line: usize, column: usize) -> YamlError {
    error(
        "YAML_LIMIT",
        line,
        column,
        format!("nesting is deeper than {MAX_DEPTH} levels"),
    )
}

fn is_ws(c: char) -> bool {
    c == ' ' || c == '\t'
}

/// Parses one YAML document into JSON.
pub(crate) fn parse(text: &str) -> Result<Json, YamlError> {
    let mut parser = Parser::new(text);
    parser.document()
}

struct Parser {
    lines: Vec<Vec<char>>,
    /// Column where the unread part of each line starts. A sequence entry such as `- key: v`
    /// moves this past the dash so the rest reads as a node of its own.
    starts: Vec<usize>,
    cur: usize,
    depth: usize,
}

impl Parser {
    fn new(text: &str) -> Self {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let lines: Vec<Vec<char>> = text
            .split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line).chars().collect())
            .collect();
        let starts = lines
            .iter()
            .map(|line| line.iter().take_while(|c| **c == ' ').count())
            .collect();
        Parser {
            lines,
            starts,
            cur: 0,
            depth: 0,
        }
    }

    fn document(&mut self) -> Result<Json, YamlError> {
        if let Some(indent) = self.peek()? {
            if self.lines[self.cur][indent] == '%' && indent == 0 {
                return Err(unsupported(self.cur, 0, "directive"));
            }
            if self.doc_marker(self.cur) && self.lines[self.cur][0] == '-' {
                let line = &self.lines[self.cur];
                let rest: String = line[3..].iter().collect();
                let rest = rest.trim();
                if !rest.is_empty() && !rest.starts_with('#') {
                    return Err(unsupported(
                        self.cur,
                        4,
                        "content on the document start line",
                    ));
                }
                self.cur += 1;
            }
        }
        let value = self.node(-1)?;
        if let Some(indent) = self.peek()? {
            if self.doc_marker(self.cur) {
                let end_marker = self.lines[self.cur][0] == '.';
                let rest_blank =
                    (self.cur + 1..self.lines.len()).all(|index| self.is_blank_or_comment(index));
                let marker_rest: String = self.lines[self.cur][3..].iter().collect();
                let marker_rest = marker_rest.trim();
                let marker_clean = marker_rest.is_empty() || marker_rest.starts_with('#');
                if end_marker && rest_blank && marker_clean {
                    return Ok(value);
                }
                return Err(unsupported(
                    self.cur,
                    0,
                    "multi-document stream (more than one document)",
                ));
            }
            return Err(syntax(
                self.cur,
                indent,
                "unexpected content; check the indentation of this line",
            ));
        }
        Ok(value)
    }

    fn is_blank_or_comment(&self, index: usize) -> bool {
        let line = &self.lines[index];
        match line[self.starts[index]..].iter().find(|c| !is_ws(**c)) {
            None => true,
            Some(c) => *c == '#',
        }
    }

    /// Skips blank and comment lines. Returns the column of the next structural line, or
    /// `None` at the end of input.
    fn peek(&mut self) -> Result<Option<usize>, YamlError> {
        while self.cur < self.lines.len() && self.is_blank_or_comment(self.cur) {
            self.cur += 1;
        }
        if self.cur >= self.lines.len() {
            return Ok(None);
        }
        let start = self.starts[self.cur];
        if self.lines[self.cur][start] == '\t' {
            return Err(syntax(
                self.cur,
                start,
                "tab characters cannot be used for indentation",
            ));
        }
        Ok(Some(start))
    }

    fn doc_marker(&self, index: usize) -> bool {
        let line = &self.lines[index];
        if self.starts[index] != 0 || line.len() < 3 {
            return false;
        }
        let head: String = line[..3].iter().collect();
        (head == "---" || head == "...") && line.get(3).is_none_or(|c| is_ws(*c))
    }

    fn is_seq_at(&self, index: usize, col: usize) -> bool {
        let line = &self.lines[index];
        line.get(col) == Some(&'-') && line.get(col + 1).is_none_or(|c| *c == ' ')
    }

    fn enter(&mut self, line: usize, column: usize) -> Result<(), YamlError> {
        if self.depth >= MAX_DEPTH {
            return Err(too_deep(line, column));
        }
        self.depth += 1;
        Ok(())
    }

    /// Reads the node that starts at the next structural line, if that line is indented
    /// deeper than `parent`; otherwise the node is absent and reads as null.
    fn node(&mut self, parent: isize) -> Result<Json, YamlError> {
        let Some(indent) = self.peek()? else {
            return Ok(Json::Null);
        };
        if self.doc_marker(self.cur) || (indent as isize) <= parent {
            return Ok(Json::Null);
        }
        let line = &self.lines[self.cur];
        let first = line[indent];
        let next = line.get(indent + 1).copied();
        match first {
            '&' | '*' | '!' => return Err(self.indicator(self.cur, indent)),
            '?' if next.is_none_or(is_ws) => {
                return Err(unsupported(self.cur, indent, "complex mapping key (`?`)"));
            }
            _ => {}
        }
        if self.is_seq_at(self.cur, indent) {
            return self.sequence(indent);
        }
        if self.key_at(self.cur, indent)?.is_some() {
            return self.mapping(indent);
        }
        self.inline(indent, parent)
    }

    /// The `YAML_UNSUPPORTED` error for an anchor, alias or tag that starts at `col`.
    fn indicator(&self, line: usize, col: usize) -> YamlError {
        let chars = &self.lines[line];
        let token: String = chars[col..]
            .iter()
            .take_while(|c| !is_ws(**c) && !matches!(**c, ',' | ']' | '}'))
            .collect();
        let kind = match chars[col] {
            '&' => "anchor",
            '*' => "alias",
            _ => "tag",
        };
        unsupported(line, col, format!("{kind} `{token}`"))
    }

    fn sequence(&mut self, indent: usize) -> Result<Json, YamlError> {
        self.enter(self.cur, indent)?;
        let mut items = Vec::new();
        while let Some(found) = self.peek()? {
            if self.doc_marker(self.cur) || found < indent {
                break;
            }
            if found > indent {
                return Err(syntax(
                    self.cur,
                    found,
                    "bad indentation: this line is indented more than the sequence entries",
                ));
            }
            if !self.is_seq_at(self.cur, indent) {
                break;
            }
            let line = &self.lines[self.cur];
            let mut after = indent + 1;
            while line.get(after) == Some(&' ') {
                after += 1;
            }
            if after >= line.len() || line[after] == '#' {
                self.cur += 1;
                let item = match self.peek()? {
                    Some(next) if !self.doc_marker(self.cur) && next > indent => {
                        self.node(indent as isize)?
                    }
                    _ => Json::Null,
                };
                items.push(item);
            } else {
                self.starts[self.cur] = after;
                items.push(self.node(indent as isize)?);
            }
        }
        self.depth -= 1;
        Ok(Json::Array(items))
    }

    fn mapping(&mut self, indent: usize) -> Result<Json, YamlError> {
        self.enter(self.cur, indent)?;
        let mut map = Map::new();
        while let Some(found) = self.peek()? {
            if self.doc_marker(self.cur) || found < indent {
                break;
            }
            if found > indent {
                return Err(syntax(
                    self.cur,
                    found,
                    "bad indentation: this line is indented more than the mapping entries",
                ));
            }
            let line_no = self.cur;
            let line = &self.lines[line_no];
            let first = line[indent];
            let next = line.get(indent + 1).copied();
            match first {
                '&' | '*' | '!' => return Err(self.indicator(line_no, indent)),
                '?' if next.is_none_or(is_ws) => {
                    return Err(unsupported(line_no, indent, "complex mapping key (`?`)"));
                }
                _ => {}
            }
            let Some((key, quoted, after)) = self.key_at(line_no, indent)? else {
                return Err(syntax(
                    line_no,
                    indent,
                    "expected a `key: value` entry at this indentation",
                ));
            };
            if !quoted && key == "<<" {
                return Err(unsupported(line_no, indent, "merge key `<<`"));
            }
            if map.contains_key(&key) {
                return Err(error(
                    "YAML_DUPLICATE_KEY",
                    line_no,
                    indent,
                    format!("duplicate key `{key}` in one mapping"),
                ));
            }
            let line = &self.lines[line_no];
            let mut at = after;
            while line.get(at).is_some_and(|c| is_ws(*c)) {
                at += 1;
            }
            let value = if at >= line.len() || line[at] == '#' {
                self.cur += 1;
                match self.peek()? {
                    Some(next) if !self.doc_marker(self.cur) && next > indent => {
                        self.node(indent as isize)?
                    }
                    Some(next)
                        if !self.doc_marker(self.cur)
                            && next == indent
                            && self.is_seq_at(self.cur, next) =>
                    {
                        self.sequence(next)?
                    }
                    _ => Json::Null,
                }
            } else {
                if self.is_seq_at(line_no, at) {
                    return Err(syntax(
                        line_no,
                        at,
                        "a block sequence cannot start on the same line as its key",
                    ));
                }
                self.inline(at, indent as isize)?
            };
            map.insert(key, value);
        }
        self.depth -= 1;
        Ok(Json::Object(map))
    }

    /// If the text at `col` is a `key:` entry, returns the key, whether it was quoted, and the
    /// column just after the colon.
    fn key_at(
        &self,
        line_no: usize,
        col: usize,
    ) -> Result<Option<(String, bool, usize)>, YamlError> {
        let line = &self.lines[line_no];
        match line[col] {
            '"' | '\'' => {
                let (text, end_line, end_col) = self.quoted(line_no, col)?;
                if end_line != line_no {
                    return Ok(None);
                }
                let mut at = end_col;
                while line.get(at).is_some_and(|c| is_ws(*c)) {
                    at += 1;
                }
                if line.get(at) == Some(&':') && line.get(at + 1).is_none_or(|c| is_ws(*c)) {
                    Ok(Some((text, true, at + 1)))
                } else {
                    Ok(None)
                }
            }
            '[' | '{' | '|' | '>' => Ok(None),
            _ => {
                let mut at = col;
                while at < line.len() {
                    let c = line[at];
                    if c == ':' && line.get(at + 1).is_none_or(|n| is_ws(*n)) {
                        let key: String = line[col..at].iter().collect();
                        let key = key.trim_end().to_owned();
                        if key.is_empty() {
                            return Err(syntax(line_no, col, "empty mapping key"));
                        }
                        return Ok(Some((key, false, at + 1)));
                    }
                    if c == '#' && at > col && is_ws(line[at - 1]) {
                        return Ok(None);
                    }
                    at += 1;
                }
                Ok(None)
            }
        }
    }

    /// Reads a value that starts at `col` on the current line and is not a block collection.
    /// Leaves `cur` on the line after the value.
    fn inline(&mut self, col: usize, parent: isize) -> Result<Json, YamlError> {
        let line_no = self.cur;
        match self.lines[line_no][col] {
            '&' | '*' | '!' => Err(self.indicator(line_no, col)),
            '|' | '>' => self.block_scalar(col, parent),
            '"' | '\'' => {
                let (text, end_line, end_col) = self.quoted(line_no, col)?;
                self.finish_line(end_line, end_col)?;
                Ok(Json::String(text))
            }
            '[' | '{' => {
                let (mut l, mut c) = (line_no, col);
                let value = self.flow(&mut l, &mut c, self.depth + 1)?;
                self.finish_line(l, c)?;
                Ok(value)
            }
            '@' | '`' => Err(syntax(
                line_no,
                col,
                "the characters `@` and the backtick are reserved and cannot start a scalar",
            )),
            _ => self.plain(col, parent),
        }
    }

    /// After a value that ended at (`line`, `col`), only whitespace and a comment may follow.
    fn finish_line(&mut self, line_no: usize, col: usize) -> Result<(), YamlError> {
        if line_no >= self.lines.len() {
            self.cur = self.lines.len();
            return Ok(());
        }
        let line = &self.lines[line_no];
        let mut at = col;
        while line.get(at).is_some_and(|c| is_ws(*c)) {
            at += 1;
        }
        if at < line.len() && !(line[at] == '#' && (at > col || col == 0)) {
            return Err(syntax(line_no, at, "unexpected text after the value"));
        }
        self.cur = line_no + 1;
        Ok(())
    }

    /// One line of a plain scalar from `col`: the text up to a comment, trimmed, and whether a
    /// comment ended it.
    fn plain_line(&self, line_no: usize, col: usize) -> Result<(String, bool), YamlError> {
        let line = &self.lines[line_no];
        let mut end = line.len();
        let mut commented = false;
        let mut at = col;
        while at < line.len() {
            let c = line[at];
            if c == '#' && at > col && is_ws(line[at - 1]) {
                end = at;
                commented = true;
                break;
            }
            at += 1;
        }
        let mut text: String = line[col..end].iter().collect();
        let trimmed = text.trim_end_matches(is_ws).len();
        text.truncate(trimmed);
        let chars: Vec<char> = text.chars().collect();
        for (offset, c) in chars.iter().enumerate() {
            if *c == ':' && chars.get(offset + 1).is_none_or(|n| is_ws(*n)) {
                return Err(syntax(
                    line_no,
                    col + offset,
                    "mapping values are not allowed here; quote the text if it contains `: `",
                ));
            }
        }
        Ok((text, commented))
    }

    fn plain(&mut self, col: usize, parent: isize) -> Result<Json, YamlError> {
        let (mut text, mut commented) = self.plain_line(self.cur, col)?;
        self.cur += 1;
        let mut multi = false;
        while !commented {
            let mut next = self.cur;
            let mut blanks = 0;
            while next < self.lines.len() && self.lines[next].iter().all(|c| is_ws(*c)) {
                next += 1;
                blanks += 1;
            }
            if next >= self.lines.len() || self.is_blank_or_comment(next) || self.doc_marker(next) {
                break;
            }
            let start = self.starts[next];
            if (start as isize) <= parent {
                break;
            }
            if self.lines[next][start] == '\t' {
                return Err(syntax(
                    next,
                    start,
                    "tab characters cannot be used for indentation",
                ));
            }
            let (segment, ended) = self.plain_line(next, start)?;
            if blanks == 0 {
                text.push(' ');
            } else {
                text.push_str(&"\n".repeat(blanks));
            }
            text.push_str(&segment);
            commented = ended;
            multi = true;
            self.cur = next + 1;
        }
        if multi {
            Ok(Json::String(text))
        } else {
            Ok(resolve_plain(&text))
        }
    }

    /// Reads a quoted scalar that opens at (`line`, `col`), across lines if needed. Returns
    /// the text and the position just after the closing quote.
    fn quoted(&self, line_no: usize, col: usize) -> Result<(String, usize, usize), YamlError> {
        let quote = self.lines[line_no][col];
        let double = quote == '"';
        let (mut l, mut i) = (line_no, col + 1);
        let mut out = String::new();
        let mut protected = 0usize;
        loop {
            let chars = &self.lines[l];
            if i >= chars.len() {
                while out.len() > protected && out.ends_with(is_ws) {
                    out.pop();
                }
                let mut next = l + 1;
                let mut blanks = 0;
                while next < self.lines.len() && self.lines[next].iter().all(|c| is_ws(*c)) {
                    next += 1;
                    blanks += 1;
                }
                if next >= self.lines.len() {
                    return Err(syntax(line_no, col, "unterminated quoted scalar"));
                }
                if blanks == 0 {
                    out.push(' ');
                } else {
                    out.push_str(&"\n".repeat(blanks));
                }
                protected = out.len();
                l = next;
                i = self.lines[l].iter().take_while(|c| is_ws(**c)).count();
                continue;
            }
            let c = chars[i];
            if c == quote {
                if !double && chars.get(i + 1) == Some(&'\'') {
                    out.push('\'');
                    i += 2;
                    continue;
                }
                return Ok((out, l, i + 1));
            }
            if double && c == '\\' {
                i += 1;
                let Some(&escape) = chars.get(i) else {
                    // A backslash at the end of a line joins it to the next one.
                    protected = out.len();
                    if l + 1 >= self.lines.len() {
                        return Err(syntax(line_no, col, "unterminated quoted scalar"));
                    }
                    l += 1;
                    i = self.lines[l].iter().take_while(|c| is_ws(**c)).count();
                    continue;
                };
                let simple = match escape {
                    '0' => Some('\0'),
                    'a' => Some('\u{7}'),
                    'b' => Some('\u{8}'),
                    't' | '\t' => Some('\t'),
                    'n' => Some('\n'),
                    'v' => Some('\u{b}'),
                    'f' => Some('\u{c}'),
                    'r' => Some('\r'),
                    'e' => Some('\u{1b}'),
                    ' ' => Some(' '),
                    '"' => Some('"'),
                    '/' => Some('/'),
                    '\\' => Some('\\'),
                    'N' => Some('\u{85}'),
                    '_' => Some('\u{a0}'),
                    'L' => Some('\u{2028}'),
                    'P' => Some('\u{2029}'),
                    _ => None,
                };
                if let Some(c) = simple {
                    out.push(c);
                    i += 1;
                } else if let Some(digits) = match escape {
                    'x' => Some(2),
                    'u' => Some(4),
                    'U' => Some(8),
                    _ => None,
                } {
                    let hex: String = chars.iter().skip(i + 1).take(digits).collect();
                    let decoded = (hex.chars().count() == digits)
                        .then(|| u32::from_str_radix(&hex, 16).ok())
                        .flatten()
                        .and_then(char::from_u32);
                    let Some(decoded) = decoded else {
                        return Err(syntax(
                            l,
                            i - 1,
                            "invalid escape: \\x, \\u and \\U need that many hex digits for one Unicode scalar value",
                        ));
                    };
                    out.push(decoded);
                    i += 1 + digits;
                } else {
                    return Err(syntax(
                        l,
                        i - 1,
                        format!("invalid escape sequence `\\{escape}`"),
                    ));
                }
                protected = out.len();
                continue;
            }
            out.push(c);
            i += 1;
        }
    }

    /// A literal `|` or folded `>` block scalar whose header is at `col` on the current line.
    fn block_scalar(&mut self, col: usize, parent: isize) -> Result<Json, YamlError> {
        let header_line = self.cur;
        let header = self.lines[header_line].clone();
        let folded = header[col] == '>';
        let mut chomp = 'c';
        let mut explicit: Option<usize> = None;
        let mut at = col + 1;
        while let Some(&c) = header.get(at) {
            match c {
                '+' | '-' if chomp == 'c' => chomp = if c == '+' { 'k' } else { 's' },
                '1'..='9' if explicit.is_none() => explicit = Some(c as usize - '0' as usize),
                _ => break,
            }
            at += 1;
        }
        let mut rest = at;
        while header.get(rest).is_some_and(|c| is_ws(*c)) {
            rest += 1;
        }
        if rest < header.len() && !(header[rest] == '#' && rest > at) {
            return Err(syntax(
                header_line,
                at,
                "invalid block scalar header; expected `|` or `>` with an optional chomping `+`/`-` and indentation digit",
            ));
        }
        let content_indent = explicit.map(|digit| parent.max(0) as usize + digit);
        let mut content_indent = content_indent;
        // Each entry is None for a blank line, or the text after the content indent.
        let mut entries: Vec<Option<String>> = Vec::new();
        let mut index = header_line + 1;
        while index < self.lines.len() {
            let line = &self.lines[index];
            let spaces = line.iter().take_while(|c| **c == ' ').count();
            if line.iter().all(|c| is_ws(*c)) {
                entries.push(None);
                index += 1;
                continue;
            }
            let ci = match content_indent {
                Some(ci) => ci,
                None => {
                    if (spaces as isize) <= parent {
                        break;
                    }
                    content_indent = Some(spaces);
                    spaces
                }
            };
            if spaces < ci {
                break;
            }
            if ci == 0 && self.doc_marker(index) {
                break;
            }
            entries.push(Some(line[ci..].iter().collect()));
            index += 1;
        }
        self.cur = index;
        let trailing = entries.iter().rev().take_while(|e| e.is_none()).count();
        let body = &entries[..entries.len() - trailing];
        let mut text = String::new();
        if folded {
            let mut previous_more: Option<bool> = None;
            let mut pending = 0usize;
            for entry in body {
                match entry {
                    None => pending += 1,
                    Some(line) => {
                        let more = line.starts_with(is_ws);
                        match previous_more {
                            Some(prev) if !more && !prev => {
                                if pending == 0 {
                                    text.push(' ');
                                } else {
                                    text.push_str(&"\n".repeat(pending));
                                }
                            }
                            Some(_) => text.push_str(&"\n".repeat(pending + 1)),
                            None => text.push_str(&"\n".repeat(pending)),
                        }
                        text.push_str(line);
                        previous_more = Some(more);
                        pending = 0;
                    }
                }
            }
        } else {
            let joined: Vec<&str> = body
                .iter()
                .map(|entry| entry.as_deref().unwrap_or(""))
                .collect();
            text = joined.join("\n");
        }
        let has_text = !body.is_empty();
        Ok(Json::String(match chomp {
            's' => text,
            'k' => {
                if has_text {
                    text.push('\n');
                }
                text.push_str(&"\n".repeat(trailing));
                text
            }
            _ => {
                if has_text {
                    text.push('\n');
                }
                text
            }
        }))
    }

    // ---- flow collections ----

    /// Skips whitespace, line breaks and comments inside a flow collection.
    fn flow_space(&self, l: &mut usize, c: &mut usize) {
        while *l < self.lines.len() {
            let line = &self.lines[*l];
            if *c >= line.len() {
                *l += 1;
                *c = 0;
                continue;
            }
            match line[*c] {
                ' ' | '\t' => *c += 1,
                '#' if *c == 0 || is_ws(line[*c - 1]) => *c = line.len(),
                _ => return,
            }
        }
    }

    fn flow(&self, l: &mut usize, c: &mut usize, depth: usize) -> Result<Json, YamlError> {
        if depth > MAX_DEPTH {
            return Err(too_deep(*l, *c));
        }
        self.flow_space(l, c);
        if *l >= self.lines.len() {
            return Err(syntax(
                self.lines.len() - 1,
                0,
                "unexpected end of input inside a flow collection",
            ));
        }
        let (start_line, start_col) = (*l, *c);
        let unterminated =
            |what: &str| syntax(start_line, start_col, format!("unterminated {what}"));
        let ch = self.lines[*l][*c];
        match ch {
            '[' => {
                *c += 1;
                let mut items = Vec::new();
                loop {
                    self.flow_space(l, c);
                    if *l >= self.lines.len() {
                        return Err(unterminated("flow sequence"));
                    }
                    if self.lines[*l][*c] == ']' {
                        *c += 1;
                        break;
                    }
                    items.push(self.flow(l, c, depth + 1)?);
                    self.flow_space(l, c);
                    if *l >= self.lines.len() {
                        return Err(unterminated("flow sequence"));
                    }
                    match self.lines[*l][*c] {
                        ',' => *c += 1,
                        ']' => {
                            *c += 1;
                            break;
                        }
                        ':' => {
                            return Err(unsupported(
                                *l,
                                *c,
                                "single-pair mapping inside a flow sequence",
                            ));
                        }
                        _ => {
                            return Err(syntax(*l, *c, "expected `,` or `]` in a flow sequence"));
                        }
                    }
                }
                Ok(Json::Array(items))
            }
            '{' => {
                *c += 1;
                let mut map = Map::new();
                loop {
                    self.flow_space(l, c);
                    if *l >= self.lines.len() {
                        return Err(unterminated("flow mapping"));
                    }
                    if self.lines[*l][*c] == '}' {
                        *c += 1;
                        break;
                    }
                    let (key_line, key_col) = (*l, *c);
                    let key = self.flow_key(l, c)?;
                    self.flow_space(l, c);
                    if *l >= self.lines.len() {
                        return Err(unterminated("flow mapping"));
                    }
                    let value = if self.lines[*l][*c] == ':' {
                        *c += 1;
                        self.flow_space(l, c);
                        if *l >= self.lines.len() {
                            return Err(unterminated("flow mapping"));
                        }
                        if matches!(self.lines[*l][*c], ',' | '}') {
                            Json::Null
                        } else {
                            self.flow(l, c, depth + 1)?
                        }
                    } else {
                        Json::Null
                    };
                    if key == "<<" {
                        return Err(unsupported(key_line, key_col, "merge key `<<`"));
                    }
                    if map.contains_key(&key) {
                        return Err(error(
                            "YAML_DUPLICATE_KEY",
                            key_line,
                            key_col,
                            format!("duplicate key `{key}` in one mapping"),
                        ));
                    }
                    map.insert(key, value);
                    self.flow_space(l, c);
                    if *l >= self.lines.len() {
                        return Err(unterminated("flow mapping"));
                    }
                    match self.lines[*l][*c] {
                        ',' => *c += 1,
                        '}' => {
                            *c += 1;
                            break;
                        }
                        _ => {
                            return Err(syntax(*l, *c, "expected `,` or `}` in a flow mapping"));
                        }
                    }
                }
                Ok(Json::Object(map))
            }
            ']' | '}' | ',' => Err(syntax(
                *l,
                *c,
                format!("unexpected `{ch}` in a flow collection"),
            )),
            '&' | '*' | '!' => Err(self.indicator(*l, *c)),
            '|' | '>' => Err(syntax(
                *l,
                *c,
                "block scalars cannot appear inside a flow collection",
            )),
            '"' | '\'' => {
                let (text, end_line, end_col) = self.quoted(*l, *c)?;
                *l = end_line;
                *c = end_col;
                Ok(Json::String(text))
            }
            _ => {
                let text = self.flow_plain(l, c)?;
                if text.is_empty() {
                    return Err(syntax(*l, *c, "expected a value"));
                }
                Ok(resolve_plain(&text))
            }
        }
    }

    fn flow_key(&self, l: &mut usize, c: &mut usize) -> Result<String, YamlError> {
        match self.lines[*l][*c] {
            '"' | '\'' => {
                let (text, end_line, end_col) = self.quoted(*l, *c)?;
                *l = end_line;
                *c = end_col;
                Ok(text)
            }
            '[' | '{' => Err(unsupported(*l, *c, "flow collection as a mapping key")),
            '&' | '*' | '!' => Err(self.indicator(*l, *c)),
            ',' | ']' | '}' => Err(syntax(*l, *c, "expected a key")),
            _ => self.flow_plain(l, c),
        }
    }

    /// A plain scalar inside a flow collection: it ends at `,`, `]`, `}`, a `:` followed by
    /// space or a flow indicator, a comment, or the end of the line.
    fn flow_plain(&self, l: &mut usize, c: &mut usize) -> Result<String, YamlError> {
        let line = &self.lines[*l];
        let begin = *c;
        let mut at = begin;
        while at < line.len() {
            let ch = line[at];
            if matches!(ch, ',' | ']' | '}') {
                break;
            }
            if ch == ':'
                && line
                    .get(at + 1)
                    .is_none_or(|n| is_ws(*n) || matches!(n, ',' | ']' | '}'))
            {
                break;
            }
            if ch == '#' && at > begin && is_ws(line[at - 1]) {
                break;
            }
            at += 1;
        }
        *c = at;
        let text: String = line[begin..at].iter().collect();
        Ok(text.trim_end_matches(is_ws).to_owned())
    }
}

/// Applies the plain-scalar policy.
fn resolve_plain(text: &str) -> Json {
    match text {
        "true" => Json::Bool(true),
        "false" => Json::Bool(false),
        "null" | "~" => Json::Null,
        _ => match plain_number(text) {
            Some(number) => Json::Number(number),
            None => Json::String(text.to_owned()),
        },
    }
}

/// An integer or decimal that reads back unchanged, so no digit is ever lost.
fn plain_number(text: &str) -> Option<Number> {
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    if unsigned.is_empty() {
        return None;
    }
    let (whole, fraction) = match unsigned.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (unsigned, None),
    };
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    if !digits(whole) || (whole.len() > 1 && whole.starts_with('0')) {
        return None;
    }
    let canonical = text.strip_prefix('+').unwrap_or(text);
    let number = match fraction {
        None => {
            if let Ok(value) = canonical.parse::<i64>() {
                Number::from(value)
            } else if let Ok(value) = canonical.parse::<u64>() {
                Number::from(value)
            } else {
                return None;
            }
        }
        Some(fraction) => {
            if !digits(fraction) {
                return None;
            }
            Number::from_f64(canonical.parse::<f64>().ok()?)?
        }
    };
    (number.to_string() == canonical).then_some(number)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ok(text: &str) -> Json {
        parse(text).unwrap_or_else(|e| panic!("{text:?}: {e:?}"))
    }

    #[test]
    fn blocks_and_scalars() {
        assert_eq!(
            ok("a: 1\nb: [x, 'y']\nc:\n  - d: e\n"),
            json!({"a": 1, "b": ["x", "y"], "c": [{"d": "e"}]})
        );
        assert_eq!(ok("t: 12:30\n"), json!({"t": "12:30"}));
        assert_eq!(
            ok("a: NO\nb: 1.10\nc: 0.5\n"),
            json!({"a": "NO", "b": "1.10", "c": 0.5})
        );
    }

    #[test]
    fn a_ten_thousand_line_mapping_is_fast() {
        let mut text = String::new();
        for index in 0..10_000 {
            text.push_str(&format!("key_{index}: value {index}\n"));
        }
        let started = std::time::Instant::now();
        let value = ok(&text);
        assert_eq!(value.as_object().unwrap().len(), 10_000);
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }

    /// Every prefix of every fixture, and a set of hostile inputs, must give a value or a typed
    /// error: never a panic, and never an error without a position.
    #[test]
    fn truncations_and_hostile_input_never_panic() {
        let base =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../cli/tests/fixtures/yaml");
        let mut inputs: Vec<String> = Vec::new();
        for name in ["workflow.yml", "compose.yml", "anchor.yml", "tag.yml"] {
            let text = std::fs::read_to_string(base.join(name)).unwrap();
            let chars: Vec<char> = text.chars().collect();
            for end in 0..=chars.len() {
                inputs.push(chars[..end].iter().collect());
            }
            // Drop a leading slice too, so a document can start mid-line or mid-indent.
            for start in 0..chars.len().min(200) {
                inputs.push(chars[start..].iter().collect());
            }
        }
        for text in [
            "\"",
            "'",
            "[",
            "{",
            "- ",
            "-",
            "? ",
            "a:",
            ": ",
            "|",
            ">",
            "a: |\n",
            "a: >2\n x",
            "\t",
            "a:\n\t",
            "&",
            "*",
            "!",
            "---",
            "...",
            "---\n---",
            "\u{feff}a: 1",
            "a: \"\\",
            "a: \"\\u12",
            "a: \"\\ud800\"",
            "[[[[[[",
            "{a: {b: {c:",
            "a: [1, 2",
            "- - - -",
            "-\n-\n  -",
            "a: 'b\n\n\n",
        ] {
            inputs.push(text.to_owned());
        }
        for text in inputs {
            match parse(&text) {
                Ok(_) => {}
                Err(found) => {
                    assert!(found.line >= 1 && found.column >= 1, "{text:?}: {found:?}");
                    assert!(
                        [
                            "YAML_SYNTAX",
                            "YAML_UNSUPPORTED",
                            "YAML_DUPLICATE_KEY",
                            "YAML_LIMIT"
                        ]
                        .contains(&found.code),
                        "{text:?}: {found:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn deep_nesting_is_a_limit_error() {
        let deep = format!("a: {}", "[".repeat(100_000));
        assert_eq!(parse(&deep).unwrap_err().code, "YAML_LIMIT");
        let sequence = "- ".repeat(100_000);
        assert_eq!(parse(&sequence).unwrap_err().code, "YAML_LIMIT");
    }
}
