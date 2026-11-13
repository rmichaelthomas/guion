use guion_model::{sort_order_f64, Artifact, Function, Instruction, Op, Span, Type};
use std::cmp::Ordering;

fn artifact_with(code: Vec<Op>) -> Artifact {
    Artifact {
        format_version: Artifact::CURRENT_VERSION,
        build_metadata: None,
        compiled_source_sha256: None,
        source_name: "sort.gu".into(),
        sources: vec!["sort.gu".into()],
        entry: "main".into(),
        functions: vec![Function {
            name: "main".into(),
            parameters: vec![],
            result: Type::Unit,
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
fn float64_sort_order_is_total_with_nan_last_and_negative_zero_first() {
    let ordered = [
        f64::NEG_INFINITY,
        -1.5,
        -0.0,
        0.0,
        1.5,
        f64::INFINITY,
        f64::NAN,
    ];
    for (left_index, left) in ordered.iter().enumerate() {
        for (right_index, right) in ordered.iter().enumerate() {
            assert_eq!(
                sort_order_f64(*left, *right),
                left_index.cmp(&right_index),
                "{left} vs {right}"
            );
        }
    }
    // Every NaN, whatever its sign or payload, sorts after every number and equal to each other.
    let negative_nan = f64::from_bits(f64::NAN.to_bits() | (1 << 63));
    assert!(negative_nan.is_nan());
    assert_eq!(
        sort_order_f64(negative_nan, f64::INFINITY),
        Ordering::Greater
    );
    assert_eq!(sort_order_f64(f64::INFINITY, negative_nan), Ordering::Less);
    assert_eq!(sort_order_f64(negative_nan, f64::NAN), Ordering::Equal);
    assert_eq!(sort_order_f64(-0.0, 0.0), Ordering::Less);
}

#[test]
fn float64_sort_order_sorts_a_shuffled_list_without_panicking() {
    let mut values = vec![
        f64::NAN,
        0.0,
        -0.0,
        3.0,
        f64::NAN,
        -3.0,
        0.0,
        -0.0,
        f64::INFINITY,
    ];
    values.sort_by(|left, right| sort_order_f64(*left, *right));
    let bits: Vec<u64> = values.iter().map(|value| value.to_bits()).collect();
    assert_eq!(values[0], -3.0);
    assert_eq!(bits[1], (-0.0f64).to_bits());
    assert_eq!(bits[2], (-0.0f64).to_bits());
    assert_eq!(bits[3], 0.0f64.to_bits());
    assert_eq!(bits[4], 0.0f64.to_bits());
    assert_eq!(values[5], 3.0);
    assert_eq!(values[6], f64::INFINITY);
    assert!(values[7].is_nan() && values[8].is_nan());
}

#[test]
fn the_current_format_is_at_least_65() {
    assert!(Artifact::CURRENT_VERSION >= 65);
}

#[test]
fn format_64_rejects_sort_natural_and_sort_by_key() {
    for op in [Op::SortNatural, Op::SortByKey(0)] {
        let mut artifact = artifact_with(vec![op]);
        let bytes = artifact.to_bytes().unwrap();
        assert!(Artifact::from_bytes(&bytes).is_ok());
        artifact.format_version = 64;
        assert_eq!(artifact.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");
        let mut legacy = bytes;
        legacy[4..8].copy_from_slice(&64u32.to_le_bytes());
        assert_eq!(
            Artifact::from_bytes(&legacy).unwrap_err().code,
            "E_ARTIFACT_VERSION"
        );
    }
    // Operations that existed at format 64 are still written and read at 64.
    let mut older = artifact_with(vec![Op::SortBy(0)]);
    older.format_version = 64;
    assert!(older.to_bytes().is_ok());
}
