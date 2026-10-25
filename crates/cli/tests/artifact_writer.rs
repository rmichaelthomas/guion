#![recursion_limit = "256"]

use guion_compiler::compile_project;
use guion_model::{Artifact, BigInt, Function, Instruction, Op, Span, Type, Value};
use guion_runtime::{HostPolicy, Runtime};
use std::{fs, path::PathBuf, process::Command};

#[test]
fn guion_artifact_writer_matches_native_writer_and_reader() {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let compiler_source = repository.join("language/self_host/compiler.gu");
    let compiler_artifact = compile_project(&compiler_source, &repository)
        .expect("stage-zero compiler builds the Guion artifact writer");
    let smoke_compiler_artifact = compiler_artifact.clone();
    let mut compiler_runtime = Runtime::new()
        .load(
            compiler_artifact,
            HostPolicy::default().with_file_access(&repository, true, false),
        )
        .expect("compiler artifact verifies");
    let ir = serde_json::json!({
        "schema": "guion-bytecode-ir/v1",
        "source_name": "sample.gu",
        "entry": "main",
        "sources": ["sample.gu"],
        "records": [],
        "externals": [],
        "functions": [{
            "name": "main",
            "parameters": [],
            "result": "Int",
            "local_count": 0,
            "code": [
                {"op": "ConstInt", "value": "0"},
                {"op": "Return"}
            ]
        }]
    });
    let encoded = compiler_runtime
        .invoke(
            "compiler_assemble",
            vec![Value::Json(guion_model::SharedJson::from_serde(ir))],
        )
        .expect("Guion writer returns artifact bytes");
    let Value::Bytes(encoded) = encoded else {
        panic!("Guion writer must return Bytes");
    };

    let native = Artifact {
        format_version: Artifact::CURRENT_VERSION,
        build_metadata: None,
        compiled_source_sha256: None,
        source_name: "sample.gu".into(),
        sources: vec!["sample.gu".into()],
        entry: "main".into(),
        functions: vec![Function {
            name: "main".into(),
            parameters: vec![],
            result: Type::Int,
            local_count: 0,
            code: vec![
                Instruction {
                    op: Op::Const(Value::Int(BigInt::zero())),
                    span: Span {
                        line: 1,
                        column: 1,
                        ..Span::default()
                    },
                },
                Instruction {
                    op: Op::Return,
                    span: Span {
                        line: 1,
                        column: 2,
                        ..Span::default()
                    },
                },
            ],
        }],
        externals: vec![],
        records: vec![],
    };
    let expected = native.to_bytes().unwrap();
    assert_eq!(encoded.as_ref().as_slice(), expected.as_slice());

    let checked = Artifact::from_bytes(encoded.as_ref()).expect("native reader accepts output");
    let mut runtime = Runtime::new()
        .load(checked, HostPolicy::default())
        .expect("native verifier accepts output");
    assert_eq!(
        runtime.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::zero())
    );

    let ir_json = serde_json::json!({
        "schema": "guion-bytecode-ir/v1",
        "source_name": "complex.gu",
        "entry": "main",
        "sources": ["complex.gu"],
        "records": [{"name":"Point","fields":[{"name":"x","type":"Int"},{"name":"y","type":"Int"},{"name":"payload","type":"Result[List[Map[Text, Json]], TaskError]"}]}],
        "externals": [{"name":"python_power","module":"math","callable":"pow","parameter_names":["x","y"],"parameters":["Float64","Float64"],"result":"Float64"}],
        "functions": [
            {"name":"add","parameters":["Int","Int"],"result":"Int","local_count":2,"code":[{"op":"Load","local":0},{"op":"Load","local":1},{"op":"Add"},{"op":"Return"}]},
            {"name":"kw","parameters":["Float64","Float64"],"result":"Float64","local_count":2,"code":[{"op":"Load","local":0},{"op":"Load","local":1},{"op":"PythonCallKw","function":"python_power","argc":0,"keyword_names":["y","x"]},{"op":"Return"}]},
            {"name":"main","parameters":[],"result":"Int","local_count":4,"code":[
                {"op":"ConstInt","value":"4"},{"op":"ConstInt","value":"5"},{"op":"MakeRecord","record":"Point","count":2},{"op":"Store","local":0},
                {"op":"Load","local":0},{"op":"GetField","record":"Point","field":"x"},{"op":"ConstInt","value":"1"},{"op":"Add"},{"op":"Store","local":1},
                {"op":"ConstText","value":"total"},{"op":"Load","local":1},{"op":"MakeMap","count":1},{"op":"Store","local":2},
                {"op":"Load","local":2},{"op":"ConstText","value":"total"},{"op":"MapHas"},{"op":"JumpIfFalse","label_id":7},
                {"op":"ConstText","value":"é"},{"op":"BytesFromText"},{"op":"ConstInt","value":"0"},{"op":"ConstInt","value":"255"},{"op":"MakeList","count":2},{"op":"BytesFromInts"},{"op":"MakeList","count":2},{"op":"BytesJoin"},{"op":"Store","local":3},
                {"op":"ConstFloat64","value":"1.0"},{"op":"Float64Bits"},{"op":"Pop"},{"op":"Load","local":1},{"op":"Return"},
                {"op":"Label","id":7},{"op":"ConstInt","value":"0"},{"op":"Return"}
            ]},
            {"name":"unit_value","parameters":[],"result":"Unit","local_count":0,"code":[{"op":"ConstUnit"},{"op":"Return"}]}
        ]
    });
    let encoded = compiler_runtime
        .invoke(
            "compiler_assemble",
            vec![Value::Json(guion_model::SharedJson::from_serde(
                ir_json.clone(),
            ))],
        )
        .expect("Guion assembler accepts compiler IR");
    let Value::Bytes(encoded) = encoded else {
        panic!("Guion assembler must return Bytes");
    };
    let directory =
        std::env::temp_dir().join(format!("guion-artifact-writer-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let ir_path = directory.join("complex.gui.json");
    let artifact_path = directory.join("complex.gub");
    fs::write(&ir_path, serde_json::to_vec(&ir_json).unwrap()).unwrap();
    let native = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args([
            "assemble-ir",
            ir_path.to_str().unwrap(),
            "--output",
            artifact_path.to_str().unwrap(),
        ])
        .output()
        .expect("run native reference assembler");
    assert!(
        native.status.success(),
        "native assembler rejected IR: {}",
        String::from_utf8_lossy(&native.stderr)
    );
    let native_bytes = fs::read(&artifact_path).unwrap();
    if encoded.as_ref().as_slice() != native_bytes.as_slice() {
        let mismatch = encoded
            .iter()
            .zip(&native_bytes)
            .position(|(left, right)| left != right)
            .unwrap_or_else(|| encoded.len().min(native_bytes.len()));
        panic!(
            "writer mismatch at byte {mismatch}: Guion {:?}, native {:?}; lengths {} and {}",
            &encoded[mismatch.saturating_sub(12)..(mismatch + 20).min(encoded.len())],
            &native_bytes[mismatch.saturating_sub(12)..(mismatch + 20).min(native_bytes.len())],
            encoded.len(),
            native_bytes.len()
        );
    }
    Artifact::from_bytes(encoded.as_ref()).expect("native reader accepts full writer output");
    fs::remove_dir_all(directory).unwrap();

    let smoke_name = format!(".artifact-writer-smoke-{}.gu", std::process::id());
    let smoke_path = repository.join(&smoke_name);
    let smoke_source = "fn main() -> Int:\n    return 0\n";
    fs::write(&smoke_path, smoke_source).unwrap();
    let compiler_args = vec![
        Value::Text(smoke_name),
        Value::Text(smoke_source.into()),
        Value::Text(String::new()),
        Value::Json(guion_model::SharedJson::from_serde(serde_json::json!({}))),
    ];
    let smoke_policy = HostPolicy::default().with_file_access(&repository, true, false);
    let compiled = std::thread::Builder::new()
        .name("guion-artifact-writer-smoke".into())
        .spawn(move || {
            let mut runtime = Runtime::new()
                .load(smoke_compiler_artifact, smoke_policy)
                .expect("load smoke compiler");
            runtime.invoke("compiler_compile_project", compiler_args)
        })
        .unwrap()
        .join()
        .expect("source compilation thread completes")
        .expect("Guion source-to-artifact entry returns a Result");
    fs::remove_file(&smoke_path).unwrap();
    let Value::ResultOk(bytes) = compiled else {
        panic!("Guion source-to-artifact entry must succeed for a valid source: {compiled:?}");
    };
    let Value::Bytes(bytes) = *bytes else {
        panic!("Guion source-to-artifact success must contain Bytes");
    };
    let compiled_artifact = Artifact::from_bytes(bytes.as_ref())
        .expect("native reader verifies the compiler's complete output");
    let mut compiled_runtime = Runtime::new()
        .load(compiled_artifact, HostPolicy::default())
        .expect("native verifier accepts compiler output");
    assert_eq!(
        compiled_runtime.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::zero())
    );
}

fn minimal_artifact(code: Vec<Instruction>) -> Artifact {
    Artifact {
        format_version: Artifact::CURRENT_VERSION,
        build_metadata: None,
        compiled_source_sha256: None,
        source_name: "malformed.gu".into(),
        sources: vec!["malformed.gu".into()],
        entry: "main".into(),
        functions: vec![Function {
            name: "main".into(),
            parameters: vec![],
            result: Type::Unit,
            local_count: 0,
            code,
        }],
        externals: vec![],
        records: vec![],
    }
}

#[test]
fn native_reader_rejects_truncated_overflow_bad_opcode_and_forged_targets() {
    let base = minimal_artifact(vec![Instruction {
        op: Op::Return,
        span: Span::default(),
    }])
    .to_bytes()
    .unwrap();
    assert_eq!(
        Artifact::from_bytes(&base[..base.len() - 1])
            .unwrap_err()
            .code,
        "E_ARTIFACT_TRUNCATED"
    );

    let mut count_overflow = base.clone();
    count_overflow[8..12].copy_from_slice(&(1_000_001u32).to_le_bytes());
    assert_eq!(
        Artifact::from_bytes(&count_overflow).unwrap_err().code,
        "E_ARTIFACT_SIZE"
    );

    let mut bad_opcode = base;
    let opcode_at = bad_opcode.len() - 2;
    bad_opcode[opcode_at] = 250;
    assert_eq!(
        Artifact::from_bytes(&bad_opcode).unwrap_err().code,
        "E_ARTIFACT_OPCODE"
    );

    let mut invalid_branch = minimal_artifact(vec![
        Instruction {
            op: Op::Jump(1),
            span: Span::default(),
        },
        Instruction {
            op: Op::Return,
            span: Span::default(),
        },
    ])
    .to_bytes()
    .unwrap();
    let branch_target_at = invalid_branch.len() - 42;
    invalid_branch[branch_target_at..branch_target_at + 4].copy_from_slice(&99u32.to_le_bytes());
    let decoded_branch =
        Artifact::from_bytes(&invalid_branch).expect("reader decodes branch bytes");
    assert!(
        Runtime::new()
            .load(decoded_branch, HostPolicy::default())
            .is_err(),
        "native verifier rejects a branch outside the function"
    );

    let mut forged_function = minimal_artifact(vec![
        Instruction {
            op: Op::Call(0, 0),
            span: Span::default(),
        },
        Instruction {
            op: Op::Return,
            span: Span::default(),
        },
    ])
    .to_bytes()
    .unwrap();
    let function_target_at = forged_function.len() - 46;
    forged_function[function_target_at..function_target_at + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    let decoded_call = Artifact::from_bytes(&forged_function).expect("reader decodes call bytes");
    assert!(
        Runtime::new()
            .load(decoded_call, HostPolicy::default())
            .is_err(),
        "native verifier rejects a forged function target"
    );
}
