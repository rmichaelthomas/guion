use guion_compiler::compile;
use guion_model::{ArrayFunction, ElementType, Op};

fn compile_main(body: &str) -> Result<guion_model::Artifact, guion_model::Diagnostic> {
    compile("arrays.gu", &format!("fn main() -> Unit:\n{body}"))
}

fn error_code(source: &str) -> String {
    compile("arrays.gu", source)
        .expect_err("source must not compile")
        .code
}

#[test]
fn array_type_parses() {
    compile(
        "arrays.gu",
        "fn size(a: Array[f64]) -> Int:\n    return array_size(a)\n\nfn main() -> Unit:\n    print(size(array_f64([1.0, 2.0])))\n",
    )
    .unwrap();
}

#[test]
fn bare_tag_rejected() {
    assert_eq!(
        error_code("fn f(x: f64) -> Int:\n    return 1\n\nfn main() -> Unit:\n    print(1)\n"),
        "E_TYPE_UNKNOWN"
    );
    assert_eq!(
        error_code(
            "fn f(x: List[i32]) -> Int:\n    return 1\n\nfn main() -> Unit:\n    print(1)\n"
        ),
        "E_TYPE_UNKNOWN"
    );
    assert_eq!(
        error_code(
            "fn f(x: Array[f16]) -> Int:\n    return 1\n\nfn main() -> Unit:\n    print(1)\n"
        ),
        "E_TYPE_UNKNOWN"
    );
}

#[test]
fn generic_binds_and_returns_scalar() {
    compile_main("    let a = array_i32([1, 2])\n    let x: Int = array_get(a, 0) + 1\n    let f: Float64 = array_get(array_f32([1.5]), 0) + 1.0\n    let b: Bool = array_get(array_bool([true]), 0)\n    print(x)\n    print(f)\n    print(b)\n").unwrap();
    assert_eq!(
        compile_main("    let x: Float64 = array_get(array_i32([1]), 0)\n    print(x)\n")
            .unwrap_err()
            .code,
        "E_TYPE_ANNOTATION"
    );
}

#[test]
fn numeric_constraint() {
    let error =
        compile_main("    let a = array_bool([true])\n    print(array_add(a, a))\n").unwrap_err();
    assert_eq!(error.code, "E_TYPE_ARGUMENT");
    assert!(error.message.contains("numeric"), "{}", error.message);
}

#[test]
fn float_constraint() {
    assert_eq!(
        compile_main("    print(array_sqrt(array_i64([4])))\n")
            .unwrap_err()
            .code,
        "E_TYPE_ARGUMENT"
    );
}

#[test]
fn tag_mismatch() {
    assert_eq!(
        compile_main("    print(array_add(array_f64([1.0]), array_f32([1.0])))\n")
            .unwrap_err()
            .code,
        "E_TYPE_ARGUMENT"
    );
}

#[test]
fn elem_parameter_checked() {
    assert_eq!(
        compile_main("    print(array_set(array_f64([1.0]), 0, 1))\n")
            .unwrap_err()
            .code,
        "E_TYPE_ARGUMENT"
    );
}

#[test]
fn non_array_first_argument_rejected() {
    assert_eq!(
        compile_main("    print(array_size([1, 2]))\n")
            .unwrap_err()
            .code,
        "E_TYPE_ARGUMENT"
    );
}

#[test]
fn emits_bound_element() {
    let artifact = compile_main(
        "    let a = array_u16([1, 2])\n    print(array_add(a, a))\n    print(array_to_f32(a))\n    print(array_any(array_gt_scalar(a, 1)))\n",
    )
    .unwrap();
    let ops: Vec<&Op> = artifact
        .functions
        .iter()
        .flat_map(|function| function.code.iter().map(|instruction| &instruction.op))
        .collect();
    for expected in [
        Op::Array(ArrayFunction::FromList, ElementType::U16),
        Op::Array(ArrayFunction::Add, ElementType::U16),
        Op::Array(ArrayFunction::Convert, ElementType::F32),
        Op::Array(ArrayFunction::GtScalar, ElementType::U16),
        Op::Array(ArrayFunction::Any, ElementType::Bool),
    ] {
        assert!(ops.contains(&&expected), "missing {expected:?} in {ops:?}");
    }
}

#[test]
fn concat_binds_through_list() {
    compile_main(
        "    let a = array_i8([1])\n    let b: Array[i8] = array_concat([a, a])\n    print(b)\n",
    )
    .unwrap();
}

#[test]
fn set_local_assignment_updates_in_place() {
    let artifact = compile_main(
        "    var a = array_f64_zeros([4])\n    set a = array_set(a, 1, 2.5)\n    print(a)\n",
    )
    .unwrap();
    let ops: Vec<&Op> = artifact
        .functions
        .iter()
        .flat_map(|function| function.code.iter().map(|instruction| &instruction.op))
        .collect();
    assert!(
        ops.iter().any(|op| matches!(op, Op::ArraySetLocal(_))),
        "{ops:?}"
    );
    assert!(!ops.contains(&&Op::Array(ArrayFunction::Set, ElementType::F64)));
    assert_eq!(
        compile_main("    var a = array_f64_zeros([4])\n    set a = array_set(a, 1, 2)\n")
            .unwrap_err()
            .code,
        "E_TYPE_ARGUMENT"
    );
}
