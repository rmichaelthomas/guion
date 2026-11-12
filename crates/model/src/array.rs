//! Native arrays: fixed-width element types, n-dimensional row-major storage, and the
//! `Op::Array` instruction family. Element and function codes are part of artifact format 62
//! and must never be reordered.

use std::fmt;

use crate::{ShareAcrossThreads, Type};

/// Largest number of axes an array may have.
pub const MAX_ARRAY_RANK: usize = 32;

/// The element type of an array. These types exist only as array element types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ElementType {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    Bool,
}

impl ElementType {
    pub const ALL: [ElementType; 11] = [
        ElementType::I8,
        ElementType::I16,
        ElementType::I32,
        ElementType::I64,
        ElementType::U8,
        ElementType::U16,
        ElementType::U32,
        ElementType::U64,
        ElementType::F32,
        ElementType::F64,
        ElementType::Bool,
    ];

    pub fn code(self) -> u8 {
        Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .expect("every element type is listed") as u8
    }

    pub fn from_code(code: u8) -> Option<Self> {
        Self::ALL.get(usize::from(code)).copied()
    }

    pub fn name(self) -> &'static str {
        match self {
            ElementType::I8 => "i8",
            ElementType::I16 => "i16",
            ElementType::I32 => "i32",
            ElementType::I64 => "i64",
            ElementType::U8 => "u8",
            ElementType::U16 => "u16",
            ElementType::U32 => "u32",
            ElementType::U64 => "u64",
            ElementType::F32 => "f32",
            ElementType::F64 => "f64",
            ElementType::Bool => "bool",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|candidate| candidate.name() == name)
    }

    /// Bytes per element in storage and in `array_to_bytes`.
    pub fn width(self) -> usize {
        match self {
            ElementType::I8 | ElementType::U8 | ElementType::Bool => 1,
            ElementType::I16 | ElementType::U16 => 2,
            ElementType::I32 | ElementType::U32 | ElementType::F32 => 4,
            ElementType::I64 | ElementType::U64 | ElementType::F64 => 8,
        }
    }

    pub fn is_numeric(self) -> bool {
        self != ElementType::Bool
    }

    pub fn is_float(self) -> bool {
        matches!(self, ElementType::F32 | ElementType::F64)
    }

    pub fn is_integer(self) -> bool {
        self.is_numeric() && !self.is_float()
    }

    /// The scalar type read from and written to an element.
    pub fn scalar_type(self) -> Type {
        if self.is_float() {
            Type::Float64
        } else if self == ElementType::Bool {
            Type::Bool
        } else {
            Type::Int
        }
    }
}

/// Contiguous element storage, one vector per element type.
#[derive(Clone, Debug, PartialEq)]
pub enum ArrayData {
    I8(Vec<i8>),
    I16(Vec<i16>),
    I32(Vec<i32>),
    I64(Vec<i64>),
    U8(Vec<u8>),
    U16(Vec<u16>),
    U32(Vec<u32>),
    U64(Vec<u64>),
    F32(Vec<f32>),
    F64(Vec<f64>),
    Bool(Vec<bool>),
}

/// Runs `$body` with `$items` bound to the storage vector, whatever its element type.
#[macro_export]
macro_rules! with_array_data {
    ($data:expr, $items:ident => $body:expr) => {
        match $data {
            $crate::ArrayData::I8($items) => $body,
            $crate::ArrayData::I16($items) => $body,
            $crate::ArrayData::I32($items) => $body,
            $crate::ArrayData::I64($items) => $body,
            $crate::ArrayData::U8($items) => $body,
            $crate::ArrayData::U16($items) => $body,
            $crate::ArrayData::U32($items) => $body,
            $crate::ArrayData::U64($items) => $body,
            $crate::ArrayData::F32($items) => $body,
            $crate::ArrayData::F64($items) => $body,
            $crate::ArrayData::Bool($items) => $body,
        }
    };
}

impl ArrayData {
    pub fn element(&self) -> ElementType {
        match self {
            ArrayData::I8(_) => ElementType::I8,
            ArrayData::I16(_) => ElementType::I16,
            ArrayData::I32(_) => ElementType::I32,
            ArrayData::I64(_) => ElementType::I64,
            ArrayData::U8(_) => ElementType::U8,
            ArrayData::U16(_) => ElementType::U16,
            ArrayData::U32(_) => ElementType::U32,
            ArrayData::U64(_) => ElementType::U64,
            ArrayData::F32(_) => ElementType::F32,
            ArrayData::F64(_) => ElementType::F64,
            ArrayData::Bool(_) => ElementType::Bool,
        }
    }

    pub fn len(&self) -> usize {
        with_array_data!(self, items => items.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn byte_len(&self) -> usize {
        self.len().saturating_mul(self.element().width())
    }

    /// Empty storage of the given element type.
    pub fn empty(element: ElementType) -> Self {
        Self::with_capacity(element, 0)
    }

    pub fn with_capacity(element: ElementType, capacity: usize) -> Self {
        match element {
            ElementType::I8 => ArrayData::I8(Vec::with_capacity(capacity)),
            ElementType::I16 => ArrayData::I16(Vec::with_capacity(capacity)),
            ElementType::I32 => ArrayData::I32(Vec::with_capacity(capacity)),
            ElementType::I64 => ArrayData::I64(Vec::with_capacity(capacity)),
            ElementType::U8 => ArrayData::U8(Vec::with_capacity(capacity)),
            ElementType::U16 => ArrayData::U16(Vec::with_capacity(capacity)),
            ElementType::U32 => ArrayData::U32(Vec::with_capacity(capacity)),
            ElementType::U64 => ArrayData::U64(Vec::with_capacity(capacity)),
            ElementType::F32 => ArrayData::F32(Vec::with_capacity(capacity)),
            ElementType::F64 => ArrayData::F64(Vec::with_capacity(capacity)),
            ElementType::Bool => ArrayData::Bool(Vec::with_capacity(capacity)),
        }
    }
}

/// An n-dimensional, row-major array value.
#[derive(Clone, Debug, PartialEq)]
pub struct ArrayValue {
    pub shape: Vec<usize>,
    pub data: ArrayData,
}

/// The element count of a shape, or `None` when it overflows.
pub fn shape_size(shape: &[usize]) -> Option<usize> {
    shape
        .iter()
        .try_fold(1usize, |total, extent| total.checked_mul(*extent))
}

impl ArrayValue {
    pub fn new(shape: Vec<usize>, data: ArrayData) -> Result<Self, (&'static str, String)> {
        if shape.len() > MAX_ARRAY_RANK {
            return Err((
                "R_ARRAY_SHAPE",
                format!(
                    "an array may have at most {MAX_ARRAY_RANK} axes, got {}",
                    shape.len()
                ),
            ));
        }
        match shape_size(&shape) {
            Some(size) if size == data.len() => Ok(Self { shape, data }),
            Some(size) => Err((
                "R_ARRAY_SHAPE",
                format!(
                    "shape {shape:?} holds {size} elements but {} were given",
                    data.len()
                ),
            )),
            None => Err(("R_ARRAY_SHAPE", format!("shape {shape:?} is too large"))),
        }
    }

    /// A rank-1 array over the given storage.
    pub fn vector(data: ArrayData) -> Self {
        Self {
            shape: vec![data.len()],
            data,
        }
    }

    pub fn size(&self) -> usize {
        self.data.len()
    }

    pub fn element(&self) -> ElementType {
        self.data.element()
    }
}

impl Eq for ArrayValue {}

impl ShareAcrossThreads for ArrayValue {
    // Arrays hold only plain numbers, so there are no nested counts to switch.
    fn share_across_threads(&self) {}
}

/// Arrays above this many elements are shown summarised.
const DISPLAY_SUMMARY_THRESHOLD: usize = 1000;
const DISPLAY_EDGE_ITEMS: usize = 3;

trait DisplayElement {
    fn write(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
}

macro_rules! display_plain {
    ($($ty:ty),*) => {$(
        impl DisplayElement for $ty {
            fn write(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{self}")
            }
        }
    )*};
}
display_plain!(i8, i16, i32, i64, u8, u16, u32, u64, f32, f64, bool);

fn write_axis<T: DisplayElement>(
    f: &mut fmt::Formatter<'_>,
    items: &[T],
    shape: &[usize],
    summarise: bool,
) -> fmt::Result {
    let Some((&extent, rest)) = shape.split_first() else {
        return items[0].write(f);
    };
    let stride = crate::array::shape_size(rest).unwrap_or(0);
    f.write_str("[")?;
    let skip = summarise && extent > 2 * DISPLAY_EDGE_ITEMS;
    let mut first = true;
    for index in 0..extent {
        if skip && index >= DISPLAY_EDGE_ITEMS && index < extent - DISPLAY_EDGE_ITEMS {
            if index == DISPLAY_EDGE_ITEMS {
                f.write_str(", ...")?;
            }
            continue;
        }
        if !first {
            f.write_str(", ")?;
        }
        first = false;
        write_axis(
            f,
            &items[index * stride..(index + 1) * stride],
            rest,
            summarise,
        )?;
    }
    f.write_str("]")
}

impl fmt::Display for ArrayValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let summarise = self.size() > DISPLAY_SUMMARY_THRESHOLD;
        with_array_data!(&self.data, items => write_axis(f, items, &self.shape, summarise))
    }
}

macro_rules! array_functions {
    ($($variant:ident),* $(,)?) => {
        /// The functions behind `Op::Array`. Codes are positions in `ALL`; never reorder.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum ArrayFunction {
            $($variant),*
        }

        impl ArrayFunction {
            pub const ALL: &'static [ArrayFunction] = &[$(ArrayFunction::$variant),*];

            /// The instruction name used by builtin catalogs and the textual bytecode form.
            pub fn op_name(self) -> &'static str {
                match self {
                    $(ArrayFunction::$variant => concat!("Array", stringify!($variant))),*
                }
            }
        }
    };
}

array_functions!(
    // Construction; the instruction's element type is the result's.
    FromList, Full, Zeros, Convert, FromBytes, Range, Linspace, Random, Normal,
    // Inspection.
    Shape, Ndim, Size, Equal, // Element access.
    Get, GetAt, Set, SetAt, // Shape and interchange.
    Reshape, Flatten, Transpose, Slice, Concat, Stack, ToList, ToBytes, // Arithmetic.
    Add, Sub, Mul, Div, Minimum, Maximum, AddScalar, SubScalar, MulScalar, DivScalar, Neg, Abs,
    Clip, // Float math.
    Sqrt, Exp, Log, Log10, Log2, Sin, Cos, Tan, Asin, Acos, Atan, Sinh, Cosh, Tanh, Floor, Ceil,
    Round, Pow, // Comparison.
    Eq, Ne, Lt, Le, Gt, Ge, EqScalar, NeScalar, LtScalar, LeScalar, GtScalar, GeScalar,
    // Logic and selection.
    And, Or, Not, Any, All, Count, Where, Filter, // Reductions.
    Sum, Prod, Min, Max, Argmin, Argmax, Mean, Var, Std, SumAxis, MinAxis, MaxAxis, MeanAxis,
    // Order and accumulation.
    Sort, Argsort, Cumsum, // Linear algebra.
    Dot, Matmul,
);

impl ArrayFunction {
    pub fn code(self) -> u8 {
        Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .expect("every array function is listed") as u8
    }

    pub fn from_code(code: u8) -> Option<Self> {
        Self::ALL.get(usize::from(code)).copied()
    }

    pub fn from_op_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|candidate| candidate.op_name() == name)
    }

    /// The textual instruction name, e.g. `ArrayAdd.f64`.
    pub fn instruction_name(self, element: ElementType) -> String {
        format!("{}.{}", self.op_name(), element.name())
    }

    /// Parses `ArrayAdd.f64` back into its function and element type.
    pub fn parse_instruction_name(name: &str) -> Option<(Self, ElementType)> {
        let (function, element) = name.split_once('.')?;
        Some((
            Self::from_op_name(function)?,
            ElementType::from_name(element)?,
        ))
    }
}

/// Which element types an array function accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElementClass {
    Any,
    Numeric,
    Float,
    Bool,
    Only(ElementType),
}

impl ElementClass {
    pub fn accepts(self, element: ElementType) -> bool {
        match self {
            ElementClass::Any => true,
            ElementClass::Numeric => element.is_numeric(),
            ElementClass::Float => element.is_float(),
            ElementClass::Bool => element == ElementType::Bool,
            ElementClass::Only(only) => element == only,
        }
    }

    /// The catalog's element variable for this class.
    pub fn variable(self) -> &'static str {
        match self {
            ElementClass::Any | ElementClass::Bool | ElementClass::Only(_) => "$A",
            ElementClass::Numeric => "$N",
            ElementClass::Float => "$F",
        }
    }
}

/// One operand of an array instruction, as the bytecode verifier checks it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArrayOperand {
    Exact(Type),
    /// An array of any element type (the source of `Convert`).
    AnyArray,
}

impl ArrayFunction {
    /// Element types this function accepts. For constructors this is the result's type.
    pub fn element_class(self) -> ElementClass {
        use ArrayFunction::*;
        match self {
            Range => ElementClass::Only(ElementType::I64),
            Linspace | Random | Normal => ElementClass::Only(ElementType::F64),
            FromList | Full | Zeros | Convert | FromBytes | Shape | Ndim | Size | Equal | Get
            | GetAt | Set | SetAt | Reshape | Flatten | Transpose | Slice | Concat | Stack
            | ToList | ToBytes | Eq | Ne | EqScalar | NeScalar => ElementClass::Any,
            Add | Sub | Mul | Div | Minimum | Maximum | AddScalar | SubScalar | MulScalar
            | DivScalar | Neg | Abs | Clip | Lt | Le | Gt | Ge | LtScalar | LeScalar | GtScalar
            | GeScalar | Sum | Prod | Min | Max | Argmin | Argmax | Mean | Var | Std | SumAxis
            | MinAxis | MaxAxis | MeanAxis | Sort | Argsort | Cumsum | Dot | Matmul => {
                ElementClass::Numeric
            }
            Sqrt | Exp | Log | Log10 | Log2 | Sin | Cos | Tan | Asin | Acos | Atan | Sinh
            | Cosh | Tanh | Floor | Ceil | Round | Pow => ElementClass::Float,
            And | Or | Not | Any | All | Count => ElementClass::Bool,
            Where | Filter => ElementClass::Any,
        }
    }

    /// Operand types (in push order) and the result type for an element type.
    pub fn signature(self, element: ElementType) -> (Vec<ArrayOperand>, Type) {
        use ArrayFunction::*;
        use ArrayOperand::{AnyArray, Exact};
        let array = Type::Array(element);
        let scalar = element.scalar_type();
        let a = || Exact(array.clone());
        let s = || Exact(scalar.clone());
        let int = || Exact(Type::Int);
        let shape = || Exact(Type::List(Box::new(Type::Int)));
        let mask = Type::Array(ElementType::Bool);
        match self {
            FromList => (vec![Exact(Type::List(Box::new(scalar.clone())))], array),
            Full => (vec![shape(), s()], array),
            Zeros => (vec![shape()], array),
            Convert => (vec![AnyArray], array),
            FromBytes => (vec![Exact(Type::Bytes)], array),
            Range => (vec![int(), int(), int()], array),
            Linspace => (
                vec![Exact(Type::Float64), Exact(Type::Float64), int()],
                array,
            ),
            Random | Normal => (vec![int(), shape()], array),
            Shape => (vec![a()], Type::List(Box::new(Type::Int))),
            Ndim | Size | Count | Argmin | Argmax => (vec![a()], Type::Int),
            Equal => (vec![a(), a()], Type::Bool),
            Get => (vec![a(), int()], scalar),
            GetAt => (vec![a(), shape()], scalar),
            Set => (vec![a(), int(), s()], array),
            SetAt => (vec![a(), shape(), s()], array),
            Reshape => (vec![a(), shape()], array),
            Flatten | Transpose | Neg | Abs | Not | Sort | Cumsum | Sqrt | Exp | Log | Log10
            | Log2 | Sin | Cos | Tan | Asin | Acos | Atan | Sinh | Cosh | Tanh | Floor | Ceil
            | Round => (vec![a()], array),
            Slice => (vec![a(), int(), int()], array),
            Concat | Stack => (vec![Exact(Type::List(Box::new(array.clone())))], array),
            ToList => (vec![a()], Type::List(Box::new(scalar))),
            ToBytes => (vec![a()], Type::Bytes),
            Add | Sub | Mul | Div | Minimum | Maximum | And | Or | Matmul => {
                (vec![a(), a()], array)
            }
            AddScalar | SubScalar | MulScalar | DivScalar => (vec![a(), s()], array),
            Clip => (vec![a(), s(), s()], array),
            Pow => (vec![a(), Exact(Type::Float64)], array),
            Eq | Ne | Lt | Le | Gt | Ge => (vec![a(), a()], mask),
            EqScalar | NeScalar | LtScalar | LeScalar | GtScalar | GeScalar => {
                (vec![a(), s()], mask)
            }
            Any | All => (vec![a()], Type::Bool),
            Where => (vec![Exact(mask), a(), a()], array),
            Filter => (vec![a(), Exact(mask)], array),
            Dot => (vec![a(), a()], scalar),
            Sum | Prod | Min | Max => (vec![a()], scalar),
            Mean | Var | Std => (vec![a()], Type::Float64),
            SumAxis | MinAxis | MaxAxis => (vec![a(), int()], array),
            MeanAxis => (vec![a(), int()], Type::Array(ElementType::F64)),
            Argsort => (vec![a()], Type::Array(ElementType::I64)),
        }
    }

    /// Number of operands the instruction pops.
    pub fn arity(self) -> usize {
        static ARITIES: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
        let arities = ARITIES.get_or_init(|| {
            ArrayFunction::ALL
                .iter()
                .map(|function| {
                    let element = ElementType::ALL
                        .into_iter()
                        .find(|element| function.element_class().accepts(*element))
                        .expect("every function accepts some element type");
                    function.signature(element).0.len() as u8
                })
                .collect()
        });
        usize::from(arities[usize::from(self.code())])
    }

    /// Whether the function builds a new array of its element type from non-array inputs or
    /// converts into it; the catalog names these per element type (`array_f64_full`).
    pub fn is_constructor(self) -> bool {
        use ArrayFunction::*;
        matches!(
            self,
            FromList | Full | Zeros | Convert | FromBytes | Range | Linspace | Random | Normal
        )
    }
}
