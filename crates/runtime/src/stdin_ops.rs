//! The program's own standard input behind `stdin_read_line` and `stdin_read_all`.
//!
//! Both read through the process's one buffered standard input, so a line read and a later
//! read of the rest see the same stream and no byte is read twice or lost. Nothing is
//! allocated beyond the limit the caller passes: a longer line is skipped without being kept.
use std::io::{BufRead, ErrorKind, Read};

/// A typed failure; the runtime turns it into a `StdError` value.
pub(crate) struct StdinError {
    pub code: &'static str,
    pub message: String,
}

fn read_failure(error: std::io::Error) -> StdinError {
    StdinError {
        code: "E_STDIN_READ",
        message: format!("cannot read standard input: {error}"),
    }
}

/// The next line, without its `\n` or `\r\n`; `None` at end of input. A final line with no
/// terminator is returned. A line longer than `limit` bytes is skipped to its end and is
/// `E_STDIN_LINE_LIMIT`, and a line that is not UTF-8 is consumed and is `E_STDIN_UTF8`, so the
/// next read starts on the following line either way.
pub(crate) fn read_line(
    input: &mut impl BufRead,
    limit: usize,
) -> Result<Option<String>, StdinError> {
    // One extra byte for the `\r` of a `\r\n` terminator, which is not part of the line.
    let keep = limit.saturating_add(1);
    let mut line: Vec<u8> = Vec::new();
    let mut seen_any = false;
    let mut overflowed = false;
    loop {
        let buffer = match input.fill_buf() {
            Ok(buffer) => buffer,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(read_failure(error)),
        };
        if buffer.is_empty() {
            break;
        }
        seen_any = true;
        let (taken, ended) = match buffer.iter().position(|byte| *byte == b'\n') {
            Some(at) => (at, true),
            None => (buffer.len(), false),
        };
        if !overflowed {
            if line.len().saturating_add(taken) > keep {
                overflowed = true;
                line = Vec::new();
            } else {
                line.extend_from_slice(&buffer[..taken]);
            }
        }
        input.consume(taken + usize::from(ended));
        if ended {
            break;
        }
    }
    if !seen_any {
        return Ok(None);
    }
    if !overflowed && line.last() == Some(&b'\r') {
        line.pop();
    }
    if overflowed || line.len() > limit {
        return Err(StdinError {
            code: "E_STDIN_LINE_LIMIT",
            message: format!("a line of standard input is longer than {limit} bytes"),
        });
    }
    match String::from_utf8(line) {
        Ok(text) => Ok(Some(text)),
        Err(_) => Err(StdinError {
            code: "E_STDIN_UTF8",
            message: "a line of standard input is not valid UTF-8".into(),
        }),
    }
}

/// Everything left on the stream. More than `max` bytes is `E_STDIN_LIMIT`; at most `max + 1`
/// bytes are held to find that out.
pub(crate) fn read_all(input: &mut impl Read, max: usize) -> Result<Vec<u8>, StdinError> {
    let mut bytes = Vec::new();
    let cap = (max as u64).saturating_add(1);
    input
        .take(cap)
        .read_to_end(&mut bytes)
        .map_err(read_failure)?;
    if bytes.len() > max {
        return Err(StdinError {
            code: "E_STDIN_LIMIT",
            message: format!("standard input is larger than {max} bytes"),
        });
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn line(bytes: &[u8], limit: usize) -> Result<Option<String>, &'static str> {
        read_line(&mut Cursor::new(bytes.to_vec()), limit).map_err(|error| error.code)
    }

    #[test]
    fn lines_end_on_lf_or_crlf_and_the_last_may_be_bare() {
        let mut input = Cursor::new(b"a\r\nb\nc".to_vec());
        assert_eq!(
            read_line(&mut input, 8).ok().flatten().as_deref(),
            Some("a")
        );
        assert_eq!(
            read_line(&mut input, 8).ok().flatten().as_deref(),
            Some("b")
        );
        assert_eq!(
            read_line(&mut input, 8).ok().flatten().as_deref(),
            Some("c")
        );
        assert!(matches!(read_line(&mut input, 8), Ok(None)));
    }

    #[test]
    fn empty_input_is_end_and_an_empty_line_is_empty_text() {
        assert_eq!(line(b"", 8), Ok(None));
        assert_eq!(line(b"\n", 8), Ok(Some(String::new())));
    }

    #[test]
    fn the_limit_counts_the_line_not_its_terminator() {
        assert_eq!(line(b"12345678\n", 8), Ok(Some("12345678".into())));
        assert_eq!(line(b"12345678\r\n", 8), Ok(Some("12345678".into())));
        assert_eq!(line(b"123456789\n", 8), Err("E_STDIN_LINE_LIMIT"));
        assert_eq!(line(b"123456789\r\n", 8), Err("E_STDIN_LINE_LIMIT"));
        assert_eq!(line(b"12345678", 8), Ok(Some("12345678".into())));
    }

    #[test]
    fn an_error_consumes_the_offending_line() {
        let mut input = Cursor::new(b"123456789\nok\n\xff\nnext\n".to_vec());
        assert!(matches!(read_line(&mut input, 8), Err(e) if e.code == "E_STDIN_LINE_LIMIT"));
        assert_eq!(
            read_line(&mut input, 8).ok().flatten().as_deref(),
            Some("ok")
        );
        assert!(matches!(read_line(&mut input, 8), Err(e) if e.code == "E_STDIN_UTF8"));
        assert_eq!(
            read_line(&mut input, 8).ok().flatten().as_deref(),
            Some("next")
        );
    }

    #[test]
    fn read_all_is_bounded() {
        assert_eq!(
            read_all(&mut Cursor::new(vec![0, 255, 10]), 3).ok(),
            Some(vec![0, 255, 10])
        );
        assert!(matches!(
            read_all(&mut Cursor::new(vec![0; 4]), 3),
            Err(e) if e.code == "E_STDIN_LIMIT"
        ));
    }
}
