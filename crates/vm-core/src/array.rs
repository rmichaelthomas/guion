//! Array kernels behind `Op::Array`. Every kernel checks its inputs, charges the output
//! against the allocation budget before allocating it, and never produces a value outside
//! its element type: integers that do not fit raise `R_ARRAY_CONVERSION` or
//! `R_ARRAY_OVERFLOW`, and floats are always finite.

use guion_model::{
    shape_size, ArrayData, ArrayFunction, ArrayValue, BigInt, ElementType, Float64, Shared, Value,
    MAX_ARRAY_RANK,
};

/// A kernel failure: a stable runtime error code and a message.
pub type ArrayError = (&'static str, String);

/// Charges bytes against the allocation budget before they are allocated.
pub type Charge<'a> = dyn FnMut(usize) -> Result<(), ArrayError> + 'a;

fn error<T>(code: &'static str, message: impl Into<String>) -> Result<T, ArrayError> {
    Err((code, message.into()))
}

/// Bytes an array of `size` elements holds, for allocation charges.
fn array_bytes(element: ElementType, rank: usize, size: usize) -> usize {
    std::mem::size_of::<ArrayValue>()
        .saturating_add(rank.saturating_mul(std::mem::size_of::<usize>()))
        .saturating_add(size.saturating_mul(element.width()))
}

fn new_array(shape: Vec<usize>, data: ArrayData) -> Result<Value, ArrayError> {
    Ok(Value::Array(Shared::new(ArrayValue::new(shape, data)?)))
}

/// An exact integer read from a scalar `Int`, when it fits 128 bits.
fn int_of(value: &BigInt) -> Option<i128> {
    match value.to_i64() {
        Some(small) => Some(i128::from(small)),
        None if value.digit_count() <= 39 => value.to_string().parse().ok(),
        None => None,
    }
}

fn big_of(value: i128) -> BigInt {
    match i64::try_from(value) {
        Ok(small) => BigInt::from_i64(small),
        Err(_) => BigInt::parse(&value.to_string()).expect("decimal digits parse"),
    }
}

#[allow(dead_code)]
pub(crate) fn float_value(value: f64) -> Result<Value, ArrayError> {
    Float64::new(value).map(Value::Float64).ok_or_else(|| {
        (
            "R_FLOAT_NONFINITE",
            format!("{value} is not a finite Float64"),
        )
    })
}

/// One element type's storage and conversions.
pub trait Element: Copy + PartialOrd + Default + Send + Sync + 'static {
    const TYPE: ElementType;
    fn wrap(items: Vec<Self>) -> ArrayData;
    fn items(data: &ArrayData) -> &[Self];
    fn items_mut(data: &mut ArrayData) -> &mut Vec<Self>;
    /// Exact conversion from an integer, or `None` when it does not fit.
    fn from_int(value: i128) -> Option<Self>;
    /// Checked conversion from a finite float: integers truncate toward zero.
    fn from_float(value: f64) -> Option<Self>;
    fn to_value(self) -> Value;
    /// The element as an exact integer (`None` for floats).
    fn to_int(self) -> Option<i128>;
    fn to_f64(self) -> f64;
    fn write_le(self, out: &mut Vec<u8>);
    fn read_le(bytes: &[u8]) -> Result<Self, ArrayError>;

    /// Converts a scalar guest value into this element type.
    fn from_value(value: &Value) -> Result<Self, ArrayError> {
        let converted = match value {
            Value::Int(int) => int_of(int).and_then(Self::from_int),
            Value::Float64(float) => {
                if Self::TYPE.is_float() {
                    Self::from_float(float.get())
                } else {
                    None
                }
            }
            Value::Bool(flag) => Self::from_int(i128::from(*flag)),
            _ => None,
        };
        converted.ok_or_else(|| {
            (
                "R_ARRAY_CONVERSION",
                format!("{value} does not fit in {}", Self::TYPE.name()),
            )
        })
    }
}

macro_rules! integer_element {
    ($ty:ty, $variant:ident, $tag:ident) => {
        impl Element for $ty {
            const TYPE: ElementType = ElementType::$tag;
            fn wrap(items: Vec<Self>) -> ArrayData {
                ArrayData::$variant(items)
            }
            fn items(data: &ArrayData) -> &[Self] {
                match data {
                    ArrayData::$variant(items) => items,
                    _ => unreachable!("array storage matches its element type"),
                }
            }
            fn items_mut(data: &mut ArrayData) -> &mut Vec<Self> {
                match data {
                    ArrayData::$variant(items) => items,
                    _ => unreachable!("array storage matches its element type"),
                }
            }
            fn from_int(value: i128) -> Option<Self> {
                <$ty>::try_from(value).ok()
            }
            fn from_float(value: f64) -> Option<Self> {
                let truncated = value.trunc();
                // Every integer type's range is exactly representable at its bounds in f64
                // except the upper bound of 64-bit types, which rounds up; compare in i128.
                if !(-1.8e19..=1.9e19).contains(&truncated) {
                    return None;
                }
                Self::from_int(truncated as i128)
            }
            fn to_value(self) -> Value {
                Value::Int(big_of(i128::from(self)))
            }
            fn to_int(self) -> Option<i128> {
                Some(i128::from(self))
            }
            fn to_f64(self) -> f64 {
                self as f64
            }
            fn write_le(self, out: &mut Vec<u8>) {
                out.extend_from_slice(&self.to_le_bytes());
            }
            fn read_le(bytes: &[u8]) -> Result<Self, ArrayError> {
                Ok(<$ty>::from_le_bytes(
                    bytes.try_into().expect("caller passes the element width"),
                ))
            }
        }
    };
}

integer_element!(i8, I8, I8);
integer_element!(i16, I16, I16);
integer_element!(i32, I32, I32);
integer_element!(i64, I64, I64);
integer_element!(u8, U8, U8);
integer_element!(u16, U16, U16);
integer_element!(u32, U32, U32);
integer_element!(u64, U64, U64);

macro_rules! float_element {
    ($ty:ty, $variant:ident, $tag:ident) => {
        impl Element for $ty {
            const TYPE: ElementType = ElementType::$tag;
            fn wrap(items: Vec<Self>) -> ArrayData {
                ArrayData::$variant(items)
            }
            fn items(data: &ArrayData) -> &[Self] {
                match data {
                    ArrayData::$variant(items) => items,
                    _ => unreachable!("array storage matches its element type"),
                }
            }
            fn items_mut(data: &mut ArrayData) -> &mut Vec<Self> {
                match data {
                    ArrayData::$variant(items) => items,
                    _ => unreachable!("array storage matches its element type"),
                }
            }
            fn from_int(value: i128) -> Option<Self> {
                Some(value as $ty)
            }
            fn from_float(value: f64) -> Option<Self> {
                let converted = value as $ty;
                converted.is_finite().then_some(converted)
            }
            fn to_value(self) -> Value {
                Value::Float64(Float64::new(self as f64).expect("array floats are finite"))
            }
            fn to_int(self) -> Option<i128> {
                None
            }
            fn to_f64(self) -> f64 {
                self as f64
            }
            fn write_le(self, out: &mut Vec<u8>) {
                out.extend_from_slice(&self.to_le_bytes());
            }
            fn read_le(bytes: &[u8]) -> Result<Self, ArrayError> {
                let value = <$ty>::from_le_bytes(
                    bytes.try_into().expect("caller passes the element width"),
                );
                if value.is_finite() {
                    Ok(value)
                } else {
                    error("R_FLOAT_NONFINITE", "array bytes hold a non-finite float")
                }
            }
        }
    };
}

float_element!(f32, F32, F32);
float_element!(f64, F64, F64);

impl Element for bool {
    const TYPE: ElementType = ElementType::Bool;
    fn wrap(items: Vec<Self>) -> ArrayData {
        ArrayData::Bool(items)
    }
    fn items(data: &ArrayData) -> &[Self] {
        match data {
            ArrayData::Bool(items) => items,
            _ => unreachable!("array storage matches its element type"),
        }
    }
    fn items_mut(data: &mut ArrayData) -> &mut Vec<Self> {
        match data {
            ArrayData::Bool(items) => items,
            _ => unreachable!("array storage matches its element type"),
        }
    }
    fn from_int(value: i128) -> Option<Self> {
        match value {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
    fn from_float(value: f64) -> Option<Self> {
        if value == 0.0 {
            Some(false)
        } else if value == 1.0 {
            Some(true)
        } else {
            None
        }
    }
    fn to_value(self) -> Value {
        Value::Bool(self)
    }
    fn to_int(self) -> Option<i128> {
        Some(i128::from(self))
    }
    fn to_f64(self) -> f64 {
        f64::from(u8::from(self))
    }
    fn write_le(self, out: &mut Vec<u8>) {
        out.push(u8::from(self));
    }
    fn read_le(bytes: &[u8]) -> Result<Self, ArrayError> {
        match bytes[0] {
            0 => Ok(false),
            1 => Ok(true),
            other => error(
                "R_ARRAY_CONVERSION",
                format!("byte {other} is not a bool (0 or 1)"),
            ),
        }
    }
}

/// Runs a generic body with `$T` bound to the Rust type of an element type.
macro_rules! with_element {
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
                type $T = bool;
                $body
            }
        }
    };
}
#[allow(unused_imports)]
pub(crate) use with_element;

fn take(args: &mut [Value], index: usize) -> Value {
    std::mem::replace(&mut args[index], Value::Unit)
}

fn array_arg(args: &[Value], index: usize) -> Result<&Shared<ArrayValue>, ArrayError> {
    match &args[index] {
        Value::Array(array) => Ok(array),
        other => error(
            "R_TYPE_INVALID",
            format!("expected an array operand, found {other}"),
        ),
    }
}

fn int_arg(args: &[Value], index: usize) -> Result<i128, ArrayError> {
    match &args[index] {
        Value::Int(value) => int_of(value).ok_or_else(|| {
            (
                "R_ARRAY_INDEX",
                format!("{value} is too large to be an index or extent"),
            )
        }),
        other => error("R_TYPE_INVALID", format!("expected Int, found {other}")),
    }
}

fn list_items(value: &Value) -> Result<Vec<Value>, ArrayError> {
    let Some(len) = value.list_len() else {
        return error("R_TYPE_INVALID", format!("expected a List, found {value}"));
    };
    Ok((0..len)
        .map(|index| value.list_item(index).expect("index is in range"))
        .collect())
}

/// Reads a shape. With `allow_infer`, one extent may be -1.
fn shape_arg(value: &Value, allow_infer: bool) -> Result<Vec<Option<usize>>, ArrayError> {
    let items = list_items(value)?;
    if items.len() > MAX_ARRAY_RANK {
        return error(
            "R_ARRAY_SHAPE",
            format!("an array may have at most {MAX_ARRAY_RANK} axes"),
        );
    }
    let mut inferred = false;
    items
        .iter()
        .map(|item| match item {
            Value::Int(extent) => match int_of(extent) {
                Some(-1) if allow_infer && !inferred => {
                    inferred = true;
                    Ok(None)
                }
                Some(extent) if extent >= 0 => usize::try_from(extent)
                    .map(Some)
                    .map_err(|_| ("R_ARRAY_SHAPE", format!("extent {extent} is too large"))),
                _ => error(
                    "R_ARRAY_SHAPE",
                    format!("extent {extent} must be a non-negative integer"),
                ),
            },
            other => error("R_TYPE_INVALID", format!("expected Int, found {other}")),
        })
        .collect()
}

fn fixed_shape(value: &Value) -> Result<Vec<usize>, ArrayError> {
    Ok(shape_arg(value, false)?
        .into_iter()
        .map(|extent| extent.expect("no inferred extent"))
        .collect())
}

fn size_of_shape(shape: &[usize]) -> Result<usize, ArrayError> {
    shape_size(shape).ok_or_else(|| ("R_ARRAY_SHAPE", format!("shape {shape:?} is too large")))
}

/// Allocates storage for `size` elements after charging for it.
fn alloc<T: Element>(
    charge: &mut Charge<'_>,
    rank: usize,
    size: usize,
) -> Result<Vec<T>, ArrayError> {
    charge(array_bytes(T::TYPE, rank, size))?;
    Ok(Vec::with_capacity(size))
}

fn flat_index(array: &ArrayValue, index: i128) -> Result<usize, ArrayError> {
    usize::try_from(index)
        .ok()
        .filter(|index| *index < array.size())
        .ok_or_else(|| {
            (
                "R_ARRAY_INDEX",
                format!(
                    "index {index} is outside an array of {} elements",
                    array.size()
                ),
            )
        })
}

fn nd_index(array: &ArrayValue, index: &Value) -> Result<usize, ArrayError> {
    let items = list_items(index)?;
    if items.len() != array.shape.len() {
        return error(
            "R_ARRAY_INDEX",
            format!(
                "an index for a rank-{} array needs {} positions, got {}",
                array.shape.len(),
                array.shape.len(),
                items.len()
            ),
        );
    }
    let mut flat = 0usize;
    for (axis, (item, extent)) in items.iter().zip(&array.shape).enumerate() {
        let position = match item {
            Value::Int(value) => int_of(value),
            _ => None,
        }
        .and_then(|position| usize::try_from(position).ok())
        .filter(|position| position < extent)
        .ok_or_else(|| {
            (
                "R_ARRAY_INDEX",
                format!("index {item} is outside axis {axis} of extent {extent}"),
            )
        })?;
        flat = flat * extent + position;
    }
    Ok(flat)
}

/// Stores a scalar at a flat position, updating the array in place when it is not shared.
pub(crate) fn set_flat(
    array: &mut Shared<ArrayValue>,
    position: usize,
    value: &Value,
) -> Result<(), ArrayError> {
    let element = array.element();
    with_element!(element, T => {
        let converted = T::from_value(value)?;
        T::items_mut(&mut Shared::make_mut(array).data)[position] = converted;
    });
    Ok(())
}

/// `array_set` on a local: the update the compiler emits for `set a = array_set(a, i, v)`.
pub fn set_local(local: &mut Value, index: &Value, value: &Value) -> Result<(), ArrayError> {
    let Value::Array(array) = local else {
        return error(
            "R_TYPE_INVALID",
            "array_set assignment target must be an array",
        );
    };
    let index = match index {
        Value::Int(index) => int_of(index).unwrap_or(i128::MAX),
        _ => return error("R_TYPE_INVALID", "array index must be Int"),
    };
    let position = flat_index(array, index)?;
    set_flat(array, position, value)
}

fn element_at(array: &ArrayValue, position: usize) -> Value {
    with_element!(array.element(), T => T::items(&array.data)[position].to_value())
}

fn from_list<T: Element>(items: &Value, charge: &mut Charge<'_>) -> Result<Value, ArrayError> {
    let len = items
        .list_len()
        .ok_or_else(|| ("R_TYPE_INVALID", "expected a List".to_string()))?;
    let mut data = alloc::<T>(charge, 1, len)?;
    if let Value::Float64List(floats) = items {
        for (index, value) in floats.iter().enumerate() {
            data.push(T::from_float(value.get()).ok_or_else(|| {
                (
                    "R_ARRAY_CONVERSION",
                    format!(
                        "item {index} ({}) does not fit in {}",
                        value.get(),
                        T::TYPE.name()
                    ),
                )
            })?);
        }
    } else if let Value::List(values) = items {
        for (index, value) in values.iter().enumerate() {
            data.push(
                T::from_value(value)
                    .map_err(|(code, message)| (code, format!("item {index}: {message}")))?,
            );
        }
    }
    new_array(vec![len], T::wrap(data))
}

fn full<T: Element>(
    shape: Vec<usize>,
    value: T,
    charge: &mut Charge<'_>,
) -> Result<Value, ArrayError> {
    let size = size_of_shape(&shape)?;
    charge(array_bytes(T::TYPE, shape.len(), size))?;
    new_array(shape, T::wrap(vec![value; size]))
}

fn convert<S: Element, D: Element>(
    array: &ArrayValue,
    charge: &mut Charge<'_>,
) -> Result<Value, ArrayError> {
    let source = S::items(&array.data);
    let mut data = alloc::<D>(charge, array.shape.len(), source.len())?;
    for (index, item) in source.iter().enumerate() {
        let converted = match item.to_int() {
            Some(int) => D::from_int(int),
            None => D::from_float(item.to_f64()),
        };
        data.push(converted.ok_or_else(|| {
            (
                "R_ARRAY_CONVERSION",
                format!(
                    "element {index} ({}) does not fit in {}",
                    item.to_value(),
                    D::TYPE.name()
                ),
            )
        })?);
    }
    new_array(array.shape.clone(), D::wrap(data))
}

fn from_bytes<T: Element>(bytes: &[u8], charge: &mut Charge<'_>) -> Result<Value, ArrayError> {
    let width = T::TYPE.width();
    if !bytes.len().is_multiple_of(width) {
        return error(
            "R_ARRAY_SHAPE",
            format!(
                "{} bytes is not a whole number of {}-byte {} elements",
                bytes.len(),
                width,
                T::TYPE.name()
            ),
        );
    }
    let mut data = alloc::<T>(charge, 1, bytes.len() / width)?;
    for chunk in bytes.chunks_exact(width) {
        data.push(T::read_le(chunk)?);
    }
    new_array(vec![data.len()], T::wrap(data))
}

fn to_bytes(array: &ArrayValue, charge: &mut Charge<'_>) -> Result<Value, ArrayError> {
    let len = array.data.byte_len();
    charge(std::mem::size_of::<Vec<u8>>().saturating_add(len))?;
    let mut out = Vec::with_capacity(len);
    with_element!(array.element(), T => {
        for item in T::items(&array.data) {
            item.write_le(&mut out);
        }
    });
    Ok(Value::Bytes(Shared::new(out)))
}

fn to_list(array: &ArrayValue, charge: &mut Charge<'_>) -> Result<Value, ArrayError> {
    let size = array.size();
    if let ArrayData::F64(items) = &array.data {
        charge(size.saturating_mul(std::mem::size_of::<Float64>()))?;
        return Ok(Value::Float64List(Shared::new(
            items
                .iter()
                .map(|item| Float64::new(*item).expect("array floats are finite"))
                .collect(),
        )));
    }
    charge(size.saturating_mul(std::mem::size_of::<Value>()))?;
    let items = with_element!(array.element(), T => {
        T::items(&array.data).iter().map(|item| item.to_value()).collect::<Vec<_>>()
    });
    Ok(Value::from_list_items(items))
}

fn reshape(array: &Shared<ArrayValue>, shape: &Value) -> Result<Value, ArrayError> {
    let requested = shape_arg(shape, true)?;
    let known = requested
        .iter()
        .flatten()
        .try_fold(1usize, |total, extent| total.checked_mul(*extent))
        .ok_or_else(|| ("R_ARRAY_SHAPE", "shape is too large".to_string()))?;
    let size = array.size();
    let shape: Vec<usize> = if requested.iter().any(Option::is_none) {
        if known == 0 || !size.is_multiple_of(known) {
            return error(
                "R_ARRAY_SHAPE",
                format!("cannot infer an extent to reshape {size} elements"),
            );
        }
        requested
            .iter()
            .map(|extent| extent.unwrap_or(size / known))
            .collect()
    } else {
        requested.into_iter().flatten().collect()
    };
    if size_of_shape(&shape)? != size {
        return error(
            "R_ARRAY_SHAPE",
            format!("cannot reshape {size} elements to {shape:?}"),
        );
    }
    let mut result = ArrayValue::clone(array);
    result.shape = shape;
    Ok(Value::Array(Shared::new(result)))
}

fn transpose<T: Element>(array: &ArrayValue, charge: &mut Charge<'_>) -> Result<Value, ArrayError> {
    let source = T::items(&array.data);
    let shape = &array.shape;
    let rank = shape.len();
    let out_shape: Vec<usize> = shape.iter().rev().copied().collect();
    let mut data = alloc::<T>(charge, rank, source.len())?;
    if rank <= 1 {
        data.extend_from_slice(source);
        return new_array(out_shape, T::wrap(data));
    }
    // Source strides read in output-axis order.
    let mut strides = vec![1usize; rank];
    for axis in (0..rank - 1).rev() {
        strides[axis] = strides[axis + 1] * shape[axis + 1];
    }
    let out_strides: Vec<usize> = strides.iter().rev().copied().collect();
    let mut counter = vec![0usize; rank];
    let mut offset = 0usize;
    for _ in 0..source.len() {
        data.push(source[offset]);
        for axis in (0..rank).rev() {
            counter[axis] += 1;
            offset += out_strides[axis];
            if counter[axis] < out_shape[axis] {
                break;
            }
            offset -= out_strides[axis] * out_shape[axis];
            counter[axis] = 0;
        }
    }
    new_array(out_shape, T::wrap(data))
}

fn slice<T: Element>(
    array: &ArrayValue,
    start: i128,
    end: i128,
    charge: &mut Charge<'_>,
) -> Result<Value, ArrayError> {
    let Some((&extent, rest)) = array.shape.split_first() else {
        return error("R_ARRAY_SHAPE", "a rank-0 array cannot be sliced");
    };
    let valid = 0 <= start && start <= end && end <= extent as i128;
    if !valid {
        return error(
            "R_ARRAY_INDEX",
            format!("slice {start}..{end} is outside axis 0 of extent {extent}"),
        );
    }
    let stride = size_of_shape(rest)?;
    let (start, end) = (start as usize, end as usize);
    let mut data = alloc::<T>(charge, array.shape.len(), (end - start) * stride)?;
    data.extend_from_slice(&T::items(&array.data)[start * stride..end * stride]);
    let mut shape = array.shape.clone();
    shape[0] = end - start;
    new_array(shape, T::wrap(data))
}

fn join<T: Element>(
    parts: &[Value],
    stack: bool,
    charge: &mut Charge<'_>,
) -> Result<Value, ArrayError> {
    let arrays: Vec<&ArrayValue> = parts
        .iter()
        .map(|part| match part {
            Value::Array(array) => Ok(array.as_ref()),
            other => error(
                "R_TYPE_INVALID",
                format!("expected an array, found {other}"),
            ),
        })
        .collect::<Result<_, _>>()?;
    let Some(first) = arrays.first() else {
        return error("R_ARRAY_SHAPE", "joining needs at least one array");
    };
    let inner: &[usize] = if stack {
        &first.shape
    } else if let Some((_, rest)) = first.shape.split_first() {
        rest
    } else {
        return error("R_ARRAY_SHAPE", "rank-0 arrays cannot be concatenated");
    };
    let mut leading = 0usize;
    for array in &arrays {
        let shape_matches = if stack {
            array.shape == first.shape
        } else {
            array.shape.len() == first.shape.len() && array.shape[1..] == *inner
        };
        if !shape_matches {
            return error(
                "R_ARRAY_SHAPE",
                format!(
                    "cannot join arrays of shapes {:?} and {:?}",
                    first.shape, array.shape
                ),
            );
        }
        leading += if stack { 1 } else { array.shape[0] };
    }
    let mut shape = vec![leading];
    shape.extend_from_slice(inner);
    if shape.len() > MAX_ARRAY_RANK {
        return error("R_ARRAY_SHAPE", "stacking would exceed the rank limit");
    }
    let size = size_of_shape(&shape)?;
    let mut data = alloc::<T>(charge, shape.len(), size)?;
    for array in &arrays {
        data.extend_from_slice(T::items(&array.data));
    }
    new_array(shape, T::wrap(data))
}

/// Runs one array instruction. `args` holds the operands in push order; a kernel may take
/// ownership of an operand to update it in place.
pub fn apply(
    function: ArrayFunction,
    element: ElementType,
    args: &mut [Value],
    charge: &mut Charge<'_>,
) -> Result<Value, ArrayError> {
    use ArrayFunction::*;
    match function {
        FromList => with_element!(element, T => from_list::<T>(&args[0], charge)),
        Full => {
            let shape = fixed_shape(&args[0])?;
            with_element!(element, T => full::<T>(shape, T::from_value(&args[1])?, charge))
        }
        Zeros => {
            let shape = fixed_shape(&args[0])?;
            with_element!(element, T => full::<T>(shape, T::default(), charge))
        }
        Convert => {
            let array = array_arg(args, 0)?;
            with_element!(array.element(), S => with_element!(element, D => convert::<S, D>(array, charge)))
        }
        FromBytes => match &args[0] {
            Value::Bytes(bytes) => with_element!(element, T => from_bytes::<T>(bytes, charge)),
            other => error("R_TYPE_INVALID", format!("expected Bytes, found {other}")),
        },
        Shape => {
            let array = array_arg(args, 0)?;
            Ok(Value::from_list_items(
                array
                    .shape
                    .iter()
                    .map(|extent| Value::Int(BigInt::from_usize(*extent)))
                    .collect(),
            ))
        }
        Ndim => Ok(Value::Int(BigInt::from_usize(
            array_arg(args, 0)?.shape.len(),
        ))),
        Size => Ok(Value::Int(BigInt::from_usize(array_arg(args, 0)?.size()))),
        Equal => Ok(Value::Bool(array_arg(args, 0)? == array_arg(args, 1)?)),
        Get => {
            let array = array_arg(args, 0)?;
            let position = flat_index(array, int_arg(args, 1).unwrap_or(i128::MAX))?;
            Ok(element_at(array, position))
        }
        GetAt => {
            let array = array_arg(args, 0)?;
            let position = nd_index(array, &args[1])?;
            Ok(element_at(array, position))
        }
        Set | SetAt => {
            let mut array = match take(args, 0) {
                Value::Array(array) => array,
                other => {
                    return error(
                        "R_TYPE_INVALID",
                        format!("expected an array, found {other}"),
                    )
                }
            };
            let position = if function == Set {
                flat_index(&array, int_arg(args, 1).unwrap_or(i128::MAX))?
            } else {
                nd_index(&array, &args[1])?
            };
            if Shared::strong_count(&array) > 1 {
                charge(array_bytes(element, array.shape.len(), array.size()))?;
            }
            set_flat(&mut array, position, &args[2])?;
            Ok(Value::Array(array))
        }
        Reshape => reshape(array_arg(args, 0)?, &args[1]),
        Flatten => {
            let array = array_arg(args, 0)?;
            let mut result = ArrayValue::clone(array);
            result.shape = vec![array.size()];
            Ok(Value::Array(Shared::new(result)))
        }
        Transpose => {
            let array = array_arg(args, 0)?;
            with_element!(element, T => transpose::<T>(array, charge))
        }
        Slice => {
            let array = array_arg(args, 0)?;
            let start = int_arg(args, 1).unwrap_or(i128::MAX);
            let end = int_arg(args, 2).unwrap_or(i128::MAX);
            with_element!(element, T => slice::<T>(array, start, end, charge))
        }
        Concat | Stack => {
            let parts = list_items(&args[0])?;
            with_element!(element, T => join::<T>(&parts, function == Stack, charge))
        }
        ToList => to_list(array_arg(args, 0)?, charge),
        ToBytes => to_bytes(array_arg(args, 0)?, charge),
        _ => crate::array_compute::apply(function, element, args, charge),
    }
}
