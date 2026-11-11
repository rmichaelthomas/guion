use guion_model::{
    ArrayData, ArrayFunction, ArrayValue, Artifact, ElementType, Function, Instruction, Op, Shared,
    Span, Type, Value,
};

fn artifact_with(code: Vec<Op>, result: Type, parameters: Vec<Type>) -> Artifact {
    Artifact {
        format_version: Artifact::CURRENT_VERSION,
        build_metadata: None,
        compiled_source_sha256: None,
        source_name: "arrays.gu".into(),
        sources: vec!["arrays.gu".into()],
        entry: "main".into(),
        functions: vec![Function {
            name: "main".into(),
            parameters,
            result,
            local_count: 0,
            code: code
                .into_iter()
                .chain([Op::Return])
                .map(|op| Instruction {
                    op,
                    span: Span::default(),
                })
                .collect(),
        }],
        externals: vec![],
        records: vec![],
    }
}

#[test]
fn element_codes_are_stable() {
    let expected = [
        ("i8", 0),
        ("i16", 1),
        ("i32", 2),
        ("i64", 3),
        ("u8", 4),
        ("u16", 5),
        ("u32", 6),
        ("u64", 7),
        ("f32", 8),
        ("f64", 9),
        ("bool", 10),
    ];
    assert_eq!(ElementType::ALL.len(), expected.len());
    for (element, (name, code)) in ElementType::ALL.iter().zip(expected) {
        assert_eq!(element.name(), name);
        assert_eq!(element.code(), code);
        assert_eq!(ElementType::from_code(code), Some(*element));
        assert_eq!(ElementType::from_name(name), Some(*element));
    }
    assert_eq!(ElementType::from_code(11), None);
    assert_eq!(ElementType::from_name("f16"), None);
    assert_eq!(ElementType::U16.scalar_type(), Type::Int);
    assert_eq!(ElementType::F32.scalar_type(), Type::Float64);
    assert_eq!(ElementType::Bool.scalar_type(), Type::Bool);
    assert!(!ElementType::Bool.is_numeric());
    assert!(ElementType::F32.is_float() && !ElementType::I64.is_float());
    assert_eq!(ElementType::U32.width(), 4);
}

#[test]
fn array_value_rejects_bad_shape() {
    let error = ArrayValue::new(vec![2, 3], ArrayData::F64(vec![0.0; 5])).unwrap_err();
    assert_eq!(error.0, "R_ARRAY_SHAPE");
    let error = ArrayValue::new(vec![1; 33], ArrayData::F64(vec![0.0])).unwrap_err();
    assert_eq!(error.0, "R_ARRAY_SHAPE");
    let value = ArrayValue::new(vec![2, 3], ArrayData::I32(vec![0; 6])).unwrap();
    assert_eq!(value.size(), 6);
    assert_eq!(value.element(), ElementType::I32);
}

#[test]
fn array_type_round_trips() {
    let artifact = artifact_with(
        vec![Op::Load(0)],
        Type::Array(ElementType::F32),
        vec![Type::Array(ElementType::F32)],
    );
    let bytes = artifact.to_bytes().unwrap();
    assert_eq!(Artifact::from_bytes(&bytes).unwrap(), artifact);
}

#[test]
fn array_op_round_trips() {
    let code: Vec<Op> = ArrayFunction::ALL
        .iter()
        .flat_map(|function| {
            ElementType::ALL
                .iter()
                .map(move |element| Op::Array(*function, *element))
        })
        .collect();
    let artifact = artifact_with(code, Type::Unit, vec![]);
    let bytes = artifact.to_bytes().unwrap();
    assert_eq!(Artifact::from_bytes(&bytes).unwrap(), artifact);
    for &function in ArrayFunction::ALL {
        assert_eq!(ArrayFunction::from_code(function.code()), Some(function));
        assert_eq!(
            ArrayFunction::from_op_name(function.op_name()),
            Some(function)
        );
        assert!(function.op_name().starts_with("Array"));
    }
}

#[test]
fn format_61_rejects_arrays() {
    let mut artifact = artifact_with(
        vec![Op::Array(ArrayFunction::Size, ElementType::F64)],
        Type::Unit,
        vec![],
    );
    let bytes = artifact.to_bytes().unwrap();
    artifact.format_version = 61;
    assert_eq!(artifact.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");
    let mut legacy = bytes;
    // The format version follows the four-byte magic.
    legacy[4..8].copy_from_slice(&61u32.to_le_bytes());
    assert_eq!(
        Artifact::from_bytes(&legacy).unwrap_err().code,
        "E_ARTIFACT_VERSION"
    );
    let typed = artifact_with(vec![], Type::Unit, vec![Type::Array(ElementType::U8)]);
    let mut typed_legacy = typed;
    typed_legacy.format_version = 61;
    assert_eq!(
        typed_legacy.to_bytes().unwrap_err().code,
        "E_ARTIFACT_VERSION"
    );
}

fn array(shape: Vec<usize>, data: ArrayData) -> Value {
    Value::Array(Shared::new(ArrayValue::new(shape, data).unwrap()))
}

#[test]
fn array_display() {
    assert_eq!(
        array(vec![2, 2], ArrayData::I32(vec![1, 2, 3, 4])).to_string(),
        "[[1, 2], [3, 4]]"
    );
    assert_eq!(array(vec![], ArrayData::F64(vec![2.5])).to_string(), "2.5");
    assert_eq!(
        array(vec![1], ArrayData::F32(vec![0.1])).to_string(),
        "[0.1]"
    );
    assert_eq!(
        array(vec![3], ArrayData::Bool(vec![true, false, true])).to_string(),
        "[true, false, true]"
    );
    assert_eq!(
        array(vec![2, 0], ArrayData::U8(vec![])).to_string(),
        "[[], []]"
    );
    let long = array(vec![2000], ArrayData::I64((0..2000).collect()));
    assert_eq!(long.to_string(), "[0, 1, 2, ..., 1997, 1998, 1999]");
}

#[test]
fn array_equality_compares_element_shape_and_contents() {
    let a = array(vec![2], ArrayData::I32(vec![1, 2]));
    assert_eq!(a, array(vec![2], ArrayData::I32(vec![1, 2])));
    assert_ne!(a, array(vec![2], ArrayData::I64(vec![1, 2])));
    assert_ne!(a, array(vec![2, 1], ArrayData::I32(vec![1, 2])));
    assert_ne!(a, array(vec![2], ArrayData::I32(vec![1, 3])));
}
