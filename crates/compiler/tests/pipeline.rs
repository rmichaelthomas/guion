use guion_compiler::{compile, compile_project, compile_project_with_roots, Diagnostic};

#[test]
fn compiles_a_general_function_and_rejects_unknown_names_with_a_span() {
    let source =
        "fn main() -> Int:\n    let answer = 40 + 2\n    print(answer)\n    return answer\n";
    let artifact = compile("hello.gu", source).expect("valid source should compile");
    assert_eq!(artifact.entry(), "main");

    let error = compile("broken.gu", "fn main() -> Int:\n    return missing\n")
        .expect_err("unknown names must be rejected during checking");
    assert_eq!(error.code, "E_NAME_UNKNOWN");
    assert_eq!(error.span.line, 2);
    assert_eq!(error.span.column, 12);
}

#[test]
fn local_resolver_has_the_public_typed_signature_and_rejects_wrong_arguments() {
    use std::{collections::HashMap, fs, path::PathBuf};

    let root = std::env::temp_dir().join(format!(
        "guion-temporal-signature-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let entry = root.join("main.gu");
    fs::write(
        &entry,
        "import \"std:time\"\nfn main() -> Int:\n    let resolution = resolve_local(LocalDateTime(2026, 11, 1, 1, 30, 0, 0), \"America/Los_Angeles\")\n    return 0\n",
    )
    .unwrap();
    let stdlib = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib");
    let artifact = compile_project_with_roots(&entry, &root, Some(&stdlib), &HashMap::new())
        .expect("the public local resolver signature type-checks");
    assert_eq!(artifact.format_version, 40);

    let bad_entry = root.join("bad.gu");
    fs::write(
        &bad_entry,
        "import \"std:time\"\nfn main() -> Int:\n    let resolution = resolve_local(42, \"America/Los_Angeles\")\n    return 0\n",
    )
    .unwrap();
    let error = compile_project_with_roots(&bad_entry, &root, Some(&stdlib), &HashMap::new())
        .expect_err("an integer is not a LocalDateTime");
    assert_eq!(error.code, "E_TYPE_ARGUMENT");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn rejects_incomplete_source_instead_of_ignoring_trailing_tokens() {
    let error: Diagnostic = compile("broken.gu", "fn main() -> Int:\n    return 1 @ 2\n")
        .expect_err("unsupported syntax must be rejected");
    assert_eq!(error.code, "E_SYNTAX");
    assert_eq!(error.span.line, 2);
}

#[test]
fn accepts_a_value_function_when_every_if_branch_returns() {
    let source = "fn main() -> Int:\n    if true:\n        return 1\n    else:\n        return 2\n";
    compile("branches.gu", source).expect("both control-flow branches return");
}

#[test]
fn source_columns_count_unicode_characters_while_spans_keep_byte_offsets() {
    let source = "fn main() -> Int:\n    print(\"é\") @\n";
    let error = compile("unicode.gu", source).unwrap_err();
    assert_eq!(error.code, "E_SYNTAX");
    assert_eq!(error.span.column, 16);
    assert_eq!(&source[error.span.start..error.span.end], "@");
}

#[test]
fn compiles_typed_python_declarations_and_rejects_signature_mismatches() {
    let source = "extern python \"math:factorial\" fn factorial(value: Int) -> Int\n\nfn main() -> Int:\n    return factorial(6)\n";
    let artifact = compile("python.gu", source).expect("typed Python declaration compiles");
    assert_eq!(artifact.externals.len(), 1);
    assert_eq!(artifact.externals[0].module, "math");
    assert!(artifact.functions[0]
        .code
        .iter()
        .any(|instruction| matches!(instruction.op, guion_model::Op::PythonCall(0, 1))));

    let mismatch = compile("python-bad.gu", "extern python \"math:factorial\" fn factorial(value: Int) -> Int\n\nfn main() -> Int:\n    return factorial(\"six\")\n").unwrap_err();
    assert_eq!(mismatch.code, "E_TYPE_ARGUMENT");
}

#[test]
fn imported_module_instruction_keeps_its_own_source_id() {
    use guion_model::Op;
    use std::fs;

    let root = std::env::temp_dir().join(format!(
        "guion-compiler-source-id-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/main.gu"),
        "import \"math.gu\"\n\nfn main() -> Int:\n    return divide()\n",
    )
    .unwrap();
    fs::write(
        root.join("src/math.gu"),
        "pub fn divide() -> Int:\n    return 1 / 0\n",
    )
    .unwrap();

    let artifact = compile_project(&root.join("src/main.gu"), &root).unwrap();
    let math_source_id = artifact
        .sources
        .iter()
        .position(|source| source == "src/math.gu")
        .unwrap() as u32;
    let divide = artifact
        .functions
        .iter()
        .find(|function| function.name == "divide")
        .unwrap();
    let divide_by_zero = divide
        .code
        .iter()
        .find(|instruction| matches!(instruction.op, Op::IntegerDivide))
        .unwrap();
    assert_eq!(divide_by_zero.span.source_id, math_source_id);
    assert_eq!(divide_by_zero.span.line, 2);

    let _ = fs::remove_dir_all(root);
}
