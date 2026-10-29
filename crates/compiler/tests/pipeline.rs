use guion_compiler::{
    compile, compile_project, compile_project_with_roots, compile_project_with_roots_profiled,
    Diagnostic,
};

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
    assert_eq!(
        artifact.format_version,
        guion_model::Artifact::CURRENT_VERSION
    );

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
fn public_import_reexports_public_functions_and_records() {
    use std::fs;

    let root = std::env::temp_dir().join(format!(
        "guion-public-import-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("main.gu"),
        "import \"facade.gu\"\nfn main() -> Int:\n    let token = Token(41)\n    let incremented = increment(token)\n    assert(incremented.value == 42)\n    return 0\n",
    )
    .unwrap();
    fs::write(root.join("facade.gu"), "pub import \"inner.gu\"\npub fn facade_value() -> Int:\n    return 0\n").unwrap();
    fs::write(
        root.join("inner.gu"),
        "pub record Token:\n    value: Int\n\npub fn increment(token: Token) -> Token:\n    return Token(token.value + 1)\n",
    )
    .unwrap();

    compile_project(&root.join("main.gu"), &root)
        .expect("public imports re-export declarations to downstream modules");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn http_response_helper_exports_its_minimal_types_without_loading_time_modules() {
    use std::{collections::HashMap, fs, path::PathBuf};

    let root = std::env::temp_dir().join(format!(
        "guion-http-types-profile-{}-{}",
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
        "import \"std:http\"\nfn decode(response: HttpResponse) -> Result[Json, StdError]:\n    return http_response_json(response)\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let stdlib = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib");

    let (artifact, profile) = compile_project_with_roots_profiled(
        &entry,
        &root,
        Some(&stdlib),
        &HashMap::new(),
    )
    .expect("std:http exports the types in its public typed HTTP signature");

    assert_eq!(artifact.sources.len(), 3);
    assert_eq!(
        profile
            .modules
            .iter()
            .map(|module| module.source_name.as_str())
            .collect::<std::collections::HashSet<_>>(),
        std::collections::HashSet::from(["main.gu", "std:http", "std:http_types"]),
        "the typed HTTP helper should share its small record module without pulling in std:types or std:time"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn typed_http_helper_remains_compatible_when_std_types_is_also_imported() {
    use std::{collections::HashMap, fs, path::PathBuf};

    let root = std::env::temp_dir().join(format!(
        "guion-http-types-compat-{}-{}",
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
        "import \"std:http\"\nimport \"std:types\"\nfn decode(response: HttpResponse) -> Result[Json, StdError]:\n    return http_response_json(response)\n\nfn main() -> Int:\n    let response = HttpResponse(200, {}, bytes_from_text(\"{}\"))\n    let parsed = unwrap(http_response_json(response))\n    assert(json_as_int(json_get(parsed, \"answer\")) == 42)\n    return 0\n",
    )
    .unwrap();
    let stdlib = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib");

    compile_project_with_roots(&entry, &root, Some(&stdlib), &HashMap::new())
        .expect("the existing combined std:http and std:types import pattern remains valid");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn profiled_compile_attributes_each_standard_library_module_once() {
    use std::{collections::HashMap, fs, path::PathBuf};

    let root = std::env::temp_dir().join(format!(
        "guion-compiler-profile-{}-{}",
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
        "import \"std:http\"\nimport \"std:types\"\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let stdlib = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib");

    let (artifact, profile) = compile_project_with_roots_profiled(
        &entry,
        &root,
        Some(&stdlib),
        &HashMap::new(),
    )
    .expect("typed HTTP import graph compiles");

    assert_eq!(artifact.sources.len(), profile.modules.len());
    for expected in ["main.gu", "std:http", "std:types", "std:time", "std:http_types"] {
        assert_eq!(
            profile
                .modules
                .iter()
                .filter(|module| module.source_name == expected)
                .count(),
            1,
            "profile includes exactly one timing record for {expected}"
        );
    }
    assert!(profile.compile_program_ns > 0);
    assert!(profile.modules.iter().all(|module| {
        module.source_bytes > 0
            && module.path_validation_ns > 0
            && module.source_read_ns > 0
            && module.parse_ns > 0
    }));

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

#[test]
fn http_response_text_is_typechecked_for_http_response_only() {
    let valid = "record HttpResponse:\n    status: Int\n    headers: Map[Text, Text]\n    body: Bytes\n\nfn decode(response: HttpResponse) -> Result[Text, HttpError]:\n    return http_response_text(response)\n\nfn main() -> Int:\n    return 0\n";
    compile("http-response-text.gu", valid)
        .expect("HttpResponse is accepted by http_response_text");

    let invalid = "fn decode(response: Text) -> Result[Text, HttpError]:\n    return http_response_text(response)\n\nfn main() -> Int:\n    return 0\n";
    let error = compile("http-response-text-wrong-type.gu", invalid)
        .expect_err("http_response_text rejects non-HttpResponse values");
    assert_eq!(error.code, "E_TYPE_ARGUMENT");
}

#[test]
fn text_slice_is_typechecked_for_text_and_int_bounds() {
    let valid = "fn main() -> Int:\n    let selected = text_slice(\"a🙂é\", 1, 3)\n    assert(selected == \"🙂é\")\n    return 0\n";
    compile("text-slice.gu", valid).expect("text_slice accepts Text and Int bounds");

    let invalid = "fn main() -> Int:\n    let selected = text_slice(\"abc\", 0, \"2\")\n    return 0\n";
    let error = compile("text-slice-wrong-type.gu", invalid)
        .expect_err("text_slice rejects a non-Int bound");
    assert_eq!(error.code, "E_TYPE_ARGUMENT");
}
