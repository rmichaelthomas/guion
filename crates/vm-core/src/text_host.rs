//! `text_lower` and `text_upper`, shared by the native host and the browser VM's host.

use crate::{charge_allocation, Budget, HostOpContext, VmLimits};
use guion_model::{Op, RuntimeError, Value};

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
