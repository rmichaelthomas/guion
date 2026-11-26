#![recursion_limit = "256"]

use guion_model::{Artifact, Function, Instruction, Op, Span, Type};
use guion_runtime::{HostPolicy, Runtime};

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
