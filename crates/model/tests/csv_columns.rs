use guion_model::{Artifact, Function, Instruction, Op, Span, Type};

fn artifact_with(code: Vec<Op>) -> Artifact {
    Artifact {
        format_version: Artifact::CURRENT_VERSION,
        build_metadata: None,
        compiled_source_sha256: None,
        source_name: "csv.gu".into(),
        sources: vec!["csv.gu".into()],
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
fn the_current_format_is_64() {
    assert_eq!(Artifact::CURRENT_VERSION, 64);
}

#[test]
fn format_63_rejects_csv_read_columns() {
    let mut artifact = artifact_with(vec![Op::CsvReadColumns]);
    let bytes = artifact.to_bytes().unwrap();
    assert!(Artifact::from_bytes(&bytes).is_ok());
    artifact.format_version = 63;
    assert_eq!(artifact.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");
    let mut legacy = bytes;
    legacy[4..8].copy_from_slice(&63u32.to_le_bytes());
    assert_eq!(
        Artifact::from_bytes(&legacy).unwrap_err().code,
        "E_ARTIFACT_VERSION"
    );
    let mut older = artifact_with(vec![Op::CsvNext]);
    older.format_version = 63;
    assert!(older.to_bytes().is_ok());
}
