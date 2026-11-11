//! Arithmetic, comparison, logic, math, reduction, ordering and linear-algebra kernels.

use guion_model::{ArrayFunction, ElementType, Value};

use crate::array::{ArrayError, Charge};

pub fn apply(
    function: ArrayFunction,
    element: ElementType,
    _args: &mut [Value],
    _charge: &mut Charge<'_>,
) -> Result<Value, ArrayError> {
    Err((
        "R_TYPE_INVALID",
        format!(
            "{} is not implemented for {} arrays",
            function.op_name(),
            element.name()
        ),
    ))
}
