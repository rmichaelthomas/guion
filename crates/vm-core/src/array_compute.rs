//! Arithmetic, comparison, logic, math, reduction, ordering, linear-algebra and generator
//! kernels for `Op::Array`.

use guion_model::{
    shape_size, ArrayFunction, ArrayValue, BigInt, ElementType, MathFunction, Shared, Value,
};

use crate::array::{float_value, with_element, ArrayError, Charge, Element, Release};

fn error<T>(code: &'static str, message: impl Into<String>) -> Result<T, ArrayError> {
    Err((code, message.into()))
}

fn array_bytes(element: ElementType, rank: usize, size: usize) -> usize {
    std::mem::size_of::<ArrayValue>()
        .saturating_add(rank.saturating_mul(std::mem::size_of::<usize>()))
        .saturating_add(size.saturating_mul(element.width()))
}

fn finish<T: Element>(shape: Vec<usize>, data: Vec<T>) -> Result<Value, ArrayError> {
    Ok(Value::Array(Shared::new(ArrayValue::new(
        shape,
        T::wrap(data),
    )?)))
}

fn array_arg(args: &[Value], index: usize) -> Result<&ArrayValue, ArrayError> {
    match &args[index] {
        Value::Array(array) => Ok(array),
        other => error(
            "R_TYPE_INVALID",
            format!("expected an array, found {other}"),
        ),
    }
}

fn int_arg(args: &[Value], index: usize) -> Result<i128, ArrayError> {
    match &args[index] {
        Value::Int(value) => Ok(value
            .to_i64()
            .map(i128::from)
            .unwrap_or(if value.is_negative() {
                i128::MIN
            } else {
                i128::MAX
            })),
        other => error("R_TYPE_INVALID", format!("expected Int, found {other}")),
    }
}

fn float_arg(args: &[Value], index: usize) -> Result<f64, ArrayError> {
    match &args[index] {
        Value::Float64(value) => Ok(value.get()),
        other => error("R_TYPE_INVALID", format!("expected Float64, found {other}")),
    }
}

/// Arithmetic on one numeric element type. Integer results that leave the type's range and
/// float results that are not finite are errors.
trait Numeric: Element {
    fn add(self, other: Self) -> Result<Self, &'static str>;
    fn sub(self, other: Self) -> Result<Self, &'static str>;
    fn mul(self, other: Self) -> Result<Self, &'static str>;
    /// Integer floor division or float division.
    fn div(self, other: Self) -> Result<Self, &'static str>;
    fn neg(self) -> Result<Self, &'static str>;
    fn abs(self) -> Result<Self, &'static str>;
    fn zero() -> Self {
        Self::default()
    }
    /// `add`, `sub` and `mul` without the per-element finiteness check; the kernels then call
    /// `first_nonfinite` once on the finished result. Integers check overflow as usual.
    #[inline]
    fn add_raw(self, other: Self) -> Result<Self, &'static str> {
        self.add(other)
    }
    #[inline]
    fn sub_raw(self, other: Self) -> Result<Self, &'static str> {
        self.sub(other)
    }
    #[inline]
    fn mul_raw(self, other: Self) -> Result<Self, &'static str> {
        self.mul(other)
    }
    /// Whether a result of the `_raw` operations is acceptable (always true for integers).
    #[inline]
    fn acceptable(self) -> bool {
        true
    }
    /// The first element of a result produced by the `_raw` operations that is not finite.
    fn first_nonfinite(_items: &[Self]) -> Option<usize> {
        None
    }
}

const OVERFLOW: &str = "R_ARRAY_OVERFLOW";
const DIV_ZERO: &str = "R_DIV_ZERO";
const NONFINITE: &str = "R_FLOAT_NONFINITE";

macro_rules! integer_numeric {
    ($($ty:ty),*) => {$(
        impl Numeric for $ty {
            fn add(self, other: Self) -> Result<Self, &'static str> {
                self.checked_add(other).ok_or(OVERFLOW)
            }
            fn sub(self, other: Self) -> Result<Self, &'static str> {
                self.checked_sub(other).ok_or(OVERFLOW)
            }
            fn mul(self, other: Self) -> Result<Self, &'static str> {
                self.checked_mul(other).ok_or(OVERFLOW)
            }
            fn div(self, other: Self) -> Result<Self, &'static str> {
                if other == 0 {
                    return Err(DIV_ZERO);
                }
                let quotient = self.checked_div(other).ok_or(OVERFLOW)?;
                // Round toward negative infinity, as `//` does.
                #[allow(unused_comparisons)]
                let adjust = (self % other != 0) && ((self < 0) != (other < 0));
                if adjust {
                    quotient.checked_sub(1).ok_or(OVERFLOW)
                } else {
                    Ok(quotient)
                }
            }
            fn neg(self) -> Result<Self, &'static str> {
                (0 as $ty).checked_sub(self).ok_or(OVERFLOW)
            }
            fn abs(self) -> Result<Self, &'static str> {
                #[allow(unused_comparisons)]
                if self < 0 { Numeric::neg(self) } else { Ok(self) }
            }
        }
    )*};
}
integer_numeric!(i8, i16, i32, i64, u8, u16, u32, u64);

macro_rules! float_numeric {
    ($($ty:ty),*) => {$(
        impl Numeric for $ty {
            fn add(self, other: Self) -> Result<Self, &'static str> {
                finite(self + other)
            }
            fn sub(self, other: Self) -> Result<Self, &'static str> {
                finite(self - other)
            }
            fn mul(self, other: Self) -> Result<Self, &'static str> {
                finite(self * other)
            }
            fn div(self, other: Self) -> Result<Self, &'static str> {
                if other == 0.0 {
                    return Err(DIV_ZERO);
                }
                finite(self / other)
            }
            fn neg(self) -> Result<Self, &'static str> {
                Ok(-self)
            }
            fn abs(self) -> Result<Self, &'static str> {
                Ok(self.abs())
            }
            #[inline]
            fn add_raw(self, other: Self) -> Result<Self, &'static str> {
                Ok(self + other)
            }
            #[inline]
            fn sub_raw(self, other: Self) -> Result<Self, &'static str> {
                Ok(self - other)
            }
            #[inline]
            fn mul_raw(self, other: Self) -> Result<Self, &'static str> {
                Ok(self * other)
            }
            #[inline]
            fn acceptable(self) -> bool {
                self.is_finite()
            }
            fn first_nonfinite(items: &[Self]) -> Option<usize> {
                if items.iter().all(|item| item.is_finite()) {
                    None
                } else {
                    items.iter().position(|item| !item.is_finite())
                }
            }
        }
    )*};
}
float_numeric!(f32, f64);

#[inline]
fn finite<T: Into<f64> + Copy>(value: T) -> Result<T, &'static str> {
    if value.into().is_finite() {
        Ok(value)
    } else {
        Err(NONFINITE)
    }
}

fn element_error(code: &'static str, what: &str, element: ElementType, index: usize) -> ArrayError {
    let message = match code {
        OVERFLOW => format!("{what} overflows {} at element {index}", element.name()),
        DIV_ZERO => format!("{what} divides by zero at element {index}"),
        NONFINITE => format!("{what} is not finite at element {index}"),
        _ => format!("{what} failed at element {index}"),
    };
    (code, message)
}

/// The broadcast shape of two shapes (NumPy rules), or `R_ARRAY_SHAPE`.
fn broadcast_shape(left: &[usize], right: &[usize]) -> Result<Vec<usize>, ArrayError> {
    let rank = left.len().max(right.len());
    let mut shape = vec![0; rank];
    for (axis, slot) in shape.iter_mut().enumerate() {
        let l = axis
            .checked_sub(rank - left.len())
            .map_or(1, |index| left[index]);
        let r = axis
            .checked_sub(rank - right.len())
            .map_or(1, |index| right[index]);
        *slot = if l == r || r == 1 {
            l
        } else if l == 1 {
            r
        } else {
            return error(
                "R_ARRAY_SHAPE",
                format!("shapes {left:?} and {right:?} do not broadcast"),
            );
        };
    }
    Ok(shape)
}

/// Strides of `shape` laid out inside `out` (0 on broadcast axes).
fn broadcast_strides(shape: &[usize], out: &[usize]) -> Vec<usize> {
    let mut strides = vec![0; out.len()];
    let offset = out.len() - shape.len();
    let mut stride = 1;
    for axis in (0..shape.len()).rev() {
        strides[offset + axis] = if shape[axis] == 1 { 0 } else { stride };
        stride *= shape[axis];
    }
    strides
}

/// Applies `f` elementwise with broadcasting.
fn zip_with<A: Element, B: Element, R: Element>(
    left: &ArrayValue,
    right: &ArrayValue,
    charge: &mut Charge<'_>,
    what: &str,
    f: impl Fn(A, B) -> Result<R, &'static str>,
) -> Result<Value, ArrayError> {
    let a = A::items(&left.data);
    let b = B::items(&right.data);
    let shape = if left.shape == right.shape {
        left.shape.clone()
    } else {
        broadcast_shape(&left.shape, &right.shape)?
    };
    let size = shape_size(&shape).ok_or(("R_ARRAY_SHAPE", "shape is too large".to_string()))?;
    charge(array_bytes(R::TYPE, shape.len(), size))?;
    let mut out = Vec::with_capacity(size);
    let fail = |code, index| element_error(code, what, R::TYPE, index);
    if left.shape == right.shape {
        for (index, (x, y)) in a.iter().zip(b).enumerate() {
            out.push(f(*x, *y).map_err(|code| fail(code, index))?);
        }
    } else if b.len() == 1 && left.shape == shape {
        let y = b[0];
        for (index, x) in a.iter().enumerate() {
            out.push(f(*x, y).map_err(|code| fail(code, index))?);
        }
    } else if a.len() == 1 && right.shape == shape {
        let x = a[0];
        for (index, y) in b.iter().enumerate() {
            out.push(f(x, *y).map_err(|code| fail(code, index))?);
        }
    } else {
        let left_strides = broadcast_strides(&left.shape, &shape);
        let right_strides = broadcast_strides(&right.shape, &shape);
        let rank = shape.len();
        let mut counter = vec![0usize; rank];
        let (mut li, mut ri) = (0usize, 0usize);
        for index in 0..size {
            out.push(f(a[li], b[ri]).map_err(|code| fail(code, index))?);
            for axis in (0..rank).rev() {
                counter[axis] += 1;
                li += left_strides[axis];
                ri += right_strides[axis];
                if counter[axis] < shape[axis] {
                    break;
                }
                li -= left_strides[axis] * shape[axis];
                ri -= right_strides[axis] * shape[axis];
                counter[axis] = 0;
            }
        }
    }
    finish(shape, out)
}

fn map<A: Element, R: Element>(
    array: &ArrayValue,
    charge: &mut Charge<'_>,
    what: &str,
    f: impl Fn(A) -> Result<R, ArrayError>,
) -> Result<Value, ArrayError> {
    let items = A::items(&array.data);
    charge(array_bytes(R::TYPE, array.shape.len(), items.len()))?;
    let mut out = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        out.push(f(*item).map_err(|(code, message)| {
            if message.is_empty() {
                element_error(code, what, R::TYPE, index)
            } else {
                (code, message)
            }
        })?);
    }
    finish(array.shape.clone(), out)
}

/// Whether `value` is an array of `shape` that nothing else holds, so a kernel may overwrite
/// its buffer with the result.
fn reusable(value: &Value, shape: &[usize]) -> bool {
    matches!(value, Value::Array(array) if Shared::strong_count(array) == 1 && array.shape == shape)
}

/// `map` that overwrites the operand's buffer when nothing else holds it.
fn map_owned<T: Element>(
    args: &mut [Value],
    charge: &mut Charge<'_>,
    what: &str,
    f: impl Fn(T) -> Result<T, ArrayError>,
) -> Result<Value, ArrayError> {
    let shape = array_arg(args, 0)?.shape.clone();
    if !reusable(&args[0], &shape) {
        return map(array_arg(args, 0)?, charge, what, f);
    }
    let Value::Array(mut owned) = std::mem::replace(&mut args[0], Value::Unit) else {
        unreachable!("checked by reusable")
    };
    let target = Shared::get_mut(&mut owned).expect("a unique array");
    for (index, slot) in T::items_mut(&mut target.data).iter_mut().enumerate() {
        *slot = f(*slot).map_err(|(code, message)| {
            if message.is_empty() {
                element_error(code, what, T::TYPE, index)
            } else {
                (code, message)
            }
        })?;
    }
    Ok(Value::Array(owned))
}

/// Same-type arithmetic with broadcasting. When an operand is an unshared array already of the
/// result's shape, its buffer receives the result; float results are checked for finiteness
/// once, after the loop.
fn zip_numeric<T: Numeric>(
    args: &mut [Value],
    scalar: Option<ArrayValue>,
    charge: &mut Charge<'_>,
    what: &str,
    f: impl Fn(T, T) -> Result<T, &'static str>,
) -> Result<Value, ArrayError> {
    let (reuse, shape) = {
        let left = array_arg(args, 0)?;
        let right = match &scalar {
            Some(scalar) => scalar,
            None => array_arg(args, 1)?,
        };
        let shape = if left.shape == right.shape {
            left.shape.clone()
        } else {
            broadcast_shape(&left.shape, &right.shape)?
        };
        let fits = |other: &ArrayValue| other.shape == shape || other.size() == 1;
        let reuse = if reusable(&args[0], &shape) && fits(right) {
            Some(0)
        } else if scalar.is_none() && reusable(&args[1], &shape) && fits(left) {
            Some(1)
        } else {
            None
        };
        (reuse, shape)
    };
    let Some(which) = reuse else {
        let left = array_arg(args, 0)?;
        let right = match &scalar {
            Some(scalar) => scalar,
            None => array_arg(args, 1)?,
        };
        if left.shape == right.shape {
            charge(array_bytes(T::TYPE, shape.len(), left.size()))?;
            let mut out: Vec<T> = Vec::with_capacity(left.size());
            let mut unacceptable = false;
            let fail = |code, index| element_error(code, what, T::TYPE, index);
            for (index, (x, y)) in T::items(&left.data)
                .iter()
                .zip(T::items(&right.data))
                .enumerate()
            {
                let value = f(*x, *y).map_err(|code| fail(code, index))?;
                unacceptable |= !value.acceptable();
                out.push(value);
            }
            if unacceptable {
                if let Some(index) = T::first_nonfinite(&out) {
                    return Err(fail(NONFINITE, index));
                }
            }
            return finish(shape, out);
        }
        let result = zip_with(left, right, charge, what, &f)?;
        if let Value::Array(array) = &result {
            if let Some(index) = T::first_nonfinite(T::items(&array.data)) {
                return Err(element_error(NONFINITE, what, T::TYPE, index));
            }
        }
        return Ok(result);
    };
    let Value::Array(mut owned) = std::mem::replace(&mut args[which], Value::Unit) else {
        unreachable!("checked by reusable")
    };
    let other = match (&scalar, which) {
        (Some(scalar), _) => scalar,
        (None, 0) => array_arg(args, 1)?,
        (None, _) => array_arg(args, 0)?,
    };
    let other = T::items(&other.data);
    let target = Shared::get_mut(&mut owned).expect("a unique array");
    let items = T::items_mut(&mut target.data);
    debug_assert_eq!(shape_size(&shape), Some(items.len()));
    let fail = |code, index| element_error(code, what, T::TYPE, index);
    let apply = |x: T, y: T| if which == 0 { f(x, y) } else { f(y, x) };
    // Finiteness is accumulated without branching so the loop stays tight; only a result
    // that failed it is searched for the offending element.
    let mut unacceptable = false;
    if other.len() == items.len() {
        for (index, (slot, y)) in items.iter_mut().zip(other).enumerate() {
            let value = apply(*slot, *y).map_err(|code| fail(code, index))?;
            unacceptable |= !value.acceptable();
            *slot = value;
        }
    } else {
        let y = other[0];
        for (index, slot) in items.iter_mut().enumerate() {
            let value = apply(*slot, y).map_err(|code| fail(code, index))?;
            unacceptable |= !value.acceptable();
            *slot = value;
        }
    }
    if unacceptable {
        if let Some(index) = T::first_nonfinite(items) {
            return Err(fail(NONFINITE, index));
        }
    }
    Ok(Value::Array(owned))
}

fn scalar_array<T: Element>(value: &Value) -> Result<ArrayValue, ArrayError> {
    ArrayValue::new(vec![], T::wrap(vec![T::from_value(value)?]))
}

fn arithmetic<T: Numeric>(
    function: ArrayFunction,
    args: &mut [Value],
    charge: &mut Charge<'_>,
) -> Result<Value, ArrayError> {
    use ArrayFunction::*;
    let scalar = match function {
        AddScalar | SubScalar | MulScalar | DivScalar => Some(scalar_array::<T>(&args[1])?),
        _ => None,
    };
    let name = &function.guest_name();
    match function {
        Add | AddScalar => zip_numeric(args, scalar, charge, name, T::add_raw),
        Sub | SubScalar => zip_numeric(args, scalar, charge, name, T::sub_raw),
        Mul | MulScalar => zip_numeric(args, scalar, charge, name, T::mul_raw),
        Div | DivScalar => zip_numeric(args, scalar, charge, name, T::div),
        Minimum => zip_numeric(args, scalar, charge, name, |x: T, y: T| {
            Ok(if y < x { y } else { x })
        }),
        Maximum => zip_numeric(args, scalar, charge, name, |x: T, y: T| {
            Ok(if y > x { y } else { x })
        }),
        _ => unreachable!("arithmetic dispatch"),
    }
}

fn compare<T: Element>(
    function: ArrayFunction,
    args: &[Value],
    charge: &mut Charge<'_>,
) -> Result<Value, ArrayError> {
    use ArrayFunction::*;
    let left = array_arg(args, 0)?;
    let scalar;
    let right = match function {
        EqScalar | NeScalar | LtScalar | LeScalar | GtScalar | GeScalar => {
            scalar = scalar_array::<T>(&args[1])?;
            &scalar
        }
        _ => array_arg(args, 1)?,
    };
    let name = &function.guest_name();
    let test: fn(&T, &T) -> bool = match function {
        Eq | EqScalar => |x, y| x == y,
        Ne | NeScalar => |x, y| x != y,
        Lt | LtScalar => |x, y| x < y,
        Le | LeScalar => |x, y| x <= y,
        Gt | GtScalar => |x, y| x > y,
        Ge | GeScalar => |x, y| x >= y,
        _ => unreachable!("comparison dispatch"),
    };
    zip_with(left, right, charge, name, |x: T, y: T| Ok(test(&x, &y)))
}

fn math_function(function: ArrayFunction) -> Option<MathFunction> {
    use ArrayFunction::*;
    Some(match function {
        Exp => MathFunction::Exp,
        Log => MathFunction::Ln,
        Log10 => MathFunction::Log10,
        Log2 => MathFunction::Log2,
        Sin => MathFunction::Sin,
        Cos => MathFunction::Cos,
        Tan => MathFunction::Tan,
        Asin => MathFunction::Asin,
        Acos => MathFunction::Acos,
        Atan => MathFunction::Atan,
        Sinh => MathFunction::Sinh,
        Cosh => MathFunction::Cosh,
        Tanh => MathFunction::Tanh,
        Floor => MathFunction::Floor,
        Ceil => MathFunction::Ceil,
        Round => MathFunction::Round,
        Pow => MathFunction::Pow,
        _ => return None,
    })
}

fn float_math<T: Element>(
    function: ArrayFunction,
    args: &mut [Value],
    charge: &mut Charge<'_>,
) -> Result<Value, ArrayError> {
    array_arg(args, 0)?;
    let exponent = if function == ArrayFunction::Pow {
        float_arg(args, 1)?
    } else {
        0.0
    };
    let math = math_function(function);
    let name = &function.guest_name();
    map_owned(args, charge, name, |x: T| {
        let x = x.to_f64();
        let result = match math {
            Some(math) => crate::apply_math(math, x, exponent)
                .map(|value| value.get())
                .map_err(|(code, message)| (code, format!("{name}: {message}")))?,
            None if x < 0.0 => {
                return error("R_MATH_DOMAIN", format!("{name} of a negative value"));
            }
            None => x.sqrt(),
        };
        T::from_float(result).ok_or((NONFINITE, String::new()))
    })
}

fn logic(
    function: ArrayFunction,
    args: &[Value],
    charge: &mut Charge<'_>,
) -> Result<Value, ArrayError> {
    use ArrayFunction::*;
    let left = array_arg(args, 0)?;
    let items = bool::items(&left.data);
    match function {
        And => zip_with(
            left,
            array_arg(args, 1)?,
            charge,
            "array_and",
            |x: bool, y: bool| Ok(x && y),
        ),
        Or => zip_with(
            left,
            array_arg(args, 1)?,
            charge,
            "array_or",
            |x: bool, y: bool| Ok(x || y),
        ),
        Not => map(left, charge, "array_not", |x: bool| Ok(!x)),
        Any => Ok(Value::Bool(items.iter().any(|item| *item))),
        All => Ok(Value::Bool(items.iter().all(|item| *item))),
        Count => Ok(Value::Int(BigInt::from_usize(
            items.iter().filter(|item| **item).count(),
        ))),
        _ => unreachable!("logic dispatch"),
    }
}

fn select<T: Element>(
    function: ArrayFunction,
    args: &[Value],
    charge: &mut Charge<'_>,
) -> Result<Value, ArrayError> {
    if function == ArrayFunction::Where {
        let mask = array_arg(args, 0)?;
        let a = array_arg(args, 1)?;
        let b = array_arg(args, 2)?;
        if mask.shape != a.shape || a.shape != b.shape {
            return error(
                "R_ARRAY_SHAPE",
                format!(
                    "array_where needs equal shapes, got {:?}, {:?} and {:?}",
                    mask.shape, a.shape, b.shape
                ),
            );
        }
        let flags = bool::items(&mask.data);
        let (a_items, b_items) = (T::items(&a.data), T::items(&b.data));
        charge(array_bytes(T::TYPE, a.shape.len(), flags.len()))?;
        let out = flags
            .iter()
            .zip(a_items.iter().zip(b_items))
            .map(|(flag, (x, y))| if *flag { *x } else { *y })
            .collect();
        return finish(a.shape.clone(), out);
    }
    let array = array_arg(args, 0)?;
    let mask = array_arg(args, 1)?;
    if mask.shape != array.shape {
        return error(
            "R_ARRAY_SHAPE",
            format!(
                "array_filter needs a mask of shape {:?}, got {:?}",
                array.shape, mask.shape
            ),
        );
    }
    let flags = bool::items(&mask.data);
    let count = flags.iter().filter(|flag| **flag).count();
    charge(array_bytes(T::TYPE, 1, count))?;
    let items = T::items(&array.data);
    let Some(first) = items.first() else {
        return finish(vec![0], Vec::<T>::new());
    };
    // Branch-free compaction: every item is written to the next free slot and the slot only
    // advances for a kept item, so a mask with no pattern costs no branch mispredictions. One
    // spare slot takes the write of a dropped item at the end.
    let mut out = vec![*first; count + 1];
    let mut next = 0;
    for (item, flag) in items.iter().zip(flags) {
        out[next] = *item;
        next += usize::from(*flag);
    }
    out.truncate(count);
    finish(vec![count], out)
}

/// An exact integer accumulator that widens to `BigInt` only when 128 bits overflow.
#[derive(Default)]
struct ExactSum {
    small: i128,
    big: Option<BigInt>,
}

fn big_of(value: i128) -> BigInt {
    match i64::try_from(value) {
        Ok(small) => BigInt::from_i64(small),
        Err(_) => BigInt::parse(&value.to_string()).expect("decimal digits parse"),
    }
}

impl ExactSum {
    fn add(&mut self, value: i128) {
        match self.small.checked_add(value) {
            Some(sum) => self.small = sum,
            None => {
                let big = self.big.take().unwrap_or_else(BigInt::zero);
                self.big = Some(&(&big + &big_of(self.small)) + &big_of(value));
                self.small = 0;
            }
        }
    }
    fn value(self) -> BigInt {
        match self.big {
            Some(big) => &big + &big_of(self.small),
            None => big_of(self.small),
        }
    }
}

/// Pairwise summation in f64, for accuracy on long arrays.
fn pairwise_sum<T: Element>(items: &[T]) -> f64 {
    if items.len() <= 128 {
        return items.iter().fold(0.0, |total, item| total + item.to_f64());
    }
    let middle = items.len() / 2;
    pairwise_sum(&items[..middle]) + pairwise_sum(&items[middle..])
}

fn empty_error(name: &str) -> ArrayError {
    ("R_ARRAY_EMPTY", format!("{name} of an empty array"))
}

fn reduce<T: Element>(function: ArrayFunction, args: &[Value]) -> Result<Value, ArrayError> {
    use ArrayFunction::*;
    let array = array_arg(args, 0)?;
    let items = T::items(&array.data);
    let name = &function.guest_name();
    match function {
        Sum if T::TYPE.is_integer() => {
            let mut sum = ExactSum::default();
            for item in items {
                sum.add(item.to_int().expect("integer element"));
            }
            Ok(Value::Int(sum.value()))
        }
        Sum => float_value(pairwise_sum(items)),
        Prod if T::TYPE.is_integer() => {
            let mut small: Option<i128> = Some(1);
            let mut big = BigInt::one();
            for item in items {
                let value = item.to_int().expect("integer element");
                small = match small {
                    Some(product) => match product.checked_mul(value) {
                        Some(product) => Some(product),
                        None => {
                            big = &big_of(product) * &big_of(value);
                            None
                        }
                    },
                    None => {
                        big = &big * &big_of(value);
                        None
                    }
                };
            }
            Ok(Value::Int(small.map(big_of).unwrap_or(big)))
        }
        Prod => float_value(items.iter().map(|item| item.to_f64()).product()),
        Min | Max | Argmin | Argmax => {
            let Some(first) = items.first() else {
                return Err(empty_error(name));
            };
            let (mut best, mut best_index) = (*first, 0usize);
            let lower = matches!(function, Min | Argmin);
            for (index, item) in items.iter().enumerate().skip(1) {
                if (lower && *item < best) || (!lower && *item > best) {
                    best = *item;
                    best_index = index;
                }
            }
            Ok(match function {
                Min | Max => best.to_value(),
                _ => Value::Int(BigInt::from_usize(best_index)),
            })
        }
        Mean | Var | Std => {
            if items.is_empty() {
                return Err(empty_error(name));
            }
            let count = items.len() as f64;
            let mean = pairwise_sum(items) / count;
            if function == Mean {
                return float_value(mean);
            }
            let deviations: Vec<f64> = items
                .iter()
                .map(|item| {
                    let deviation = item.to_f64() - mean;
                    deviation * deviation
                })
                .collect();
            let variance = pairwise_sum(&deviations) / count;
            float_value(if function == Var {
                variance
            } else {
                variance.sqrt()
            })
        }
        _ => unreachable!("reduction dispatch"),
    }
}

fn axis_parts(
    array: &ArrayValue,
    axis: i128,
) -> Result<(usize, usize, usize, Vec<usize>), ArrayError> {
    let rank = array.shape.len();
    let axis = usize::try_from(axis)
        .ok()
        .filter(|axis| *axis < rank)
        .ok_or_else(|| {
            (
                "R_ARRAY_SHAPE",
                format!("axis {axis} is outside a rank-{rank} array"),
            )
        })?;
    let outer: usize = array.shape[..axis].iter().product();
    let extent = array.shape[axis];
    let inner: usize = array.shape[axis + 1..].iter().product();
    let mut shape = array.shape.clone();
    shape.remove(axis);
    Ok((outer, extent, inner, shape))
}

fn reduce_axis<T: Numeric>(
    function: ArrayFunction,
    args: &[Value],
    charge: &mut Charge<'_>,
) -> Result<Value, ArrayError> {
    use ArrayFunction::*;
    let array = array_arg(args, 0)?;
    let (outer, extent, inner, shape) = axis_parts(array, int_arg(args, 1)?)?;
    let items = T::items(&array.data);
    let size = outer * inner;
    let name = &function.guest_name();
    if extent == 0 && size > 0 && function != SumAxis {
        return Err(empty_error(name));
    }
    let at = |o: usize, k: usize, i: usize| items[(o * extent + k) * inner + i];
    if function == MeanAxis {
        charge(array_bytes(ElementType::F64, shape.len(), size))?;
        let mut out = Vec::with_capacity(size);
        for o in 0..outer {
            for i in 0..inner {
                let sum = (0..extent).fold(0.0, |total, k| total + at(o, k, i).to_f64());
                let mean = sum / extent as f64;
                if !mean.is_finite() {
                    return Err((
                        NONFINITE,
                        format!(
                            "array_mean_axis is not finite at output index {}",
                            o * inner + i
                        ),
                    ));
                }
                out.push(mean);
            }
        }
        return finish(shape, out);
    }
    charge(array_bytes(T::TYPE, shape.len(), size))?;
    let mut out = Vec::with_capacity(size);
    for o in 0..outer {
        for i in 0..inner {
            let mut accumulator = if function == SumAxis {
                T::zero()
            } else {
                at(o, 0, i)
            };
            let start = usize::from(function != SumAxis);
            for k in start..extent {
                let item = at(o, k, i);
                accumulator = match function {
                    SumAxis => accumulator
                        .add(item)
                        .map_err(|code| element_error(code, name, T::TYPE, o * inner + i))?,
                    MinAxis if item < accumulator => item,
                    MaxAxis if item > accumulator => item,
                    _ => accumulator,
                };
            }
            out.push(accumulator);
        }
    }
    finish(shape, out)
}

fn order<T: Numeric>(
    function: ArrayFunction,
    args: &[Value],
    charge: &mut Charge<'_>,
    held: &mut usize,
) -> Result<Value, ArrayError> {
    use ArrayFunction::*;
    let array = array_arg(args, 0)?;
    let items = T::items(&array.data);
    let name = &function.guest_name();
    if function == Cumsum {
        charge(array_bytes(T::TYPE, 1, items.len()))?;
        let mut out = Vec::with_capacity(items.len());
        let mut running = T::zero();
        for (index, item) in items.iter().enumerate() {
            running = running
                .add(*item)
                .map_err(|code| element_error(code, name, T::TYPE, index))?;
            out.push(running);
        }
        return finish(vec![items.len()], out);
    }
    if array.shape.len() != 1 {
        return error(
            "R_ARRAY_SHAPE",
            format!(
                "{name} needs a rank-1 array, got rank {}",
                array.shape.len()
            ),
        );
    }
    hold(charge, held, items.len().saturating_mul(size_of::<usize>()))?;
    let mut indices: Vec<usize> = (0..items.len()).collect();
    // Elements are finite, so `partial_cmp` is total here; `sort_by` is stable.
    indices.sort_by(|a, b| items[*a].partial_cmp(&items[*b]).expect("finite elements"));
    if function == Argsort {
        charge(array_bytes(ElementType::I64, 1, items.len()))?;
        let out: Vec<i64> = indices.into_iter().map(|index| index as i64).collect();
        return finish(vec![items.len()], out);
    }
    charge(array_bytes(T::TYPE, 1, items.len()))?;
    let out: Vec<T> = indices.into_iter().map(|index| items[index]).collect();
    finish(vec![items.len()], out)
}

fn dot<T: Element>(args: &[Value]) -> Result<Value, ArrayError> {
    let (a, b) = (array_arg(args, 0)?, array_arg(args, 1)?);
    if a.shape.len() != 1 || a.shape != b.shape {
        return error(
            "R_ARRAY_SHAPE",
            format!(
                "array_dot needs two rank-1 arrays of equal length, got {:?} and {:?}",
                a.shape, b.shape
            ),
        );
    }
    let (x, y) = (T::items(&a.data), T::items(&b.data));
    if T::TYPE.is_integer() {
        let mut sum = ExactSum::default();
        for (p, q) in x.iter().zip(y) {
            let (p, q) = (p.to_int().expect("integer"), q.to_int().expect("integer"));
            match p.checked_mul(q) {
                Some(product) => sum.add(product),
                None => {
                    let product = &big_of(p) * &big_of(q);
                    let current = std::mem::take(&mut sum).value();
                    sum.big = Some(&current + &product);
                }
            }
        }
        return Ok(Value::Int(sum.value()));
    }
    float_value(
        x.iter()
            .zip(y)
            .fold(0.0, |total, (p, q)| total + p.to_f64() * q.to_f64()),
    )
}

const MATMUL_BLOCK: usize = 64;

fn matmul<T: Element>(
    args: &[Value],
    charge: &mut Charge<'_>,
    held: &mut usize,
) -> Result<Value, ArrayError> {
    let (a, b) = (array_arg(args, 0)?, array_arg(args, 1)?);
    let (n, k) = match a.shape.as_slice() {
        [n, k] => (*n, *k),
        _ => {
            return error(
                "R_ARRAY_SHAPE",
                format!(
                    "array_matmul needs a rank-2 left operand, got {:?}",
                    a.shape
                ),
            )
        }
    };
    let (kb, m, vector) = match b.shape.as_slice() {
        [kb, m] => (*kb, *m, false),
        [kb] => (*kb, 1, true),
        _ => {
            return error(
                "R_ARRAY_SHAPE",
                format!(
                    "array_matmul needs a rank-1 or rank-2 right operand, got {:?}",
                    b.shape
                ),
            )
        }
    };
    if k != kb {
        return error(
            "R_ARRAY_SHAPE",
            format!("cannot multiply {:?} by {:?}", a.shape, b.shape),
        );
    }
    let shape = if vector { vec![n] } else { vec![n, m] };
    charge(array_bytes(T::TYPE, shape.len(), n * m))?;
    let (x, y) = (T::items(&a.data), T::items(&b.data));
    if T::TYPE.is_integer() {
        hold(charge, held, (n * m).saturating_mul(size_of::<i128>()))?;
        let mut acc = vec![0i128; n * m];
        for i in 0..n {
            for p in 0..k {
                let left = x[i * k + p].to_int().expect("integer");
                if left == 0 {
                    continue;
                }
                let row = &mut acc[i * m..(i + 1) * m];
                for (j, cell) in row.iter_mut().enumerate() {
                    let overflow = || element_error(OVERFLOW, "array_matmul", T::TYPE, i * m + j);
                    let product = left
                        .checked_mul(y[p * m + j].to_int().expect("integer"))
                        .ok_or_else(overflow)?;
                    *cell = cell.checked_add(product).ok_or_else(overflow)?;
                }
            }
        }
        let mut out = Vec::with_capacity(n * m);
        for (index, value) in acc.into_iter().enumerate() {
            out.push(
                T::from_int(value)
                    .ok_or_else(|| element_error(OVERFLOW, "array_matmul", T::TYPE, index))?,
            );
        }
        return finish(shape, out);
    }
    hold(
        charge,
        held,
        (x.len() + y.len() + n * m).saturating_mul(size_of::<f64>()),
    )?;
    let left: Vec<f64> = x.iter().map(|item| item.to_f64()).collect();
    let right: Vec<f64> = y.iter().map(|item| item.to_f64()).collect();
    let mut acc = vec![0.0f64; n * m];
    for i0 in (0..n).step_by(MATMUL_BLOCK) {
        for p0 in (0..k).step_by(MATMUL_BLOCK) {
            for j0 in (0..m).step_by(MATMUL_BLOCK) {
                let (i1, p1, j1) = (
                    (i0 + MATMUL_BLOCK).min(n),
                    (p0 + MATMUL_BLOCK).min(k),
                    (j0 + MATMUL_BLOCK).min(m),
                );
                for i in i0..i1 {
                    for p in p0..p1 {
                        let scale = left[i * k + p];
                        let row = &mut acc[i * m + j0..i * m + j1];
                        let column = &right[p * m + j0..p * m + j1];
                        for (cell, value) in row.iter_mut().zip(column) {
                            *cell += scale * value;
                        }
                    }
                }
            }
        }
    }
    let mut out = Vec::with_capacity(n * m);
    for (index, value) in acc.into_iter().enumerate() {
        out.push(
            T::from_float(value)
                .ok_or_else(|| element_error(NONFINITE, "array_matmul", T::TYPE, index))?,
        );
    }
    finish(shape, out)
}

fn generate(
    function: ArrayFunction,
    args: &[Value],
    charge: &mut Charge<'_>,
) -> Result<Value, ArrayError> {
    use ArrayFunction::*;
    match function {
        Range => {
            let (start, stop, step) = (int_arg(args, 0)?, int_arg(args, 1)?, int_arg(args, 2)?);
            if step == 0 {
                return error("R_ARRAY_ARGUMENT", "array_i64_range step must not be 0");
            }
            let overflow = || {
                (
                    "R_ARRAY_CONVERSION",
                    "array_i64_range values must fit in i64".to_string(),
                )
            };
            // Operands are i128, so the span, the count and the last value are all checked.
            let span = stop.checked_sub(start).ok_or_else(overflow)?;
            let count_wide: u128 = if (span > 0) == (step > 0) && span != 0 {
                let (distance, stride) = (span.unsigned_abs(), step.unsigned_abs());
                distance / stride + u128::from(distance % stride != 0)
            } else {
                0
            };
            let count = usize::try_from(count_wide).map_err(|_| overflow())?;
            let last = match count_wide.checked_sub(1) {
                Some(steps) => i128::try_from(steps)
                    .ok()
                    .and_then(|steps| step.checked_mul(steps))
                    .and_then(|offset| start.checked_add(offset))
                    .ok_or_else(overflow)?,
                None => start,
            };
            if i64::try_from(start).is_err() || i64::try_from(last).is_err() {
                return Err(overflow());
            }
            charge(array_bytes(ElementType::I64, 1, count))?;
            let out: Vec<i64> = (0..count)
                .map(|index| (start + step * index as i128) as i64)
                .collect();
            finish(vec![count], out)
        }
        Linspace => {
            let (start, stop) = (float_arg(args, 0)?, float_arg(args, 1)?);
            let count = usize::try_from(int_arg(args, 2)?).map_err(|_| {
                (
                    "R_ARRAY_SHAPE",
                    "array_f64_linspace count must not be negative".to_string(),
                )
            })?;
            charge(array_bytes(ElementType::F64, 1, count))?;
            let step = if count > 1 {
                (stop - start) / (count - 1) as f64
            } else {
                0.0
            };
            let mut out: Vec<f64> = (0..count)
                .map(|index| start + step * index as f64)
                .collect();
            if count > 1 {
                out[count - 1] = stop;
            }
            if out.iter().any(|value| !value.is_finite()) {
                return error(NONFINITE, "array_f64_linspace produced a non-finite value");
            }
            finish(vec![count], out)
        }
        Random | Normal => {
            let seed = match &args[0] {
                Value::Int(seed) => crate::bigint_low_u64(seed),
                other => return error("R_TYPE_INVALID", format!("expected Int, found {other}")),
            };
            let shape = shape_from_list(&args[1])?;
            let size =
                shape_size(&shape).ok_or(("R_ARRAY_SHAPE", "shape is too large".to_string()))?;
            charge(array_bytes(ElementType::F64, shape.len(), size))?;
            let mut state = seed;
            let mut next = || {
                state = state.wrapping_add(crate::SPLITMIX64_GAMMA);
                crate::random_unit_float(state).get()
            };
            let out: Vec<f64> = if function == Random {
                (0..size).map(|_| next()).collect()
            } else {
                let mut out = Vec::with_capacity(size);
                while out.len() < size {
                    // 1 - u lies in (0, 1], so the logarithm is finite.
                    let radius = (-2.0 * (1.0 - next()).ln()).sqrt();
                    let angle = std::f64::consts::TAU * next();
                    out.push(radius * angle.cos());
                    if out.len() < size {
                        out.push(radius * angle.sin());
                    }
                }
                out
            };
            finish(shape, out)
        }
        _ => unreachable!("generator dispatch"),
    }
}

fn shape_from_list(value: &Value) -> Result<Vec<usize>, ArrayError> {
    let len = value
        .list_len()
        .ok_or(("R_TYPE_INVALID", "expected a List of extents".to_string()))?;
    (0..len)
        .map(|index| match value.list_item(index) {
            Some(Value::Int(extent)) => extent.to_usize().ok_or_else(|| {
                (
                    "R_ARRAY_SHAPE",
                    format!("extent {extent} must be a non-negative integer"),
                )
            }),
            _ => error("R_TYPE_INVALID", "expected Int extents"),
        })
        .collect()
}

macro_rules! with_numeric {
    ($element:expr, $T:ident => $body:expr) => {
        match $element {
            ElementType::I8 => {
                type $T = i8;
                $body
            }
            ElementType::I16 => {
                type $T = i16;
                $body
            }
            ElementType::I32 => {
                type $T = i32;
                $body
            }
            ElementType::I64 => {
                type $T = i64;
                $body
            }
            ElementType::U8 => {
                type $T = u8;
                $body
            }
            ElementType::U16 => {
                type $T = u16;
                $body
            }
            ElementType::U32 => {
                type $T = u32;
                $body
            }
            ElementType::U64 => {
                type $T = u64;
                $body
            }
            ElementType::F32 => {
                type $T = f32;
                $body
            }
            ElementType::F64 => {
                type $T = f64;
                $body
            }
            ElementType::Bool => {
                return error(
                    "R_TYPE_INVALID",
                    "this array function needs a numeric array",
                )
            }
        }
    };
}

/// Charges a transient buffer and notes it in `held`, so `apply` can release it however the
/// kernel ends.
fn hold(charge: &mut Charge<'_>, held: &mut usize, bytes: usize) -> Result<(), ArrayError> {
    charge(bytes)?;
    *held = held.saturating_add(bytes);
    Ok(())
}

pub fn apply(
    function: ArrayFunction,
    element: ElementType,
    args: &mut [Value],
    charge: &mut Charge<'_>,
    release: &mut Release<'_>,
) -> Result<Value, ArrayError> {
    let mut held = 0usize;
    let result = apply_kernel(function, element, args, charge, &mut held);
    release(held);
    result
}

fn apply_kernel(
    function: ArrayFunction,
    element: ElementType,
    args: &mut [Value],
    charge: &mut Charge<'_>,
    held: &mut usize,
) -> Result<Value, ArrayError> {
    use ArrayFunction::*;
    if !function.element_class().accepts(element) {
        return error(
            "R_TYPE_INVALID",
            format!(
                "{} does not accept {} arrays",
                function.guest_name(),
                element.name()
            ),
        );
    }
    match function {
        Add | Sub | Mul | Div | Minimum | Maximum | AddScalar | SubScalar | MulScalar
        | DivScalar => with_numeric!(element, T => arithmetic::<T>(function, args, charge)),
        Neg => with_numeric!(element, T => map_owned(args, charge, "array_neg", |x: T| {
            Numeric::neg(x).map_err(|code| (code, String::new()))
        })),
        Abs => with_numeric!(element, T => map_owned(args, charge, "array_abs", |x: T| {
            Numeric::abs(x).map_err(|code| (code, String::new()))
        })),
        Clip => with_numeric!(element, T => {
            let (low, high) = (T::from_value(&args[1])?, T::from_value(&args[2])?);
            if low > high {
                return error("R_ARRAY_ARGUMENT", "array_clip needs low <= high");
            }
            map_owned(args, charge, "array_clip", |x: T| {
                Ok(if x < low { low } else if x > high { high } else { x })
            })
        }),
        Sqrt | Exp | Log | Log10 | Log2 | Sin | Cos | Tan | Asin | Acos | Atan | Sinh | Cosh
        | Tanh | Floor | Ceil | Round | Pow => match element {
            ElementType::F32 => float_math::<f32>(function, args, charge),
            _ => float_math::<f64>(function, args, charge),
        },
        Eq | Ne | Lt | Le | Gt | Ge | EqScalar | NeScalar | LtScalar | LeScalar | GtScalar
        | GeScalar => with_element!(element, T => compare::<T>(function, args, charge)),
        And | Or | Not | Any | All | Count => logic(function, args, charge),
        Where | Filter => with_element!(element, T => select::<T>(function, args, charge)),
        Sum | Prod | Min | Max | Argmin | Argmax | Mean | Var | Std => {
            with_numeric!(element, T => reduce::<T>(function, args))
        }
        SumAxis | MinAxis | MaxAxis | MeanAxis => {
            with_numeric!(element, T => reduce_axis::<T>(function, args, charge))
        }
        Sort | Argsort | Cumsum => {
            with_numeric!(element, T => order::<T>(function, args, charge, held))
        }
        Dot => with_numeric!(element, T => dot::<T>(args)),
        Matmul => with_numeric!(element, T => matmul::<T>(args, charge, held)),
        Range | Linspace | Random | Normal => generate(function, args, charge),
        Take => with_element!(element, T => take::<T>(args, charge)),
        Factorize => with_element!(element, T => factorize::<T>(args, charge, held)),
        FirstIndex | CountBy => by_code(function, args, charge),
        SumBy | MinBy | MaxBy | MeanBy => {
            with_numeric!(element, T => reduce_by::<T>(function, args, charge))
        }
        JoinIndex => join_index(args, charge, held),
        _ => error(
            "R_TYPE_INVALID",
            format!("{} is not a compute instruction", function.guest_name()),
        ),
    }
}

/// A multiply-rotate hasher for integer keys; the grouping kernels hash millions of codes and
/// the default SipHash would dominate their cost. Keys are guest data, but the tables are
/// per-call and bounded by the allocation budget, so collision flooding only slows one call.
#[derive(Default)]
struct KeyHasher(u64);

impl std::hash::Hasher for KeyHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.write_u64(u64::from(*byte));
        }
    }
    fn write_u64(&mut self, value: u64) {
        self.0 = (self.0.rotate_left(5) ^ value).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    fn write_i64(&mut self, value: i64) {
        self.write_u64(value as u64);
    }
    fn write_u128(&mut self, value: u128) {
        self.write_u64(value as u64);
        self.write_u64((value >> 64) as u64);
    }
    fn write_i128(&mut self, value: i128) {
        self.write_u128(value as u128);
    }
}

type KeyMap<K, V> = std::collections::HashMap<K, V, std::hash::BuildHasherDefault<KeyHasher>>;

fn rank1<'a>(array: &'a ArrayValue, name: &str) -> Result<&'a ArrayValue, ArrayError> {
    if array.shape.len() != 1 {
        return error(
            "R_ARRAY_SHAPE",
            format!(
                "{name} needs a rank-1 array, got rank {}",
                array.shape.len()
            ),
        );
    }
    Ok(array)
}

fn i64_items<'a>(array: &'a ArrayValue, name: &str) -> Result<&'a [i64], ArrayError> {
    let array = rank1(array, name)?;
    match &array.data {
        guion_model::ArrayData::I64(items) => Ok(items),
        _ => error("R_TYPE_INVALID", format!("{name} needs an Array[i64]")),
    }
}

fn group_count(args: &[Value], index: usize, name: &str) -> Result<usize, ArrayError> {
    let groups = int_arg(args, index)?;
    if groups < 0 || groups > i128::from(u32::MAX) {
        return error(
            "R_ARRAY_SHAPE",
            format!(
                "{name} needs a group count from 0 to {}, got {groups}",
                u32::MAX
            ),
        );
    }
    Ok(groups as usize)
}

/// Checks every code is a group index and returns them as positions.
fn group_codes(codes: &[i64], groups: usize, name: &str) -> Result<(), ArrayError> {
    for (row, code) in codes.iter().enumerate() {
        if *code < 0 || *code as u64 >= groups as u64 {
            return error(
                "R_ARRAY_INDEX",
                format!("{name}: code {code} at row {row} is outside 0..{groups}"),
            );
        }
    }
    Ok(())
}

fn take<T: Element>(args: &[Value], charge: &mut Charge<'_>) -> Result<Value, ArrayError> {
    let source = rank1(array_arg(args, 0)?, "array_take")?;
    let indices = i64_items(array_arg(args, 1)?, "array_take")?;
    let items = T::items(&source.data);
    charge(array_bytes(T::TYPE, 1, indices.len()))?;
    let mut out = Vec::with_capacity(indices.len());
    for (position, index) in indices.iter().enumerate() {
        match usize::try_from(*index)
            .ok()
            .and_then(|index| items.get(index))
        {
            Some(item) => out.push(*item),
            None => {
                return error(
                    "R_ARRAY_INDEX",
                    format!(
                        "array_take: index {index} at position {position} is outside 0..{}",
                        items.len()
                    ),
                )
            }
        }
    }
    finish(vec![indices.len()], out)
}

/// The hash key of an element: floats by value with `-0.0` equal to `0.0`.
fn element_key<T: Element>(item: T) -> i128 {
    match item.to_int() {
        Some(int) => int,
        None => i128::from((item.to_f64() + 0.0).to_bits()),
    }
}

fn factorize<T: Element>(
    args: &[Value],
    charge: &mut Charge<'_>,
    held: &mut usize,
) -> Result<Value, ArrayError> {
    let source = rank1(array_arg(args, 0)?, "array_factorize")?;
    let items = T::items(&source.data);
    charge(array_bytes(ElementType::I64, 1, items.len()))?;
    let mut codes: KeyMap<i128, i64> = KeyMap::default();
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let next = codes.len() as i64;
        let code = *codes.entry(element_key(*item)).or_insert_with(|| next);
        if code == next {
            hold(charge, held, 48)?;
        }
        out.push(code);
    }
    finish(vec![items.len()], out)
}

fn by_code(
    function: ArrayFunction,
    args: &[Value],
    charge: &mut Charge<'_>,
) -> Result<Value, ArrayError> {
    let name = if function == ArrayFunction::FirstIndex {
        "array_first_index"
    } else {
        "array_count_by"
    };
    let codes = i64_items(array_arg(args, 0)?, name)?;
    let groups = group_count(args, 1, name)?;
    group_codes(codes, groups, name)?;
    charge(array_bytes(ElementType::I64, 1, groups))?;
    if function == ArrayFunction::FirstIndex {
        let mut out = vec![-1i64; groups];
        for (row, code) in codes.iter().enumerate().rev() {
            out[*code as usize] = row as i64;
        }
        return finish(vec![groups], out);
    }
    let mut out = vec![0i64; groups];
    for code in codes {
        out[*code as usize] += 1;
    }
    finish(vec![groups], out)
}

fn reduce_by<T: Numeric>(
    function: ArrayFunction,
    args: &[Value],
    charge: &mut Charge<'_>,
) -> Result<Value, ArrayError> {
    use ArrayFunction::*;
    let name = match function {
        SumBy => "array_sum_by",
        MinBy => "array_min_by",
        MaxBy => "array_max_by",
        _ => "array_mean_by",
    };
    let values = rank1(array_arg(args, 0)?, name)?;
    let codes = i64_items(array_arg(args, 1)?, name)?;
    let groups = group_count(args, 2, name)?;
    let items = T::items(&values.data);
    if items.len() != codes.len() {
        return error(
            "R_ARRAY_SHAPE",
            format!(
                "{name} needs one code per value, got {} values and {} codes",
                items.len(),
                codes.len()
            ),
        );
    }
    group_codes(codes, groups, name)?;
    let empty_group = |seen: &[bool]| -> Result<(), ArrayError> {
        match seen.iter().position(|seen| !seen) {
            Some(group) => error(
                "R_ARRAY_EMPTY",
                format!("{name}: group {group} has no values"),
            ),
            None => Ok(()),
        }
    };
    match function {
        SumBy => {
            charge(array_bytes(T::TYPE, 1, groups))?;
            let mut out = vec![T::zero(); groups];
            for (row, (item, code)) in items.iter().zip(codes).enumerate() {
                let slot = &mut out[*code as usize];
                *slot = slot
                    .add(*item)
                    .map_err(|code| element_error(code, name, T::TYPE, row))?;
            }
            finish(vec![groups], out)
        }
        MinBy | MaxBy => {
            charge(array_bytes(T::TYPE, 1, groups).saturating_add(groups))?;
            let mut out = vec![T::zero(); groups];
            let mut seen = vec![false; groups];
            for (item, code) in items.iter().zip(codes) {
                let group = *code as usize;
                let better = !seen[group]
                    || if function == MinBy {
                        *item < out[group]
                    } else {
                        *item > out[group]
                    };
                if better {
                    out[group] = *item;
                    seen[group] = true;
                }
            }
            empty_group(&seen)?;
            finish(vec![groups], out)
        }
        _ => {
            charge(array_bytes(ElementType::F64, 1, groups).saturating_add(groups * 16))?;
            let mut sums = vec![0.0f64; groups];
            let mut counts = vec![0u64; groups];
            for (item, code) in items.iter().zip(codes) {
                sums[*code as usize] += item.to_f64();
                counts[*code as usize] += 1;
            }
            let seen: Vec<bool> = counts.iter().map(|count| *count > 0).collect();
            empty_group(&seen)?;
            let mut out = Vec::with_capacity(groups);
            for (group, (sum, count)) in sums.iter().zip(&counts).enumerate() {
                let mean = sum / *count as f64;
                if !mean.is_finite() {
                    return error(NONFINITE, format!("{name} is not finite at group {group}"));
                }
                out.push(mean + 0.0);
            }
            finish(vec![groups], out)
        }
    }
}

fn join_index(
    args: &[Value],
    charge: &mut Charge<'_>,
    held: &mut usize,
) -> Result<Value, ArrayError> {
    let name = "array_join_index";
    let left = i64_items(array_arg(args, 0)?, name)?;
    let right = i64_items(array_arg(args, 1)?, name)?;
    let keep_unmatched = matches!(args[2], Value::Bool(true));
    // Right rows grouped by code, in right order: `slots[code]` indexes `starts`.
    hold(charge, held, right.len().saturating_mul(40))?;
    let mut slots: KeyMap<i64, usize> = KeyMap::default();
    let mut counts: Vec<usize> = Vec::new();
    let mut slot_of_row = Vec::with_capacity(right.len());
    for code in right {
        if *code < 0 {
            slot_of_row.push(usize::MAX);
            continue;
        }
        let next = counts.len();
        let slot = *slots.entry(*code).or_insert(next);
        if slot == next {
            counts.push(0);
        }
        counts[slot] += 1;
        slot_of_row.push(slot);
    }
    let mut starts = Vec::with_capacity(counts.len() + 1);
    let mut total = 0usize;
    for count in &counts {
        starts.push(total);
        total += count;
    }
    starts.push(total);
    let mut fill = starts.clone();
    let mut rows = vec![0usize; total];
    for (row, slot) in slot_of_row.iter().enumerate() {
        if *slot != usize::MAX {
            rows[fill[*slot]] = row;
            fill[*slot] += 1;
        }
    }
    let matches_of = |code: i64| -> Option<usize> {
        if code < 0 {
            None
        } else {
            slots.get(&code).copied()
        }
    };
    let mut pairs = 0usize;
    for code in left {
        let found = matches_of(*code).map_or(0, |slot| starts[slot + 1] - starts[slot]);
        let found = if found == 0 && keep_unmatched {
            1
        } else {
            found
        };
        pairs = pairs.checked_add(found).ok_or_else(|| {
            (
                "R_ARRAY_SHAPE",
                format!("{name}: the join has too many rows"),
            )
        })?;
    }
    let length = pairs.checked_mul(2).ok_or_else(|| {
        (
            "R_ARRAY_SHAPE",
            format!("{name}: the join has too many rows"),
        )
    })?;
    charge(array_bytes(ElementType::I64, 1, length))?;
    let mut out = vec![0i64; length];
    let mut next = 0usize;
    for (row, code) in left.iter().enumerate() {
        match matches_of(*code) {
            Some(slot) => {
                for right_row in &rows[starts[slot]..starts[slot + 1]] {
                    out[next] = row as i64;
                    out[pairs + next] = *right_row as i64;
                    next += 1;
                }
            }
            None if keep_unmatched => {
                out[next] = row as i64;
                out[pairs + next] = -1;
                next += 1;
            }
            None => {}
        }
    }
    finish(vec![length], out)
}

#[cfg(test)]
mod budget_tests {
    use super::*;
    use std::cell::Cell;

    fn array(element: ElementType, shape: Vec<usize>, fill: impl Fn(usize) -> i64) -> Value {
        let size: usize = shape.iter().product();
        let data = match element {
            ElementType::F64 => crate::array::Element::wrap(
                (0..size)
                    .map(|index| fill(index) as f64)
                    .collect::<Vec<f64>>(),
            ),
            _ => crate::array::Element::wrap((0..size).map(&fill).collect::<Vec<i64>>()),
        };
        Value::Array(Shared::new(ArrayValue::new(shape, data).expect("array")))
    }

    /// Runs a kernel against a simulated in-use budget of `limit` bytes. Returns the result
    /// and the bytes still charged afterwards.
    fn run(
        function: ArrayFunction,
        element: ElementType,
        mut args: Vec<Value>,
        limit: usize,
        used: &Cell<usize>,
    ) -> Result<Value, ArrayError> {
        let mut charge = |bytes: usize| {
            if used.get().saturating_add(bytes) > limit {
                Err(("R_ALLOCATION_LIMIT", "limit".to_string()))
            } else {
                used.set(used.get() + bytes);
                Ok(())
            }
        };
        let mut release = |bytes: usize| used.set(used.get() - bytes);
        crate::array::apply(function, element, &mut args, &mut charge, &mut release)
    }

    fn assert_limit(result: Result<Value, ArrayError>, what: &str) {
        match result {
            Err((code, _)) => assert_eq!(code, "R_ALLOCATION_LIMIT", "{what}"),
            Ok(_) => panic!("{what}: fit in a budget that holds only its output"),
        }
    }

    #[test]
    fn matmul_temporaries_do_not_fit_a_budget_that_holds_only_the_output() {
        for element in [ElementType::F64, ElementType::I64] {
            let a = array(element, vec![16, 16], |i| i as i64 % 7);
            let b = array(element, vec![16, 16], |i| i as i64 % 5);
            let output = array_bytes(element, 2, 256);
            let used = Cell::new(0);
            let result = run(ArrayFunction::Matmul, element, vec![a, b], output, &used);
            assert_limit(result, "matmul");
            assert_eq!(
                used.get(),
                output,
                "a failed matmul leaves only its output charge"
            );
        }
    }

    #[test]
    fn sort_and_argsort_index_vectors_are_charged() {
        for (function, out_element) in [
            (ArrayFunction::Sort, ElementType::I64),
            (ArrayFunction::Argsort, ElementType::I64),
        ] {
            let a = array(ElementType::I64, vec![512], |i| (i as i64 * 7919) % 513);
            let output = array_bytes(out_element, 1, 512);
            let used = Cell::new(0);
            let result = run(function, ElementType::I64, vec![a], output, &used);
            assert_limit(result, "sort index vector");
        }
    }

    #[test]
    fn factorize_hash_table_is_released() {
        let a = array(ElementType::I64, vec![256], |i| i as i64);
        let output = array_bytes(ElementType::I64, 1, 256);
        let used = Cell::new(0);
        let result = run(
            ArrayFunction::Factorize,
            ElementType::I64,
            vec![a],
            1 << 30,
            &used,
        );
        assert!(result.is_ok());
        assert_eq!(used.get(), output, "only the output stays charged");
    }

    #[test]
    fn join_index_tables_are_charged_and_released() {
        let left = array(ElementType::I64, vec![128], |i| i as i64 % 40);
        let right = array(ElementType::I64, vec![128], |i| i as i64 % 40);
        let args = || vec![left.clone(), right.clone(), Value::Bool(false)];
        let used = Cell::new(0);
        let result = run(
            ArrayFunction::JoinIndex,
            ElementType::I64,
            args(),
            1 << 30,
            &used,
        );
        let Ok(Value::Array(out)) = result else {
            panic!("join_index failed");
        };
        assert_eq!(used.get(), array_bytes(ElementType::I64, 1, out.size()));
        // A budget that holds the output but not the tables raises the limit.
        let tight = Cell::new(0);
        let result = run(
            ArrayFunction::JoinIndex,
            ElementType::I64,
            args(),
            array_bytes(ElementType::I64, 1, out.size()),
            &tight,
        );
        assert_limit(result, "join_index");
    }

    #[test]
    fn a_thousand_argsorts_under_a_small_budget_complete() {
        let a = array(ElementType::I64, vec![64], |i| (i as i64 * 31) % 64);
        let output = array_bytes(ElementType::I64, 1, 64);
        let used = Cell::new(0);
        for _ in 0..1000 {
            let result = run(
                ArrayFunction::Argsort,
                ElementType::I64,
                vec![a.clone()],
                output + 64 * 8,
                &used,
            );
            assert!(result.is_ok(), "argsort ran out of budget");
            // Dropping the result releases its own charge, as the VM does.
            used.set(used.get() - output);
            assert_eq!(used.get(), 0, "temporaries must be released");
        }
    }
}
