//! `text_lower`, `text_upper`, `int_to_text_radix` and `text_parse_int_radix`, shared by the native
//! host and the browser VM's host.

use crate::{charge_allocation, Budget, HostOpContext, VmLimits};
use guion_model::{BigInt, FileErrorValue, Op, RuntimeError, Value};

/// Runs `TextLower` or `TextUpper` on the frame in `context`. These are the Unicode default case
/// mappings (`Straße` uppercases to `STRASSE`; a capital sigma ending a word lowercases to the
/// final form). `Ok(false)` means the instruction is not one of the two and nothing was touched.
pub fn run_text_case_instruction(
    context: &mut HostOpContext<'_>,
    limits: &VmLimits,
) -> Result<bool, RuntimeError> {
    let upper = match context.instruction.op {
        Op::TextLower => false,
        Op::TextUpper => true,
        _ => return Ok(false),
    };
    let span = context.instruction.span;
    let budget: &Budget = context.budget.as_ref();
    let Some(Value::Text(text)) = context.stack.pop() else {
        return Err(RuntimeError::new(
            "R_TYPE_INVALID",
            "text_lower and text_upper require Text",
            span,
        ));
    };
    let output = if upper {
        text.to_uppercase()
    } else {
        text.to_lowercase()
    };
    if output.len() > limits.max_text_bytes {
        return Err(RuntimeError::new(
            "R_TEXT_LIMIT",
            "case-mapped text exceeds the configured text limit",
            span,
        ));
    }
    charge_allocation(budget, output.len(), span)?;
    context.stack.push(Value::Text(output.into()));
    Ok(true)
}

fn radix_of(value: Value, span: guion_model::Span) -> Result<u32, RuntimeError> {
    let Value::Int(radix) = value else {
        return Err(RuntimeError::new(
            "R_TYPE_INVALID",
            "the radix must be an Int",
            span,
        ));
    };
    match radix.to_i64() {
        Some(radix @ 2..=36) => Ok(radix as u32),
        _ => Err(RuntimeError::new(
            "R_RADIX_RANGE",
            "the radix must be from 2 to 36",
            span,
        )),
    }
}

fn parse_failure(message: &str, span: guion_model::Span) -> Value {
    Value::ResultErr(Box::new(Value::FileError(Box::new(FileErrorValue {
        code: "E_INT_PARSE".to_owned(),
        message: message.to_owned(),
        trace: vec![span],
    }))))
}

/// Runs `IntToTextRadix` or `TextParseIntRadix` on the frame in `context`. `Ok(false)` means the
/// instruction is not one of the two and nothing was touched. A radix outside 2 to 36 is the
/// runtime fault `R_RADIX_RANGE`; text that is not an integer in the radix is a `FileError` with
/// code `E_INT_PARSE`, as `text_parse_int` gives.
pub fn run_radix_instruction(
    context: &mut HostOpContext<'_>,
    limits: &VmLimits,
) -> Result<bool, RuntimeError> {
    let writing = match context.instruction.op {
        Op::IntToTextRadix => true,
        Op::TextParseIntRadix => false,
        _ => return Ok(false),
    };
    let span = context.instruction.span;
    let budget: &Budget = context.budget.as_ref();
    let (Some(radix), Some(first)) = (context.stack.pop(), context.stack.pop()) else {
        return Err(RuntimeError::new(
            "R_STACK_UNDERFLOW",
            "operand stack is empty",
            span,
        ));
    };
    let radix = radix_of(radix, span)?;
    if writing {
        let Value::Int(value) = first else {
            return Err(RuntimeError::new(
                "R_TYPE_INVALID",
                "int_to_text_radix requires an Int",
                span,
            ));
        };
        let output = value.to_radix_string(radix);
        if output.len() > limits.max_text_bytes {
            return Err(RuntimeError::new(
                "R_TEXT_LIMIT",
                "the integer's digits exceed the configured text limit",
                span,
            ));
        }
        charge_allocation(budget, output.len(), span)?;
        context.stack.push(Value::Text(output.into()));
        return Ok(true);
    }
    let Value::Text(text) = first else {
        return Err(RuntimeError::new(
            "R_TYPE_INVALID",
            "text_parse_int_radix requires Text",
            span,
        ));
    };
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, &*text),
    };
    // A run this long has more decimal digits than the limit, whatever its value: refuse it before
    // doing the work.
    let certain_digits =
        (digits.len().saturating_sub(1) as f64 * f64::from(radix).log10()) as usize;
    if certain_digits > limits.max_integer_digits {
        return Err(RuntimeError::new(
            "R_INTEGER_LIMIT",
            "parsed integer exceeds the configured digit limit",
            span,
        ));
    }
    let result = match BigInt::parse_radix(digits, radix, negative) {
        Ok(value) => value,
        Err(_) => {
            context.stack.push(parse_failure(
                "expected an integer written with the digits of the radix and an optional leading `-`",
                span,
            ));
            return Ok(true);
        }
    };
    if result.digit_count() > limits.max_integer_digits {
        return Err(RuntimeError::new(
            "R_INTEGER_LIMIT",
            "parsed integer exceeds the configured digit limit",
            span,
        ));
    }
    context
        .stack
        .push(Value::ResultOk(Box::new(Value::Int(result))));
    Ok(true)
}
