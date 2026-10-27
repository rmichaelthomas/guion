#[cfg(test)]
mod tests {
    use super::{
        Artifact, BigInt, BuildMetadata, ExternalFunction, FileErrorValue, Function, Instruction,
        JsonErrorValue, Op, ResourceLimits, SharedJson, Span, Type, Value,
    };
    use std::sync::Arc;

    #[test]
    fn cloned_text_values_share_immutable_storage() {
        let value = Value::Text("a sufficiently long immutable string value".into());
        let cloned = value.clone();
        let (Value::Text(original), Value::Text(copy)) = (value, cloned) else {
            panic!("text values remain text after cloning")
        };

        assert_eq!(original.as_ptr(), copy.as_ptr());
    }

    #[test]
    fn decimal_integer_math_is_unbounded_and_normalized() {
        let a = BigInt::parse("999999999999999999999999999999").unwrap();
        let b = BigInt::parse("1").unwrap();
        assert_eq!((&a + &b).to_string(), "1000000000000000000000000000000");
        assert_eq!((&b - &a).to_string(), "-999999999999999999999999999998");
        assert_eq!(BigInt::parse("-00042").unwrap().to_string(), "-42");
        assert_eq!(BigInt::parse("-0").unwrap().to_string(), "0");
    }

    #[test]
    fn arbitrary_precision_integer_converts_to_usize_without_string_roundtrip() {
        assert_eq!(BigInt::parse("0").unwrap().to_usize(), Some(0));
        assert_eq!(BigInt::parse("42").unwrap().to_usize(), Some(42));
        assert_eq!(BigInt::from_usize(usize::MAX).to_usize(), Some(usize::MAX));
        assert_eq!(BigInt::parse("-1").unwrap().to_usize(), None);
        assert_eq!(
            (&BigInt::from_usize(usize::MAX) + &BigInt::one()).to_usize(),
            None
        );
        assert_eq!(
            BigInt::parse("100000000000000000000000000000000000000")
                .unwrap()
                .to_usize(),
            None
        );
    }

    #[test]
    fn arbitrary_precision_integer_converts_to_i64_only_when_in_range() {
        assert_eq!(BigInt::parse("0").unwrap().to_i64(), Some(0));
        assert_eq!(
            BigInt::parse("9223372036854775807").unwrap().to_i64(),
            Some(i64::MAX)
        );
        assert_eq!(
            BigInt::parse("-9223372036854775808").unwrap().to_i64(),
            Some(i64::MIN)
        );
        assert_eq!(BigInt::parse("9223372036854775808").unwrap().to_i64(), None);
        assert_eq!(
            BigInt::parse("-9223372036854775809").unwrap().to_i64(),
            None
        );
        assert_eq!(
            BigInt::parse("12345678901234567890").unwrap().to_i64(),
            None
        );
    }

    #[test]
    fn signed_and_unsigned_machine_integers_convert_without_decimal_parsing() {
        for value in [0, 1, -1, i64::MIN, i64::MAX] {
            assert_eq!(BigInt::from_i64(value).to_i64(), Some(value));
        }
        for value in [0, 1, u64::MAX] {
            assert_eq!(BigInt::from_u64(value).to_string(), value.to_string());
        }
    }

    #[test]
    fn decimal_integer_multiplication_and_order_are_exact() {
        let a = BigInt::parse("-12345678901234567890").unwrap();
        let b = BigInt::parse("98765432109876543210").unwrap();
        assert_eq!(
            (&a * &b).to_string(),
            "-1219326311370217952237463801111263526900"
        );
        assert!(a < b);
    }

    #[test]
    fn arbitrary_precision_division_truncates_and_floors_with_contract_signs() {
        let negative_seven = BigInt::parse("-7").unwrap();
        let positive_three = BigInt::parse("3").unwrap();
        let (truncated, truncated_remainder) =
            negative_seven.trunc_div_rem(&positive_three).unwrap();
        assert_eq!(truncated.to_string(), "-2");
        assert_eq!(truncated_remainder.to_string(), "-1");
        let (floored, floored_remainder) = negative_seven.floor_div_rem(&positive_three).unwrap();
        assert_eq!(floored.to_string(), "-3");
        assert_eq!(floored_remainder.to_string(), "2");

        let positive_seven = BigInt::parse("7").unwrap();
        let negative_three = BigInt::parse("-3").unwrap();
        let (floored, remainder) = positive_seven.floor_div_rem(&negative_three).unwrap();
        assert_eq!(floored.to_string(), "-3");
        assert_eq!(remainder.to_string(), "-2");

        let large = BigInt::parse("123456789012345678901234567890").unwrap();
        let seven = BigInt::parse("7").unwrap();
        assert_eq!(
            large.trunc_div_rem(&seven).unwrap().0.to_string(),
            "17636684144620811271604938270"
        );
        assert!(positive_seven.trunc_div_rem(&BigInt::zero()).is_none());
    }

    #[test]
    fn arbitrary_precision_long_division_handles_the_runtime_digit_ceiling() {
        let dividend = BigInt::parse(&"9".repeat(4096)).unwrap();
        let divisor = BigInt::parse("9").unwrap();
        let (quotient, remainder) = dividend.trunc_div_rem(&divisor).unwrap();
        assert_eq!(quotient.to_string(), "1".repeat(4096));
        assert_eq!(remainder, BigInt::zero());
    }

    #[test]
    fn task5_service_opcodes_and_resource_types_require_format_37_or_newer() {
        let operations = [
            Op::TextCaseFold,
            Op::TextCollapseWhitespace,
            Op::TextNormalize,
            Op::CryptoHmacSha256,
            Op::CryptoVerifyHmacSha256,
            Op::SqliteOpen,
            Op::SqliteBegin,
            Op::SqliteExecute,
            Op::SqliteQuery,
            Op::SqliteCommit,
            Op::SqliteRollback,
            Op::SqliteClose,
            Op::TomlParse,
            Op::TomlHas,
            Op::TomlGetText,
            Op::TomlGetInt,
            Op::TomlGetBool,
            Op::TomlPutText,
            Op::TomlPutInt,
            Op::TomlPutBool,
            Op::TomlRender,
            Op::CalendarParse,
        ];
        let artifact = Artifact {
            format_version: Artifact::CURRENT_VERSION,
            build_metadata: None,
            compiled_source_sha256: None,
            source_name: "services.gu".into(),
            sources: vec!["services.gu".into()],
            entry: "main".into(),
            functions: vec![Function {
                name: "main".into(),
                parameters: vec![],
                result: Type::NativeResource("TomlDocument".into()),
                local_count: 0,
                code: operations
                    .into_iter()
                    .map(|op| Instruction {
                        op,
                        span: Span::default(),
                    })
                    .chain([Instruction {
                        op: Op::Return,
                        span: Span::default(),
                    }])
                    .collect(),
            }],
            externals: vec![],
            records: vec![],
        };

        assert_eq!(
            Artifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap(),
            artifact
        );
        let mut legacy = artifact;
        legacy.format_version = 36;
        assert_eq!(legacy.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");
    }

    #[test]
    fn byte_http_opcode_requires_format_38_or_newer() {
        let artifact = Artifact {
            format_version: Artifact::CURRENT_VERSION,
            build_metadata: None,
            compiled_source_sha256: None,
            source_name: "http-bytes.gu".into(),
            sources: vec!["http-bytes.gu".into()],
            entry: "main".into(),
            functions: vec![Function {
                name: "main".into(),
                parameters: vec![],
                result: Type::Int,
                local_count: 5,
                code: vec![Instruction {
                    op: Op::HttpRequestBytes,
                    span: Span::default(),
                }],
            }],
            externals: vec![],
            records: vec![],
        };
        assert_eq!(
            Artifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap(),
            artifact
        );
        let mut previous = artifact;
        previous.format_version = 37;
        assert_eq!(previous.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");
    }

    #[test]
    fn clock_opcodes_require_format_39_or_newer() {
        let artifact = Artifact {
            format_version: Artifact::CURRENT_VERSION,
            build_metadata: None,
            compiled_source_sha256: None,
            source_name: "clock.gu".into(),
            sources: vec!["clock.gu".into()],
            entry: "main".into(),
            functions: vec![Function {
                name: "main".into(),
                parameters: vec![],
                result: Type::Int,
                local_count: 0,
                code: vec![
                    Instruction {
                        op: Op::UnixTimeMs,
                        span: Span::default(),
                    },
                    Instruction {
                        op: Op::TimeTodayLocal,
                        span: Span::default(),
                    },
                    Instruction {
                        op: Op::Pop,
                        span: Span::default(),
                    },
                    Instruction {
                        op: Op::Return,
                        span: Span::default(),
                    },
                ],
            }],
            externals: vec![],
            records: vec![],
        };
        assert_eq!(
            Artifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap(),
            artifact
        );
        let mut previous = artifact;
        previous.format_version = 38;
        assert_eq!(previous.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");
    }

    #[test]
    fn temporal_opcodes_round_trip_in_format_40_and_current_format_with_append_only_tags() {
        for (op, expected_tag) in [(Op::TimeResolveLocal, 158), (Op::TimeZoneDataVersion, 159)] {
            let artifact = Artifact {
                format_version: 40,
                build_metadata: None,
                compiled_source_sha256: None,
                source_name: "temporal.gu".into(),
                sources: vec!["temporal.gu".into()],
                entry: "main".into(),
                functions: vec![Function {
                    name: "main".into(),
                    parameters: vec![],
                    result: Type::Int,
                    local_count: 0,
                    code: vec![
                        Instruction {
                            op,
                            span: Span::default(),
                        },
                        Instruction {
                            op: Op::Return,
                            span: Span::default(),
                        },
                    ],
                }],
                externals: vec![],
                records: vec![],
            };
            let bytes = artifact.to_bytes().unwrap();
            assert!(bytes.contains(&expected_tag));
            assert_eq!(Artifact::from_bytes(&bytes).unwrap(), artifact);

            let mut legacy_bytes = bytes;
            legacy_bytes[4..8].copy_from_slice(&39u32.to_le_bytes());
            assert_eq!(
                Artifact::from_bytes(&legacy_bytes).unwrap_err().code,
                "E_ARTIFACT_VERSION"
            );
            let mut legacy = artifact.clone();
            legacy.format_version = 39;
            assert_eq!(legacy.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");

            let mut current = artifact;
            current.format_version = Artifact::CURRENT_VERSION;
            let current_tag = match expected_tag {
                158 => 161,
                159 => 162,
                _ => unreachable!(),
            };
            let current_bytes = current.to_bytes().unwrap();
            assert!(current_bytes.contains(&current_tag));
            assert_eq!(Artifact::from_bytes(&current_bytes).unwrap(), current);
        }
    }

    #[test]
    fn http_bytecode_round_trips_only_in_format_19() {
        let artifact = Artifact {
            format_version: Artifact::CURRENT_VERSION,
            build_metadata: None,
            compiled_source_sha256: None,
            source_name: "http.gu".into(),
            sources: vec!["http.gu".into()],
            entry: "main".into(),
            functions: vec![Function {
                name: "main".into(),
                parameters: vec![],
                result: Type::Int,
                local_count: 0,
                code: vec![
                    Instruction {
                        op: Op::Const(Value::Text("https://example.com".into())),
                        span: Span::default(),
                    },
                    Instruction {
                        op: Op::HttpGet,
                        span: Span::default(),
                    },
                    Instruction {
                        op: Op::Return,
                        span: Span::default(),
                    },
                ],
            }],
            externals: vec![],
            records: vec![],
        };
        let encoded = artifact.to_bytes().unwrap();
        let decoded = Artifact::from_bytes(&encoded).unwrap();
        assert_eq!(decoded, artifact);

        let mut legacy = artifact;
        legacy.format_version = 18;
        assert_eq!(legacy.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");
    }

    #[test]
    fn json_construction_bytecode_requires_format_20_or_newer() {
        let artifact = Artifact {
            format_version: Artifact::CURRENT_VERSION,
            build_metadata: None,
            compiled_source_sha256: None,
            source_name: "json-builder.gu".into(),
            sources: vec!["json-builder.gu".into()],
            entry: "main".into(),
            functions: vec![Function {
                name: "main".into(),
                parameters: vec![],
                result: Type::Json,
                local_count: 0,
                code: vec![
                    Instruction {
                        op: Op::JsonArray,
                        span: Span::default(),
                    },
                    Instruction {
                        op: Op::Return,
                        span: Span::default(),
                    },
                ],
            }],
            externals: vec![],
            records: vec![],
        };
        let encoded = artifact.to_bytes().unwrap();
        assert_eq!(Artifact::from_bytes(&encoded).unwrap(), artifact);
        let mut mislabeled = encoded;
        mislabeled[4..8].copy_from_slice(&19u32.to_be_bytes());
        assert_eq!(
            Artifact::from_bytes(&mislabeled).unwrap_err().code,
            "E_ARTIFACT_VERSION"
        );
        let mut legacy = artifact;
        legacy.format_version = 19;
        assert_eq!(legacy.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");
    }

    #[test]
    fn atomic_text_write_bytecode_requires_format_21_or_newer() {
        let artifact = Artifact {
            format_version: Artifact::CURRENT_VERSION,
            build_metadata: None,
            compiled_source_sha256: None,
            source_name: "atomic.gu".into(),
            sources: vec!["atomic.gu".into()],
            entry: "main".into(),
            functions: vec![Function {
                name: "main".into(),
                parameters: vec![],
                result: Type::Int,
                local_count: 0,
                code: vec![
                    Instruction {
                        op: Op::Const(Value::Text("state.json".into())),
                        span: Span::default(),
                    },
                    Instruction {
                        op: Op::Const(Value::Text("{}".into())),
                        span: Span::default(),
                    },
                    Instruction {
                        op: Op::WriteTextAtomic,
                        span: Span::default(),
                    },
                    Instruction {
                        op: Op::Return,
                        span: Span::default(),
                    },
                ],
            }],
            externals: vec![],
            records: vec![],
        };
        let encoded = artifact.to_bytes().unwrap();
        assert_eq!(Artifact::from_bytes(&encoded).unwrap(), artifact);
        let mut mislabeled = encoded;
        mislabeled[4..8].copy_from_slice(&20u32.to_be_bytes());
        assert_eq!(
            Artifact::from_bytes(&mislabeled).unwrap_err().code,
            "E_ARTIFACT_VERSION"
        );
        let mut legacy = artifact;
        legacy.format_version = 20;
        assert_eq!(legacy.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");
    }

    #[test]
    fn artifacts_round_trip_with_versioned_source_maps() {
        let artifact = Artifact {
            format_version: Artifact::CURRENT_VERSION,
            build_metadata: Some(BuildMetadata {
                metadata_schema_version: 1,
                compiler_version: "0.1.0".into(),
                target: "linux-x86_64-gnu".into(),
                source_sha256: Some("a".repeat(64)),
                dependency_lock_sha256: Some("b".repeat(64)),
                resource_limits: ResourceLimits {
                    instruction_limit: 20_000,
                    allocation_limit_bytes: 16 * 1024 * 1024,
                    call_depth_limit: 32,
                    output_limit_bytes: 10_000,
                    max_integer_digits: 4096,
                    max_text_bytes: 16 * 1024 * 1024,
                    max_collection_items: 1_000_000,
                    max_parallelism: 8,
                    max_file_bytes: 32 * 1024 * 1024,
                },
            }),
            compiled_source_sha256: Some("a".repeat(64)),
            source_name: "sum.gu".into(),
            sources: vec!["sum.gu".into()],
            entry: "main".into(),
            functions: vec![Function {
                name: "main".into(),
                parameters: vec![
                    Type::List(Box::new(Type::Int)),
                    Type::Bytes,
                    Type::JsonError,
                    Type::FileError,
                ],
                result: Type::Int,
                local_count: 0,
                code: vec![
                    Instruction {
                        op: Op::Const(Value::List(Arc::new(vec![
                            Value::Int(BigInt::parse("123456789012345678901234567890").unwrap()),
                            Value::List(Arc::new(vec![Value::Bool(true), Value::Text("é".into())])),
                        ]))),
                        span: Span {
                            source_id: 0,
                            start: 10,
                            end: 40,
                            line: 2,
                            column: 12,
                        },
                    },
                    Instruction {
                        op: Op::Const(Value::Bytes(Arc::new(vec![0, 0xff, 0x80, b'a']))),
                        span: Span::default(),
                    },
                    Instruction {
                        op: Op::Return,
                        span: Span::default(),
                    },
                    Instruction {
                        op: Op::MakeList(3),
                        span: Span::default(),
                    },
                    Instruction {
                        op: Op::ParallelMap(0),
                        span: Span::default(),
                    },
                    Instruction {
                        op: Op::ReadText,
                        span: Span::default(),
                    },
                    Instruction {
                        op: Op::WriteText,
                        span: Span::default(),
                    },
                    Instruction {
                        op: Op::ReadBytes,
                        span: Span::default(),
                    },
                    Instruction {
                        op: Op::WriteBytes,
                        span: Span::default(),
                    },
                    Instruction {
                        op: Op::Args,
                        span: Span::default(),
                    },
                    Instruction {
                        op: Op::Assert,
                        span: Span::default(),
                    },
                    Instruction {
                        op: Op::PythonCall(0, 1),
                        span: Span::default(),
                    },
                    Instruction {
                        op: Op::PythonCallKw(0, 0, vec!["value".into()]),
                        span: Span::default(),
                    },
                    Instruction {
                        op: Op::Const(Value::Json(SharedJson::from_serde(
                            serde_json::from_str(
                                "{\"count\":123456789012345678901,\"name\":\"Guion\"}",
                            )
                            .unwrap(),
                        ))),
                        span: Span::default(),
                    },
                    Instruction {
                        op: Op::Const(Value::JsonError(JsonErrorValue {
                            code: "R_JSON_PARSE".into(),
                            message: "unexpected end of input".into(),
                            line: 1,
                            column: 1,
                        })),
                        span: Span::default(),
                    },
                    Instruction {
                        op: Op::Const(Value::FileError(FileErrorValue {
                            code: "R_FILE_READ".into(),
                            message: "file is missing".into(),
                            trace: vec![Span {
                                source_id: 0,
                                start: 5,
                                end: 14,
                                line: 3,
                                column: 7,
                            }],
                        })),
                        span: Span::default(),
                    },
                ],
            }],
            externals: vec![ExternalFunction {
                name: "factorial".into(),
                module: "math".into(),
                callable: "factorial".into(),
                parameter_names: vec!["value".into()],
                parameters: vec![Type::Int],
                result: Type::Int,
            }],
            records: vec![],
        };
        let bytes = artifact.to_bytes().unwrap();
        assert_eq!(Artifact::from_bytes(&bytes).unwrap(), artifact);

        let mut incompatible = artifact.clone();
        incompatible.format_version = 16;
        assert_eq!(
            incompatible.to_bytes().unwrap_err().code,
            "E_ARTIFACT_VERSION"
        );

        let mut keyword_version = artifact.clone();
        keyword_version.format_version = 17;
        keyword_version.functions[0]
            .parameters
            .retain(|ty| *ty != Type::Bytes);
        keyword_version.functions[0].code.retain(|instruction| {
            !matches!(instruction.op, Op::ReadBytes | Op::WriteBytes)
                && !matches!(&instruction.op, Op::Const(Value::Bytes(_)))
        });
        assert_eq!(
            keyword_version.to_bytes().unwrap_err().code,
            "E_ARTIFACT_VERSION"
        );

        let mut unsupported_format = artifact.clone();
        unsupported_format.format_version = Artifact::CURRENT_VERSION + 1;
        assert_eq!(
            unsupported_format.to_bytes().unwrap_err().code,
            "E_ARTIFACT_VERSION"
        );

        let mut unsupported_schema = artifact.clone();
        unsupported_schema
            .build_metadata
            .as_mut()
            .unwrap()
            .metadata_schema_version = 2;
        assert_eq!(
            unsupported_schema.to_bytes().unwrap_err().code,
            "E_ARTIFACT_METADATA_SCHEMA"
        );

        let mut legacy = artifact.clone();
        legacy.format_version = 15;
        legacy.functions[0]
            .parameters
            .retain(|ty| *ty != Type::Bytes);
        legacy.functions[0].code.retain(|instruction| {
            !matches!(instruction.op, Op::ReadBytes | Op::WriteBytes)
                && !matches!(instruction.op, Op::PythonCallKw(_, _, _))
                && !matches!(&instruction.op, Op::Const(Value::Bytes(_)))
        });
        legacy
            .build_metadata
            .as_mut()
            .unwrap()
            .metadata_schema_version = 0;
        legacy.build_metadata.as_mut().unwrap().source_sha256 = None;
        legacy
            .build_metadata
            .as_mut()
            .unwrap()
            .dependency_lock_sha256 = None;
        legacy.compiled_source_sha256 = None;
        let legacy_bytes = legacy.to_bytes().unwrap();
        assert_eq!(Artifact::from_bytes(&legacy_bytes).unwrap(), legacy);

        legacy.format_version = 14;
        legacy.build_metadata = None;
        let legacy_bytes = legacy.to_bytes().unwrap();
        assert_eq!(Artifact::from_bytes(&legacy_bytes).unwrap(), legacy);
        let mut corrupted = bytes;
        corrupted[0] ^= 0xff;
        assert_eq!(
            Artifact::from_bytes(&corrupted).unwrap_err().code,
            "E_ARTIFACT_MAGIC"
        );
    }

    #[test]
    fn artifact_reader_rejects_truncation_and_oversized_fields() {
        let artifact = Artifact {
            format_version: Artifact::CURRENT_VERSION,
            build_metadata: None,
            compiled_source_sha256: None,
            source_name: "x".into(),
            sources: vec!["x".into()],
            entry: "main".into(),
            functions: vec![Function {
                name: "main".into(),
                parameters: vec![],
                result: Type::Unit,
                local_count: 0,
                code: vec![Instruction {
                    op: Op::Return,
                    span: Span::default(),
                }],
            }],
            externals: vec![],
            records: vec![],
        };
        let bytes = artifact.to_bytes().unwrap();

        let mut truncated = bytes.clone();
        truncated.pop();
        assert_eq!(
            Artifact::from_bytes(&truncated).unwrap_err().code,
            "E_ARTIFACT_TRUNCATED"
        );

        let mut oversized_string = bytes.clone();
        oversized_string[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            Artifact::from_bytes(&oversized_string).unwrap_err().code,
            "E_ARTIFACT_SIZE"
        );

        // The source count follows the source name and entry strings.
        let source_count_offset = 8 + 4 + artifact.source_name.len() + 4 + artifact.entry.len();
        let mut oversized_count = bytes.clone();
        oversized_count[source_count_offset..source_count_offset + 4]
            .copy_from_slice(&1_000_001_u32.to_le_bytes());
        assert_eq!(
            Artifact::from_bytes(&oversized_count).unwrap_err().code,
            "E_ARTIFACT_SIZE"
        );

        // This fixture has a fixed header through instruction count, then the
        // 36-byte span, followed by the one-byte Return opcode.
        let mut opcode_offset = 8;
        opcode_offset += 4 + artifact.source_name.len();
        opcode_offset += 4 + artifact.entry.len();
        opcode_offset += 4; // source count
        opcode_offset += 4 + artifact.sources[0].len();
        opcode_offset += 4; // record count
        opcode_offset += 4; // external count
        opcode_offset += 4; // function count
        opcode_offset += 4 + "main".len();
        opcode_offset += 4; // parameter count
        opcode_offset += 1; // result type
        opcode_offset += 4; // local count
        opcode_offset += 4; // instruction count
        opcode_offset += 36; // source span
        let mut bad_opcode = bytes;
        bad_opcode[opcode_offset] = u8::MAX;
        assert_eq!(
            Artifact::from_bytes(&bad_opcode).unwrap_err().code,
            "E_ARTIFACT_OPCODE"
        );
    }

    #[test]
    fn cloning_json_values_shares_immutable_storage() {
        let original = Value::Json(SharedJson::from_serde(serde_json::json!({
            "rows": [{"amount": 10}, {"amount": 20}]
        })));
        let copied = original.clone();
        let (Value::Json(original), Value::Json(copied)) = (&original, &copied) else {
            panic!("expected JSON values")
        };
        assert!(Arc::ptr_eq(original, copied));

        let original_list = Value::List(Arc::new(vec![Value::Int(BigInt::from_usize(1))]));
        let copied_list = original_list.clone();
        let (Value::List(original_list), Value::List(copied_list)) = (&original_list, &copied_list)
        else {
            panic!("expected list values")
        };
        assert!(Arc::ptr_eq(original_list, copied_list));
    }
}

use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashSet},
    fmt,
    ops::{Add, Mul, Sub},
    sync::Arc,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub source_id: u32,
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: String,
    pub message: String,
    pub span: Span,
    pub source_name: Option<String>,
}

impl Diagnostic {
    pub fn new(code: impl Into<String>, message: impl Into<String>, span: Span) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            span,
            source_name: None,
        }
    }

    pub fn with_source_name(mut self, source_name: impl Into<String>) -> Self {
        self.source_name = Some(source_name.into());
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BigInt {
    sign: i8,
    // Base 10, least significant digit first.
    digits: Vec<u8>,
}

impl BigInt {
    pub fn parse(input: &str) -> Result<Self, &'static str> {
        if input.is_empty() {
            return Err("empty integer");
        }
        let (sign, digits) = match input.strip_prefix('-') {
            Some(rest) => (-1, rest),
            None => (1, input.strip_prefix('+').unwrap_or(input)),
        };
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err("invalid integer");
        }
        let mut value = Self {
            sign,
            digits: digits.bytes().rev().map(|b| b - b'0').collect(),
        };
        value.normalize();
        Ok(value)
    }

    pub fn zero() -> Self {
        Self {
            sign: 0,
            digits: vec![0],
        }
    }
    pub fn one() -> Self {
        Self {
            sign: 1,
            digits: vec![1],
        }
    }
    pub fn from_usize(value: usize) -> Self {
        Self::parse(&value.to_string()).expect("usize is a decimal integer")
    }
    pub fn from_u64(mut value: u64) -> Self {
        if value == 0 {
            return Self::zero();
        }
        let mut digits = Vec::new();
        while value != 0 {
            digits.push((value % 10) as u8);
            value /= 10;
        }
        Self { sign: 1, digits }
    }
    pub fn from_i64(value: i64) -> Self {
        if value < 0 {
            let mut integer = Self::from_u64(value.unsigned_abs());
            integer.sign = -1;
            integer
        } else {
            Self::from_u64(value as u64)
        }
    }
    pub fn to_usize(&self) -> Option<usize> {
        if self.sign < 0 {
            return None;
        }
        self.digits.iter().rev().try_fold(0usize, |value, digit| {
            value.checked_mul(10)?.checked_add(usize::from(*digit))
        })
    }
    pub fn to_i64(&self) -> Option<i64> {
        if self.sign == 0 {
            return Some(0);
        }
        let magnitude = self.digits.iter().rev().try_fold(0u64, |value, digit| {
            value.checked_mul(10)?.checked_add(u64::from(*digit))
        })?;
        if self.sign < 0 {
            let minimum_magnitude = (i64::MAX as u64) + 1;
            if magnitude == minimum_magnitude {
                Some(i64::MIN)
            } else {
                i64::try_from(magnitude).ok()?.checked_neg()
            }
        } else {
            i64::try_from(magnitude).ok()
        }
    }
    pub fn is_zero(&self) -> bool {
        self.sign == 0
    }
    pub fn is_negative(&self) -> bool {
        self.sign < 0
    }
    pub fn digit_count(&self) -> usize {
        self.digits.len()
    }

    pub fn trunc_div_rem(&self, rhs: &Self) -> Option<(Self, Self)> {
        if rhs.is_zero() {
            return None;
        }
        if self.is_zero() {
            return Some((Self::zero(), Self::zero()));
        }
        let divisor = Self {
            sign: 1,
            digits: rhs.digits.clone(),
        };
        let multiples = (1..=9)
            .map(|factor| divisor.mul_small(factor))
            .collect::<Vec<_>>();
        let mut remainder = Self::zero();
        let mut quotient_msd = Vec::with_capacity(self.digits.len());
        for digit in self.digits.iter().rev() {
            if remainder.is_zero() {
                remainder.digits[0] = *digit;
                remainder.normalize();
                if *digit != 0 {
                    remainder.sign = 1;
                }
            } else {
                remainder.digits.insert(0, *digit);
                remainder.normalize();
            }
            let (mut low, mut high) = (0usize, multiples.len());
            while low < high {
                let middle = low + (high - low).div_ceil(2);
                if multiples[middle - 1].cmp_abs(&remainder) != Ordering::Greater {
                    low = middle;
                } else {
                    high = middle - 1;
                }
            }
            let qdigit = low;
            if qdigit != 0 {
                remainder = &remainder - &multiples[qdigit - 1];
            }
            quotient_msd.push(qdigit as u8);
        }
        let mut quotient = Self {
            sign: self.sign * rhs.sign,
            digits: quotient_msd.into_iter().rev().collect(),
        };
        quotient.normalize();
        if !remainder.is_zero() {
            remainder.sign = self.sign;
        }
        Some((quotient, remainder))
    }

    pub fn floor_div_rem(&self, rhs: &Self) -> Option<(Self, Self)> {
        let (mut quotient, mut remainder) = self.trunc_div_rem(rhs)?;
        if !remainder.is_zero() && self.sign != rhs.sign {
            quotient = &quotient - &Self::one();
            remainder = &remainder + rhs;
        }
        Some((quotient, remainder))
    }

    fn mul_small(&self, factor: u8) -> Self {
        if factor == 0 || self.is_zero() {
            return Self::zero();
        }
        let mut digits = Vec::with_capacity(self.digits.len() + 1);
        let mut carry = 0u16;
        for digit in &self.digits {
            let product = u16::from(*digit) * u16::from(factor) + carry;
            digits.push((product % 10) as u8);
            carry = product / 10;
        }
        while carry != 0 {
            digits.push((carry % 10) as u8);
            carry /= 10;
        }
        Self {
            sign: self.sign,
            digits,
        }
    }

    fn normalize(&mut self) {
        while self.digits.len() > 1 && self.digits.last() == Some(&0) {
            self.digits.pop();
        }
        if self.digits.iter().all(|d| *d == 0) {
            self.digits = vec![0];
            self.sign = 0;
        }
    }

    fn cmp_abs(&self, other: &Self) -> Ordering {
        self.digits
            .len()
            .cmp(&other.digits.len())
            .then_with(|| self.digits.iter().rev().cmp(other.digits.iter().rev()))
    }

    fn add_abs(a: &Self, b: &Self) -> Vec<u8> {
        let n = a.digits.len().max(b.digits.len());
        let mut out = Vec::with_capacity(n + 1);
        let mut carry = 0u16;
        for i in 0..n {
            let sum = u16::from(*a.digits.get(i).unwrap_or(&0))
                + u16::from(*b.digits.get(i).unwrap_or(&0))
                + carry;
            out.push((sum % 10) as u8);
            carry = sum / 10;
        }
        if carry != 0 {
            out.push(carry as u8);
        }
        out
    }

    // Requires |a| >= |b|.
    fn sub_abs(a: &Self, b: &Self) -> Vec<u8> {
        let mut out = Vec::with_capacity(a.digits.len());
        let mut borrow = 0i16;
        for i in 0..a.digits.len() {
            let mut value =
                i16::from(a.digits[i]) - i16::from(*b.digits.get(i).unwrap_or(&0)) - borrow;
            if value < 0 {
                value += 10;
                borrow = 1;
            } else {
                borrow = 0;
            }
            out.push(value as u8);
        }
        out
    }
}

impl Add<&BigInt> for &BigInt {
    type Output = BigInt;
    fn add(self, rhs: &BigInt) -> BigInt {
        let mut out = match (self.sign, rhs.sign) {
            (0, _) => rhs.clone(),
            (_, 0) => self.clone(),
            (a, b) if a == b => BigInt {
                sign: a,
                digits: BigInt::add_abs(self, rhs),
            },
            _ => match self.cmp_abs(rhs) {
                Ordering::Equal => BigInt::zero(),
                Ordering::Greater => BigInt {
                    sign: self.sign,
                    digits: BigInt::sub_abs(self, rhs),
                },
                Ordering::Less => BigInt {
                    sign: rhs.sign,
                    digits: BigInt::sub_abs(rhs, self),
                },
            },
        };
        out.normalize();
        out
    }
}

impl Sub<&BigInt> for &BigInt {
    type Output = BigInt;
    fn sub(self, rhs: &BigInt) -> BigInt {
        let mut neg = rhs.clone();
        neg.sign = -neg.sign;
        self + &neg
    }
}

impl Mul<&BigInt> for &BigInt {
    type Output = BigInt;
    fn mul(self, rhs: &BigInt) -> BigInt {
        if self.is_zero() || rhs.is_zero() {
            return BigInt::zero();
        }
        let mut out = vec![0u64; self.digits.len() + rhs.digits.len()];
        for (i, a) in self.digits.iter().enumerate() {
            for (j, b) in rhs.digits.iter().enumerate() {
                out[i + j] += u64::from(*a) * u64::from(*b);
            }
        }
        for i in 0..out.len() - 1 {
            let carry = out[i] / 10;
            out[i] %= 10;
            out[i + 1] += carry;
        }
        let mut result = BigInt {
            sign: self.sign * rhs.sign,
            digits: out.into_iter().map(|d| d as u8).collect(),
        };
        result.normalize();
        result
    }
}

impl PartialOrd for BigInt {
    fn partial_cmp(&self, rhs: &Self) -> Option<Ordering> {
        Some(self.cmp(rhs))
    }
}
impl Ord for BigInt {
    fn cmp(&self, rhs: &Self) -> Ordering {
        match self.sign.cmp(&rhs.sign) {
            Ordering::Equal if self.sign < 0 => rhs.cmp_abs(self),
            Ordering::Equal => self.cmp_abs(rhs),
            other => other,
        }
    }
}
impl fmt::Display for BigInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.sign < 0 {
            f.write_str("-")?;
        }
        for digit in self.digits.iter().rev() {
            write!(f, "{digit}")?;
        }
        Ok(())
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug)]
pub struct Float64(u64);

impl Float64 {
    pub fn new(value: f64) -> Option<Self> {
        value.is_finite().then_some(Self(value.to_bits()))
    }
    pub fn get(self) -> f64 {
        f64::from_bits(self.0)
    }
}
impl PartialEq for Float64 {
    fn eq(&self, other: &Self) -> bool {
        self.get() == other.get()
    }
}
impl Eq for Float64 {}

/// Immutable JSON nodes with shared child storage. Reading a nested value
/// clones an Arc instead of recursively copying the entire subtree.
#[derive(Clone, Debug, PartialEq)]
pub enum SharedJson {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(Arc<str>),
    Array(Arc<Vec<Arc<SharedJson>>>),
    Object(Arc<BTreeMap<String, Arc<SharedJson>>>),
}

impl SharedJson {
    pub fn from_serde(value: serde_json::Value) -> Arc<Self> {
        Arc::new(match value {
            serde_json::Value::Null => Self::Null,
            serde_json::Value::Bool(value) => Self::Bool(value),
            serde_json::Value::Number(value) => Self::Number(value),
            serde_json::Value::String(value) => Self::String(Arc::from(value)),
            serde_json::Value::Array(values) => {
                Self::Array(Arc::new(values.into_iter().map(Self::from_serde).collect()))
            }
            serde_json::Value::Object(values) => Self::Object(Arc::new(
                values
                    .into_iter()
                    .map(|(key, value)| (key, Self::from_serde(value)))
                    .collect(),
            )),
        })
    }

    pub fn to_serde(&self) -> serde_json::Value {
        match self {
            Self::Null => serde_json::Value::Null,
            Self::Bool(value) => serde_json::Value::Bool(*value),
            Self::Number(value) => serde_json::Value::Number(value.clone()),
            Self::String(value) => serde_json::Value::String(value.to_string()),
            Self::Array(values) => {
                serde_json::Value::Array(values.iter().map(|value| value.to_serde()).collect())
            }
            Self::Object(values) => serde_json::Value::Object(
                values
                    .iter()
                    .map(|(key, value)| (key.clone(), value.to_serde()))
                    .collect(),
            ),
        }
    }

    pub fn as_array(&self) -> Option<&[Arc<SharedJson>]> {
        match self {
            Self::Array(values) => Some(values),
            _ => None,
        }
    }

    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Arc<SharedJson>>> {
        match self {
            Self::Array(values) => Some(Arc::make_mut(values)),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&BTreeMap<String, Arc<SharedJson>>> {
        match self {
            Self::Object(values) => Some(values),
            _ => None,
        }
    }

    pub fn as_object_mut(&mut self) -> Option<&mut BTreeMap<String, Arc<SharedJson>>> {
        match self {
            Self::Object(values) => Some(Arc::make_mut(values)),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_number(&self) -> Option<&serde_json::Number> {
        match self {
            Self::Number(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn is_number(&self) -> bool {
        matches!(self, Self::Number(_))
    }

    pub fn is_boolean(&self) -> bool {
        matches!(self, Self::Bool(_))
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
}

impl fmt::Display for SharedJson {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match serde_json::to_string(&self.to_serde()) {
            Ok(value) => f.write_str(&value),
            Err(_) => f.write_str("null"),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Value {
    Int(BigInt),
    Float64(Float64),
    Record(String, Arc<Vec<Value>>),
    Bytes(Arc<Vec<u8>>),
    Bool(bool),
    Text(Arc<str>),
    List(Arc<Vec<Value>>),
    Float64List(Arc<Vec<Float64>>),
    Map(Arc<BTreeMap<String, Value>>),
    OptionNone,
    OptionSome(Box<Value>),
    ResultOk(Box<Value>),
    ResultErr(Box<Value>),
    TaskError(TaskErrorValue),
    Json(Arc<SharedJson>),
    JsonError(JsonErrorValue),
    FileError(FileErrorValue),
    HttpError(HttpErrorValue),
    NativeResource(NativeResourceValue),
    Unit,
}

impl Value {
    /// Build a list, using packed Float64 storage when every element has that type.
    pub fn from_list_items(items: Vec<Value>) -> Self {
        if !items.is_empty() && items.iter().all(|item| matches!(item, Value::Float64(_))) {
            Value::Float64List(Arc::new(
                items
                    .into_iter()
                    .map(|item| match item {
                        Value::Float64(value) => value,
                        _ => unreachable!("the list was checked to contain only Float64 values"),
                    })
                    .collect(),
            ))
        } else {
            Value::List(Arc::new(items))
        }
    }

    pub fn list_len(&self) -> Option<usize> {
        match self {
            Value::List(items) => Some(items.len()),
            Value::Float64List(items) => Some(items.len()),
            _ => None,
        }
    }

    pub fn list_item(&self, index: usize) -> Option<Value> {
        match self {
            Value::List(items) => items.get(index).cloned(),
            Value::Float64List(items) => items.get(index).copied().map(Value::Float64),
            _ => None,
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Float64List(left), Value::Float64List(right)) => left == right,
            (Value::Float64List(left), Value::List(right)) => {
                left.len() == right.len()
                    && left.iter().zip(right.iter()).all(
                        |(left, right)| matches!(right, Value::Float64(right) if left == right),
                    )
            }
            (Value::List(left), Value::Float64List(right)) => {
                left.len() == right.len()
                    && left
                        .iter()
                        .zip(right.iter())
                        .all(|(left, right)| matches!(left, Value::Float64(left) if left == right))
            }
            (Value::Int(left), Value::Int(right)) => left == right,
            (Value::Float64(left), Value::Float64(right)) => left == right,
            (Value::Record(left_name, left), Value::Record(right_name, right)) => {
                left_name == right_name && left == right
            }
            (Value::Bytes(left), Value::Bytes(right)) => left == right,
            (Value::Bool(left), Value::Bool(right)) => left == right,
            (Value::Text(left), Value::Text(right)) => left == right,
            (Value::List(left), Value::List(right)) => left == right,
            (Value::Map(left), Value::Map(right)) => left == right,
            (Value::OptionNone, Value::OptionNone) => true,
            (Value::OptionSome(left), Value::OptionSome(right))
            | (Value::ResultOk(left), Value::ResultOk(right))
            | (Value::ResultErr(left), Value::ResultErr(right)) => left == right,
            (Value::TaskError(left), Value::TaskError(right)) => left == right,
            (Value::Json(left), Value::Json(right)) => left == right,
            (Value::JsonError(left), Value::JsonError(right)) => left == right,
            (Value::FileError(left), Value::FileError(right)) => left == right,
            (Value::HttpError(left), Value::HttpError(right)) => left == right,
            (Value::NativeResource(left), Value::NativeResource(right)) => left == right,
            (Value::Unit, Value::Unit) => true,
            _ => false,
        }
    }
}

impl Eq for Value {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskErrorValue {
    pub index: BigInt,
    pub code: String,
    pub message: String,
    pub trace: Vec<Span>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonErrorValue {
    pub code: String,
    pub message: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileErrorValue {
    pub code: String,
    pub message: String,
    pub trace: Vec<Span>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpErrorValue {
    pub code: String,
    pub message: String,
    pub trace: Vec<Span>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeResourceValue {
    kind: String,
    owner: u64,
    id: u64,
}

impl NativeResourceValue {
    pub fn new(kind: impl Into<String>, owner: u64, id: u64) -> Self {
        Self {
            kind: kind.into(),
            owner,
            id,
        }
    }
    pub fn kind(&self) -> &str {
        &self.kind
    }
    pub fn parts(&self) -> (&str, u64, u64) {
        (&self.kind, self.owner, self.id)
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(i) => write!(f, "{i}"),
            Value::Float64(value) => write!(f, "{}", value.get()),
            Value::Bytes(value) => write!(f, "Bytes({})", value.len()),
            Value::Record(name, fields) => {
                write!(f, "{name}(")?;
                for (index, field) in fields.iter().enumerate() {
                    if index != 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{field}")?;
                }
                f.write_str(")")
            }
            Value::Bool(b) => write!(f, "{b}"),
            Value::Text(t) => f.write_str(t),
            Value::List(items) => {
                f.write_str("[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{item}")?;
                }
                f.write_str("]")
            }
            Value::Float64List(items) => {
                f.write_str("[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{}", item.get())?;
                }
                f.write_str("]")
            }
            Value::Map(items) => {
                f.write_str("{")?;
                for (i, (key, value)) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{key:?}: {value}")?;
                }
                f.write_str("}")
            }
            Value::OptionNone => f.write_str("None"),
            Value::OptionSome(value) => write!(f, "Some({value})"),
            Value::ResultOk(value) => write!(f, "Ok({value})"),
            Value::ResultErr(value) => write!(f, "Err({value})"),
            Value::TaskError(error) => write!(
                f,
                "TaskError({}, {}, {})",
                error.index, error.code, error.message
            ),
            Value::Json(value) => write!(f, "{value}"),
            Value::JsonError(error) => write!(f, "JsonError({}, {})", error.code, error.message),
            Value::FileError(error) => write!(f, "FileError({}, {})", error.code, error.message),
            Value::HttpError(error) => write!(f, "HttpError({}, {})", error.code, error.message),
            Value::NativeResource(resource) => write!(f, "<{} resource>", resource.kind()),
            Value::Unit => f.write_str("()"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Type {
    Int,
    Float64,
    Record(String),
    Bytes,
    Bool,
    Text,
    List(Box<Type>),
    Map(Box<Type>),
    Option(Box<Type>),
    Result(Box<Type>, Box<Type>),
    TaskError,
    Json,
    JsonError,
    FileError,
    HttpError,
    NativeResource(String),
    Unit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    Const(Value),
    Load(usize),
    Store(usize),
    Pop,
    Add,
    Subtract,
    Multiply,
    Negate,
    Not,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Jump(usize),
    JumpIfFalse(usize),
    Call(usize, usize),
    Print,
    PrintErr,
    Return,
    Length,
    Index,
    MakeList(usize),
    MakeMap(usize),
    MakeTypedList(Type),
    MakeTypedMap(Type),
    MapKeys,
    MapSet,
    MapHas,
    ListPush,
    ListPushLocal(usize),
    ListSet,
    ListSetLocal(usize),
    Divide,
    FloatSqrt,
    JsonAsFloat,
    IntegerDivide,
    FloorDivide,
    Remainder,
    MakeRecord(usize, usize),
    GetField(usize, usize),
    ParallelMap(usize),
    ReadText,
    WriteText,
    WriteTextAtomic,
    ReadBytes,
    WriteBytes,
    Args,
    ArgsFrom,
    Assert,
    IsOk,
    Unwrap,
    GetError,
    TaskIndex,
    TaskCode,
    TaskMessage,
    PythonCall(usize, usize),
    PythonCallKw(usize, usize, Vec<String>),
    HttpGet,
    JsonParse,
    JsonStringify,
    JsonNull,
    JsonBool,
    JsonInt,
    JsonFloat,
    JsonText,
    JsonArray,
    JsonObject,
    JsonPush,
    JsonSet,
    JsonGet,
    JsonIndex,
    JsonHas,
    JsonLength,
    JsonAsText,
    JsonAsInt,
    JsonAsBool,
    JsonIsNull,
    JsonIntColumn,
    JsonTextCategories,
    JsonAsIntList,
    JsonProjectColumns,
    JsonTextCategoriesValues,
    JsonPushLocal(usize),
    JsonSetLocal(usize),
    JsonErrorMessage,
    JsonErrorLine,
    JsonErrorColumn,
    JsonErrorCode,
    FileErrorCode,
    FileErrorMessage,
    FileErrorLine,
    FileErrorColumn,
    HttpErrorCode,
    HttpErrorMessage,
    MakeNone,
    MakeSome,
    IsSome,
    MakeOk,
    MakeErr,
    TryResult,
    HttpRequest,
    HttpRequestWithTimeout,
    HttpRequestBytes,
    Environment,
    TextChars,
    BytesFromInts,
    BytesFromText,
    BytesJoin,
    Float64Bits,
    FileOpenLines,
    FileNextLine,
    FileClose,
    CsvOpen,
    CsvNext,
    CsvEncode,
    JsonTryGet,
    JsonTryIndex,
    JsonTryText,
    JsonTryInt,
    JsonTryBool,
    JsonTryFloat,
    TimeParseDate,
    TimeParseTimestamp,
    TimeFormatUtc,
    UnixTimeMs,
    TimeTodayLocal,
    TimeResolveLocal,
    TimeZoneDataVersion,
    SemVerParse,
    SemVerCompare,
    CliParse,
    FsWalk,
    ListGetOption,
    MapGetOption,
    TextParseInt,
    TextParseFloat,
    SortBy(usize),
    SortByIntKey(usize),
    MergeSortedByIntKey(usize),
    TextCaseFold,
    TextCollapseWhitespace,
    TextNormalize,
    CryptoHmacSha256,
    CryptoVerifyHmacSha256,
    SqliteOpen,
    SqliteBegin,
    SqliteExecute,
    SqliteQuery,
    SqliteCommit,
    SqliteRollback,
    SqliteClose,
    TomlParse,
    TomlHas,
    TomlGetText,
    TomlGetInt,
    TomlGetBool,
    TomlPutText,
    TomlPutInt,
    TomlPutBool,
    TomlRender,
    CalendarParse,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instruction {
    pub op: Op,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Function {
    pub name: String,
    pub parameters: Vec<Type>,
    pub result: Type,
    pub local_count: usize,
    pub code: Vec<Instruction>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalFunction {
    pub name: String,
    pub module: String,
    pub callable: String,
    pub parameter_names: Vec<String>,
    pub parameters: Vec<Type>,
    pub result: Type,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordField {
    pub name: String,
    pub ty: Type,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordDefinition {
    pub name: String,
    pub fields: Vec<RecordField>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceLimits {
    pub instruction_limit: u64,
    pub allocation_limit_bytes: usize,
    pub call_depth_limit: usize,
    pub output_limit_bytes: usize,
    pub max_integer_digits: usize,
    pub max_text_bytes: usize,
    pub max_collection_items: usize,
    pub max_parallelism: usize,
    pub max_file_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildMetadata {
    pub metadata_schema_version: u32,
    pub compiler_version: String,
    pub target: String,
    pub source_sha256: Option<String>,
    pub dependency_lock_sha256: Option<String>,
    pub resource_limits: ResourceLimits,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Artifact {
    pub format_version: u32,
    pub build_metadata: Option<BuildMetadata>,
    /// Digest of source bytes used by the compiler; serialized only in build metadata.
    pub compiled_source_sha256: Option<String>,
    pub source_name: String,
    pub sources: Vec<String>,
    pub entry: String,
    pub functions: Vec<Function>,
    pub externals: Vec<ExternalFunction>,
    pub records: Vec<RecordDefinition>,
}

impl Artifact {
    pub const CURRENT_VERSION: u32 = 42;
    pub fn entry(&self) -> &str {
        &self.entry
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, ArtifactError> {
        if !(14..=Self::CURRENT_VERSION).contains(&self.format_version) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                format!("unsupported artifact format {}", self.format_version),
            ));
        }
        if self.format_version < 17 && artifact_uses_bytes(self) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "Bytes values and byte host operations require artifact format 17",
            ));
        }
        if self.format_version < 18 && artifact_uses_python_keywords(self) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "Python keyword calls require artifact format 18",
            ));
        }
        if self.format_version < 19 && artifact_uses_http(self) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "HTTP host calls require artifact format 19",
            ));
        }
        if self.format_version < 20 && artifact_uses_json_construction(self) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "JSON construction operations require artifact format 20",
            ));
        }
        if self.format_version < 21 && artifact_uses_atomic_text_write(self) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "atomic text replacement requires artifact format 21",
            ));
        }
        if self.format_version < 22 && artifact_uses_args_from(self) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "args_from requires artifact format 22",
            ));
        }
        if self.format_version < 23 && artifact_uses_http_request(self) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "general HTTP requests require artifact format 23",
            ));
        }
        if self.format_version < 24 && artifact_uses_environment(self) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "environment access requires artifact format 24",
            ));
        }
        if self.format_version < 25 && artifact_uses_typed_empty_collections(self) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "typed empty collection literals require artifact format 25",
            ));
        }
        if self.format_version < 26 && artifact_uses_list_updates_or_float_sqrt(self) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "immutable list updates and Float64 square root require artifact format 26",
            ));
        }
        if self.format_version < 27 && artifact_uses_http_request_timeout(self) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "per-request HTTP timeouts require artifact format 27",
            ));
        }
        if self.format_version < 28 && artifact_uses_program_stderr(self) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "program stderr output requires artifact format 28",
            ));
        }
        if self.format_version < 29 && artifact_uses_json_columns(self) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "JSON column operations require artifact format 29",
            ));
        }
        if self.format_version < 30 && artifact_uses_json_projection(self) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "streaming JSON column projection requires artifact format 30",
            ));
        }
        if self.format_version < 31 && artifact_uses_text_chars(self) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "text_chars requires artifact format 31",
            ));
        }
        if self.format_version < 32 && artifact_uses_list_push_local(self) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "in-place list push requires artifact format 32",
            ));
        }
        if self.format_version < 33 && artifact_uses_json_push_local(self) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "in-place JSON array push requires artifact format 33",
            ));
        }
        if self.format_version < 34 && artifact_uses_byte_construction(self) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "bounded byte construction operations require artifact format 34",
            ));
        }
        if self.format_version < 35 && artifact_uses_task4_data(self) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "Task 4 data operations require artifact format 35",
            ));
        }
        if self.format_version < 36 && artifact_uses_sort_by(self) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "stable generic sorting requires artifact format 36",
            ));
        }
        if self.format_version < 37 && artifact_uses_task5_services(self) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "Unicode, cryptography, SQLite, TOML, calendar, and native resource types require artifact format 37",
            ));
        }
        if self.format_version < 38 && artifact_uses_http_request_bytes(self) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "byte HTTP requests require artifact format 38",
            ));
        }
        if self.format_version < 39 && artifact_uses_clock(self) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "clock operations require artifact format 39",
            ));
        }
        if self.format_version < 41 && artifact_uses_sort_by_int_key(self) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "stable integer-key sorting requires artifact format 41",
            ));
        }
        if self.format_version < 41 && artifact_uses_merge_sorted_by_int_key(self) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "stable integer-key feed merging requires artifact format 41",
            ));
        }
        if self.format_version < 41 && artifact_uses_json_set_local(self) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "in-place JSON object field updates require artifact format 41",
            ));
        }
        if (self.format_version < 40 || self.format_version == 41)
            && artifact_uses_temporal_resolution(self)
        {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "local time resolution requires artifact format 40 or 42",
            ));
        }
        validate_sort_comparator_effects(self)
            .map_err(|message| ArtifactError::new("E_SORT_EFFECT", message))?;
        let mut w = Writer(Vec::new());
        w.0.extend_from_slice(b"VRTA");
        w.u32(self.format_version);
        w.string(&self.source_name)?;
        w.string(&self.entry)?;
        w.count(self.sources.len())?;
        for source in &self.sources {
            w.string(source)?;
        }
        w.count(self.records.len())?;
        for record in &self.records {
            w.string(&record.name)?;
            w.count(record.fields.len())?;
            for field in &record.fields {
                w.string(&field.name)?;
                w.ty(&field.ty)?;
            }
        }
        w.count(self.externals.len())?;
        for external in &self.externals {
            if external.parameter_names.len() != external.parameters.len() {
                return Err(ArtifactError::new(
                    "E_ARTIFACT_SIGNATURE",
                    "external parameter names and types have different lengths",
                ));
            }
            w.string(&external.name)?;
            w.string(&external.module)?;
            w.string(&external.callable)?;
            w.count(external.parameters.len())?;
            for (name, ty) in external.parameter_names.iter().zip(&external.parameters) {
                w.string(name)?;
                w.ty(ty)?;
            }
            w.ty(&external.result)?;
        }
        w.count(self.functions.len())?;
        for f in &self.functions {
            w.string(&f.name)?;
            w.count(f.parameters.len())?;
            for ty in &f.parameters {
                w.ty(ty)?;
            }
            w.ty(&f.result)?;
            w.count(f.local_count)?;
            w.count(f.code.len())?;
            for instruction in &f.code {
                w.span(instruction.span);
                match &instruction.op {
                    Op::Const(v) => {
                        w.u8(0);
                        w.value(v)?
                    }
                    Op::Load(i) => {
                        w.u8(1);
                        w.count(*i)?
                    }
                    Op::Store(i) => {
                        w.u8(2);
                        w.count(*i)?
                    }
                    Op::Pop => w.u8(3),
                    Op::Add => w.u8(4),
                    Op::Subtract => w.u8(5),
                    Op::Multiply => w.u8(6),
                    Op::Negate => w.u8(7),
                    Op::Not => w.u8(8),
                    Op::Equal => w.u8(9),
                    Op::NotEqual => w.u8(10),
                    Op::Less => w.u8(11),
                    Op::LessEqual => w.u8(12),
                    Op::Greater => w.u8(13),
                    Op::GreaterEqual => w.u8(14),
                    Op::Jump(i) => {
                        w.u8(15);
                        w.count(*i)?
                    }
                    Op::JumpIfFalse(i) => {
                        w.u8(16);
                        w.count(*i)?
                    }
                    Op::Call(i, a) => {
                        w.u8(17);
                        w.count(*i)?;
                        w.count(*a)?
                    }
                    Op::Print => w.u8(18),
                    Op::PrintErr => w.u8(96),
                    Op::Return => w.u8(19),
                    Op::Length => w.u8(20),
                    Op::Index => w.u8(21),
                    Op::MakeList(n) => {
                        w.u8(22);
                        w.count(*n)?;
                    }
                    Op::ParallelMap(function) => {
                        w.u8(23);
                        w.count(*function)?;
                    }
                    Op::ReadText => w.u8(24),
                    Op::WriteText => w.u8(25),
                    Op::WriteTextAtomic => w.u8(85),
                    Op::Args => w.u8(26),
                    Op::Assert => w.u8(27),
                    Op::IsOk => w.u8(28),
                    Op::Unwrap => w.u8(29),
                    Op::GetError => w.u8(30),
                    Op::TaskIndex => w.u8(31),
                    Op::TaskCode => w.u8(32),
                    Op::TaskMessage => w.u8(33),
                    Op::PythonCall(target, argc) => {
                        w.u8(34);
                        w.count(*target)?;
                        w.count(*argc)?;
                    }
                    Op::JsonParse => w.u8(35),
                    Op::JsonStringify => w.u8(36),
                    Op::JsonGet => w.u8(37),
                    Op::JsonIndex => w.u8(38),
                    Op::JsonHas => w.u8(39),
                    Op::JsonLength => w.u8(40),
                    Op::JsonAsText => w.u8(41),
                    Op::JsonAsInt => w.u8(42),
                    Op::JsonAsBool => w.u8(43),
                    Op::JsonIsNull => w.u8(44),
                    Op::JsonErrorMessage => w.u8(45),
                    Op::JsonErrorLine => w.u8(46),
                    Op::JsonErrorColumn => w.u8(47),
                    Op::JsonErrorCode => w.u8(48),
                    Op::FileErrorCode => w.u8(49),
                    Op::FileErrorMessage => w.u8(50),
                    Op::MakeOk => w.u8(51),
                    Op::MakeErr => w.u8(52),
                    Op::TryResult => w.u8(53),
                    Op::FileErrorLine => w.u8(54),
                    Op::FileErrorColumn => w.u8(55),
                    Op::MakeNone => w.u8(56),
                    Op::MakeSome => w.u8(57),
                    Op::IsSome => w.u8(58),
                    Op::MakeMap(n) => {
                        w.u8(59);
                        w.count(*n)?;
                    }
                    Op::MapKeys => w.u8(60),
                    Op::MapSet => w.u8(61),
                    Op::MapHas => w.u8(62),
                    Op::Divide => w.u8(63),
                    Op::JsonAsFloat => w.u8(64),
                    Op::IntegerDivide => w.u8(65),
                    Op::FloorDivide => w.u8(66),
                    Op::Remainder => w.u8(67),
                    Op::MakeRecord(record, count) => {
                        w.u8(68);
                        w.count(*record)?;
                        w.count(*count)?;
                    }
                    Op::GetField(record, field) => {
                        w.u8(69);
                        w.count(*record)?;
                        w.count(*field)?;
                    }
                    Op::PythonCallKw(target, positional_count, names) => {
                        w.u8(72);
                        w.count(*target)?;
                        w.count(*positional_count)?;
                        w.count(names.len())?;
                        for name in names {
                            w.string(name)?;
                        }
                    }
                    Op::ReadBytes => w.u8(70),
                    Op::WriteBytes => w.u8(71),
                    Op::HttpGet => w.u8(73),
                    Op::HttpErrorCode => w.u8(74),
                    Op::HttpErrorMessage => w.u8(75),
                    Op::JsonNull => w.u8(76),
                    Op::JsonBool => w.u8(77),
                    Op::JsonInt => w.u8(78),
                    Op::JsonFloat => w.u8(79),
                    Op::JsonText => w.u8(80),
                    Op::JsonArray => w.u8(81),
                    Op::JsonObject => w.u8(82),
                    Op::JsonPush => w.u8(83),
                    Op::JsonSet => w.u8(84),
                    Op::ArgsFrom => w.u8(86),
                    Op::HttpRequest => w.u8(87),
                    Op::Environment => w.u8(88),
                    Op::MakeTypedList(item) => {
                        w.u8(89);
                        w.ty(item)?;
                    }
                    Op::MakeTypedMap(item) => {
                        w.u8(90);
                        w.ty(item)?;
                    }
                    Op::ListPush => w.u8(91),
                    Op::ListSet => w.u8(92),
                    Op::FloatSqrt => w.u8(93),
                    Op::ListSetLocal(slot) => {
                        w.u8(94);
                        w.count(*slot)?;
                    }
                    Op::HttpRequestWithTimeout => w.u8(95),
                    Op::JsonIntColumn => w.u8(97),
                    Op::JsonTextCategories => w.u8(98),
                    Op::JsonAsIntList => w.u8(99),
                    Op::JsonProjectColumns => w.u8(100),
                    Op::JsonTextCategoriesValues => w.u8(101),
                    Op::TextChars => w.u8(102),
                    Op::ListPushLocal(slot) => {
                        w.u8(103);
                        w.count(*slot)?;
                    }
                    Op::JsonPushLocal(slot) => {
                        w.u8(104);
                        w.count(*slot)?;
                    }
                    Op::JsonSetLocal(slot) => {
                        w.u8(160);
                        w.count(*slot)?;
                    }
                    Op::BytesFromInts => w.u8(105),
                    Op::BytesFromText => w.u8(106),
                    Op::BytesJoin => w.u8(107),
                    Op::Float64Bits => w.u8(108),
                    Op::FileOpenLines => w.u8(109),
                    Op::FileNextLine => w.u8(110),
                    Op::FileClose => w.u8(111),
                    Op::CsvOpen => w.u8(112),
                    Op::CsvNext => w.u8(113),
                    Op::CsvEncode => w.u8(114),
                    Op::JsonTryGet => w.u8(115),
                    Op::JsonTryIndex => w.u8(116),
                    Op::JsonTryText => w.u8(117),
                    Op::JsonTryInt => w.u8(118),
                    Op::JsonTryBool => w.u8(119),
                    Op::JsonTryFloat => w.u8(120),
                    Op::TimeParseDate => w.u8(121),
                    Op::TimeParseTimestamp => w.u8(122),
                    Op::TimeFormatUtc => w.u8(123),
                    Op::SemVerParse => w.u8(124),
                    Op::SemVerCompare => w.u8(125),
                    Op::CliParse => w.u8(126),
                    Op::FsWalk => w.u8(127),
                    Op::ListGetOption => w.u8(128),
                    Op::MapGetOption => w.u8(129),
                    Op::TextParseInt => w.u8(130),
                    Op::TextParseFloat => w.u8(131),
                    Op::SortBy(target) => {
                        w.u8(132);
                        w.count(*target)?;
                    }
                    Op::TextCaseFold => w.u8(133),
                    Op::TextCollapseWhitespace => w.u8(134),
                    Op::TextNormalize => w.u8(135),
                    Op::CryptoHmacSha256 => w.u8(136),
                    Op::CryptoVerifyHmacSha256 => w.u8(137),
                    Op::SqliteOpen => w.u8(138),
                    Op::SqliteBegin => w.u8(139),
                    Op::SqliteExecute => w.u8(140),
                    Op::SqliteQuery => w.u8(141),
                    Op::SqliteCommit => w.u8(142),
                    Op::SqliteRollback => w.u8(143),
                    Op::SqliteClose => w.u8(144),
                    Op::TomlParse => w.u8(145),
                    Op::TomlHas => w.u8(146),
                    Op::TomlGetText => w.u8(147),
                    Op::TomlGetInt => w.u8(148),
                    Op::TomlGetBool => w.u8(149),
                    Op::TomlPutText => w.u8(150),
                    Op::TomlPutInt => w.u8(151),
                    Op::TomlPutBool => w.u8(152),
                    Op::TomlRender => w.u8(153),
                    Op::CalendarParse => w.u8(154),
                    Op::HttpRequestBytes => w.u8(155),
                    Op::UnixTimeMs => w.u8(156),
                    Op::TimeTodayLocal => w.u8(157),
                    Op::SortByIntKey(target) => {
                        w.u8(158);
                        w.count(*target)?;
                    }
                    Op::MergeSortedByIntKey(target) => {
                        w.u8(159);
                        w.count(*target)?;
                    }
                    Op::TimeResolveLocal => {
                        w.u8(if self.format_version == 40 { 158 } else { 161 })
                    }
                    Op::TimeZoneDataVersion => {
                        w.u8(if self.format_version == 40 { 159 } else { 162 })
                    }
                }
            }
        }
        if self.format_version >= 15 {
            match &self.build_metadata {
                Some(metadata) => {
                    w.u8(1);
                    w.string(&metadata.compiler_version)?;
                    w.string(&metadata.target)?;
                    if self.format_version >= 16 {
                        if metadata.metadata_schema_version != 1 {
                            return Err(ArtifactError::new(
                                "E_ARTIFACT_METADATA_SCHEMA",
                                format!(
                                    "unsupported build metadata schema {}",
                                    metadata.metadata_schema_version
                                ),
                            ));
                        }
                        validate_digest(metadata.source_sha256.as_deref(), "source")?;
                        validate_digest(
                            metadata.dependency_lock_sha256.as_deref(),
                            "dependency lock",
                        )?;
                        if metadata.source_sha256.is_none() {
                            return Err(ArtifactError::new(
                                "E_ARTIFACT_METADATA",
                                "provenance build metadata requires a source digest",
                            ));
                        }
                        w.u32(metadata.metadata_schema_version);
                        write_optional_string(&mut w, &metadata.source_sha256)?;
                        write_optional_string(&mut w, &metadata.dependency_lock_sha256)?;
                    }
                    let limits = &metadata.resource_limits;
                    w.u64(limits.instruction_limit);
                    for limit in [
                        limits.allocation_limit_bytes,
                        limits.call_depth_limit,
                        limits.output_limit_bytes,
                        limits.max_integer_digits,
                        limits.max_text_bytes,
                        limits.max_collection_items,
                        limits.max_parallelism,
                        limits.max_file_bytes,
                    ] {
                        w.u64(limit as u64);
                    }
                }
                None => w.u8(0),
            }
        }
        if w.0.len() > MAX_ARTIFACT_BYTES {
            return Err(ArtifactError::new(
                "E_ARTIFACT_SIZE",
                "artifact exceeds the 64 MiB format limit",
            ));
        }
        Ok(w.0)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ArtifactError> {
        if bytes.len() > MAX_ARTIFACT_BYTES {
            return Err(ArtifactError::new(
                "E_ARTIFACT_SIZE",
                "artifact exceeds the 64 MiB format limit",
            ));
        }
        let mut r = Reader { bytes, at: 0 };
        if r.take(4)? != b"VRTA" {
            return Err(ArtifactError::new(
                "E_ARTIFACT_MAGIC",
                "not a Guion artifact",
            ));
        }
        let format_version = r.u32()?;
        if !(14..=Self::CURRENT_VERSION).contains(&format_version) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                format!("unsupported artifact format {format_version}"),
            ));
        }
        let source_name = r.string()?;
        let entry = r.string()?;
        let source_count = r.count()?;
        let mut sources = Vec::with_capacity(source_count);
        for _ in 0..source_count {
            sources.push(r.string()?);
        }
        let nr = r.count()?;
        let mut records = Vec::with_capacity(nr);
        for _ in 0..nr {
            let name = r.string()?;
            let nf = r.count()?;
            let mut fields = Vec::with_capacity(nf);
            for _ in 0..nf {
                fields.push(RecordField {
                    name: r.string()?,
                    ty: r.ty()?,
                });
            }
            records.push(RecordDefinition { name, fields });
        }
        let ne = r.count()?;
        let mut externals = Vec::with_capacity(ne);
        for _ in 0..ne {
            let name = r.string()?;
            let module = r.string()?;
            let callable = r.string()?;
            let np = r.count()?;
            let mut parameter_names = Vec::with_capacity(np);
            let mut parameters = Vec::with_capacity(np);
            for _ in 0..np {
                parameter_names.push(r.string()?);
                parameters.push(r.ty()?);
            }
            let result = r.ty()?;
            externals.push(ExternalFunction {
                name,
                module,
                callable,
                parameter_names,
                parameters,
                result,
            });
        }
        let nf = r.count()?;
        let mut functions = Vec::with_capacity(nf);
        for _ in 0..nf {
            let name = r.string()?;
            let np = r.count()?;
            let mut parameters = Vec::with_capacity(np);
            for _ in 0..np {
                parameters.push(r.ty()?);
            }
            let result = r.ty()?;
            let local_count = r.count()?;
            let ni = r.count()?;
            let mut code = Vec::with_capacity(ni);
            for _ in 0..ni {
                let span = r.span()?;
                let tag = r.u8()?;
                let op = match tag {
                    0 => Op::Const(r.value()?),
                    1 => Op::Load(r.count()?),
                    2 => Op::Store(r.count()?),
                    3 => Op::Pop,
                    4 => Op::Add,
                    5 => Op::Subtract,
                    6 => Op::Multiply,
                    7 => Op::Negate,
                    8 => Op::Not,
                    9 => Op::Equal,
                    10 => Op::NotEqual,
                    11 => Op::Less,
                    12 => Op::LessEqual,
                    13 => Op::Greater,
                    14 => Op::GreaterEqual,
                    15 => Op::Jump(r.count()?),
                    16 => Op::JumpIfFalse(r.count()?),
                    17 => Op::Call(r.count()?, r.count()?),
                    18 => Op::Print,
                    96 => Op::PrintErr,
                    19 => Op::Return,
                    20 => Op::Length,
                    21 => Op::Index,
                    22 => Op::MakeList(r.count()?),
                    23 => Op::ParallelMap(r.count()?),
                    24 => Op::ReadText,
                    25 => Op::WriteText,
                    85 => Op::WriteTextAtomic,
                    26 => Op::Args,
                    27 => Op::Assert,
                    28 => Op::IsOk,
                    29 => Op::Unwrap,
                    30 => Op::GetError,
                    31 => Op::TaskIndex,
                    32 => Op::TaskCode,
                    33 => Op::TaskMessage,
                    34 => Op::PythonCall(r.count()?, r.count()?),
                    35 => Op::JsonParse,
                    36 => Op::JsonStringify,
                    37 => Op::JsonGet,
                    38 => Op::JsonIndex,
                    39 => Op::JsonHas,
                    40 => Op::JsonLength,
                    41 => Op::JsonAsText,
                    42 => Op::JsonAsInt,
                    43 => Op::JsonAsBool,
                    44 => Op::JsonIsNull,
                    45 => Op::JsonErrorMessage,
                    46 => Op::JsonErrorLine,
                    47 => Op::JsonErrorColumn,
                    48 => Op::JsonErrorCode,
                    49 => Op::FileErrorCode,
                    50 => Op::FileErrorMessage,
                    51 => Op::MakeOk,
                    52 => Op::MakeErr,
                    53 => Op::TryResult,
                    54 => Op::FileErrorLine,
                    55 => Op::FileErrorColumn,
                    56 => Op::MakeNone,
                    57 => Op::MakeSome,
                    58 => Op::IsSome,
                    59 => Op::MakeMap(r.count()?),
                    60 => Op::MapKeys,
                    61 => Op::MapSet,
                    62 => Op::MapHas,
                    63 => Op::Divide,
                    64 => Op::JsonAsFloat,
                    65 => Op::IntegerDivide,
                    66 => Op::FloorDivide,
                    67 => Op::Remainder,
                    68 => Op::MakeRecord(r.count()?, r.count()?),
                    69 => Op::GetField(r.count()?, r.count()?),
                    70 => Op::ReadBytes,
                    71 => Op::WriteBytes,
                    72 => {
                        let target = r.count()?;
                        let positional_count = r.count()?;
                        let count = r.count()?;
                        let mut names = Vec::with_capacity(count);
                        for _ in 0..count {
                            names.push(r.string()?);
                        }
                        Op::PythonCallKw(target, positional_count, names)
                    }
                    73 => Op::HttpGet,
                    74 => Op::HttpErrorCode,
                    75 => Op::HttpErrorMessage,
                    76 => Op::JsonNull,
                    77 => Op::JsonBool,
                    78 => Op::JsonInt,
                    79 => Op::JsonFloat,
                    80 => Op::JsonText,
                    81 => Op::JsonArray,
                    82 => Op::JsonObject,
                    83 => Op::JsonPush,
                    84 => Op::JsonSet,
                    86 => Op::ArgsFrom,
                    87 => Op::HttpRequest,
                    88 => Op::Environment,
                    89 => Op::MakeTypedList(r.ty()?),
                    90 => Op::MakeTypedMap(r.ty()?),
                    91 => Op::ListPush,
                    92 => Op::ListSet,
                    93 => Op::FloatSqrt,
                    94 => Op::ListSetLocal(r.count()?),
                    95 => Op::HttpRequestWithTimeout,
                    97 => Op::JsonIntColumn,
                    98 => Op::JsonTextCategories,
                    99 => Op::JsonAsIntList,
                    100 => Op::JsonProjectColumns,
                    101 => Op::JsonTextCategoriesValues,
                    102 => Op::TextChars,
                    103 => Op::ListPushLocal(r.count()?),
                    104 => Op::JsonPushLocal(r.count()?),
                    105 => Op::BytesFromInts,
                    106 => Op::BytesFromText,
                    107 => Op::BytesJoin,
                    108 => Op::Float64Bits,
                    109 => Op::FileOpenLines,
                    110 => Op::FileNextLine,
                    111 => Op::FileClose,
                    112 => Op::CsvOpen,
                    113 => Op::CsvNext,
                    114 => Op::CsvEncode,
                    115 => Op::JsonTryGet,
                    116 => Op::JsonTryIndex,
                    117 => Op::JsonTryText,
                    118 => Op::JsonTryInt,
                    119 => Op::JsonTryBool,
                    120 => Op::JsonTryFloat,
                    121 => Op::TimeParseDate,
                    122 => Op::TimeParseTimestamp,
                    123 => Op::TimeFormatUtc,
                    124 => Op::SemVerParse,
                    125 => Op::SemVerCompare,
                    126 => Op::CliParse,
                    127 => Op::FsWalk,
                    128 => Op::ListGetOption,
                    129 => Op::MapGetOption,
                    130 => Op::TextParseInt,
                    131 => Op::TextParseFloat,
                    132 => Op::SortBy(r.count()?),
                    133 => Op::TextCaseFold,
                    134 => Op::TextCollapseWhitespace,
                    135 => Op::TextNormalize,
                    136 => Op::CryptoHmacSha256,
                    137 => Op::CryptoVerifyHmacSha256,
                    138 => Op::SqliteOpen,
                    139 => Op::SqliteBegin,
                    140 => Op::SqliteExecute,
                    141 => Op::SqliteQuery,
                    142 => Op::SqliteCommit,
                    143 => Op::SqliteRollback,
                    144 => Op::SqliteClose,
                    145 => Op::TomlParse,
                    146 => Op::TomlHas,
                    147 => Op::TomlGetText,
                    148 => Op::TomlGetInt,
                    149 => Op::TomlGetBool,
                    150 => Op::TomlPutText,
                    151 => Op::TomlPutInt,
                    152 => Op::TomlPutBool,
                    153 => Op::TomlRender,
                    154 => Op::CalendarParse,
                    155 => Op::HttpRequestBytes,
                    156 => Op::UnixTimeMs,
                    157 => Op::TimeTodayLocal,
                    158 if format_version == 40 => Op::TimeResolveLocal,
                    159 if format_version == 40 => Op::TimeZoneDataVersion,
                    158 if format_version >= 41 => Op::SortByIntKey(r.count()?),
                    159 if format_version >= 41 => Op::MergeSortedByIntKey(r.count()?),
                    158 | 159 => {
                        return Err(ArtifactError::new(
                            "E_ARTIFACT_VERSION",
                            "artifact uses an instruction newer than its declared format",
                        ))
                    }
                    160 => Op::JsonSetLocal(r.count()?),
                    161 if format_version >= 42 => Op::TimeResolveLocal,
                    162 if format_version >= 42 => Op::TimeZoneDataVersion,
                    _ => {
                        return Err(ArtifactError::new(
                            "E_ARTIFACT_OPCODE",
                            format!("unknown instruction tag {tag}"),
                        ))
                    }
                };
                code.push(Instruction { op, span });
            }
            functions.push(Function {
                name,
                parameters,
                result,
                local_count,
                code,
            });
        }
        let build_metadata = if format_version >= 15 {
            match r.u8()? {
                0 => None,
                1 => {
                    let compiler_version = r.string()?;
                    let target = r.string()?;
                    let (metadata_schema_version, source_sha256, dependency_lock_sha256) =
                        if format_version >= 16 {
                            let schema = r.u32()?;
                            if schema != 1 {
                                return Err(ArtifactError::new(
                                    "E_ARTIFACT_METADATA_SCHEMA",
                                    format!("unsupported build metadata schema {schema}"),
                                ));
                            }
                            let source_digest = read_optional_string(&mut r)?;
                            let dependency_digest = read_optional_string(&mut r)?;
                            validate_digest(source_digest.as_deref(), "source")?;
                            validate_digest(dependency_digest.as_deref(), "dependency lock")?;
                            if source_digest.is_none() {
                                return Err(ArtifactError::new(
                                    "E_ARTIFACT_METADATA",
                                    "provenance build metadata is missing its source digest",
                                ));
                            }
                            (schema, source_digest, dependency_digest)
                        } else {
                            (0, None, None)
                        };
                    Some(BuildMetadata {
                        metadata_schema_version,
                        compiler_version,
                        target,
                        source_sha256,
                        dependency_lock_sha256,
                        resource_limits: ResourceLimits {
                            instruction_limit: r.u64()?,
                            allocation_limit_bytes: r.limit()?,
                            call_depth_limit: r.limit()?,
                            output_limit_bytes: r.limit()?,
                            max_integer_digits: r.limit()?,
                            max_text_bytes: r.limit()?,
                            max_collection_items: r.limit()?,
                            max_parallelism: r.limit()?,
                            max_file_bytes: r.limit()?,
                        },
                    })
                }
                tag => {
                    return Err(ArtifactError::new(
                        "E_ARTIFACT_METADATA",
                        format!("unknown build metadata tag {tag}"),
                    ))
                }
            }
        } else {
            None
        };
        if r.at != bytes.len() {
            return Err(ArtifactError::new(
                "E_ARTIFACT_TRAILING",
                "artifact contains trailing bytes",
            ));
        }
        let artifact = Self {
            format_version,
            compiled_source_sha256: build_metadata
                .as_ref()
                .and_then(|metadata| metadata.source_sha256.clone()),
            build_metadata,
            source_name,
            sources,
            entry,
            functions,
            externals,
            records,
        };
        if format_version < 17 && artifact_uses_bytes(&artifact) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "artifact uses Bytes values or operations before format 17",
            ));
        }
        if format_version < 18 && artifact_uses_python_keywords(&artifact) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "artifact uses Python keyword calls before format 18",
            ));
        }
        if format_version < 19 && artifact_uses_http(&artifact) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "HTTP host calls require artifact format 19",
            ));
        }
        if format_version < 20 && artifact_uses_json_construction(&artifact) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "JSON construction operations require artifact format 20",
            ));
        }
        if format_version < 21 && artifact_uses_atomic_text_write(&artifact) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "atomic text replacement requires artifact format 21",
            ));
        }
        if format_version < 22 && artifact_uses_args_from(&artifact) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "args_from requires artifact format 22",
            ));
        }
        if format_version < 23 && artifact_uses_http_request(&artifact) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "general HTTP requests require artifact format 23",
            ));
        }
        if format_version < 24 && artifact_uses_environment(&artifact) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "environment access requires artifact format 24",
            ));
        }
        if format_version < 25 && artifact_uses_typed_empty_collections(&artifact) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "typed empty collection literals require artifact format 25",
            ));
        }
        if format_version < 26 && artifact_uses_list_updates_or_float_sqrt(&artifact) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "immutable list updates and Float64 square root require artifact format 26",
            ));
        }
        if format_version < 27 && artifact_uses_http_request_timeout(&artifact) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "per-request HTTP timeouts require artifact format 27",
            ));
        }
        if format_version < 28 && artifact_uses_program_stderr(&artifact) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "program stderr output requires artifact format 28",
            ));
        }
        if format_version < 29 && artifact_uses_json_columns(&artifact) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "JSON column operations require artifact format 29",
            ));
        }
        if format_version < 30 && artifact_uses_json_projection(&artifact) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "streaming JSON column projection requires artifact format 30",
            ));
        }
        if format_version < 31 && artifact_uses_text_chars(&artifact) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "text_chars requires artifact format 31",
            ));
        }
        if format_version < 32 && artifact_uses_list_push_local(&artifact) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "in-place list push requires artifact format 32",
            ));
        }
        if format_version < 33 && artifact_uses_json_push_local(&artifact) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "in-place JSON array push requires artifact format 33",
            ));
        }
        if format_version < 34 && artifact_uses_byte_construction(&artifact) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "bounded byte construction operations require artifact format 34",
            ));
        }
        if format_version < 35 && artifact_uses_task4_data(&artifact) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "Task 4 data operations require artifact format 35",
            ));
        }
        if format_version < 36 && artifact_uses_sort_by(&artifact) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "stable generic sorting requires artifact format 36",
            ));
        }
        if format_version < 37 && artifact_uses_task5_services(&artifact) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "artifact uses Task 5 services or native resource types before format 37",
            ));
        }
        if format_version < 38 && artifact_uses_http_request_bytes(&artifact) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "artifact uses byte HTTP requests before format 38",
            ));
        }
        if format_version < 39 && artifact_uses_clock(&artifact) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "artifact uses clock operations before format 39",
            ));
        }
        if format_version < 41 && artifact_uses_sort_by_int_key(&artifact) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "stable integer-key sorting requires artifact format 41",
            ));
        }
        if format_version < 41 && artifact_uses_merge_sorted_by_int_key(&artifact) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "stable integer-key feed merging requires artifact format 41",
            ));
        }
        if format_version < 41 && artifact_uses_json_set_local(&artifact) {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "in-place JSON object field updates require artifact format 41",
            ));
        }
        if (format_version < 40 || format_version == 41)
            && artifact_uses_temporal_resolution(&artifact)
        {
            return Err(ArtifactError::new(
                "E_ARTIFACT_VERSION",
                "local time resolution requires artifact format 40 or 42",
            ));
        }
        validate_sort_comparator_effects(&artifact)
            .map_err(|message| ArtifactError::new("E_SORT_EFFECT", message))?;
        Ok(artifact)
    }
}

fn artifact_uses_bytes(artifact: &Artifact) -> bool {
    fn ty_has_bytes(ty: &Type) -> bool {
        match ty {
            Type::Bytes => true,
            Type::List(item) | Type::Map(item) | Type::Option(item) => ty_has_bytes(item),
            Type::Result(ok, err) => ty_has_bytes(ok) || ty_has_bytes(err),
            _ => false,
        }
    }
    fn value_has_bytes(value: &Value) -> bool {
        match value {
            Value::Bytes(_) => true,
            Value::List(items) | Value::Record(_, items) => items.iter().any(value_has_bytes),
            Value::Map(items) => items.values().any(value_has_bytes),
            Value::OptionSome(value) | Value::ResultOk(value) | Value::ResultErr(value) => {
                value_has_bytes(value)
            }
            _ => false,
        }
    }
    artifact
        .records
        .iter()
        .any(|record| record.fields.iter().any(|field| ty_has_bytes(&field.ty)))
        || artifact.externals.iter().any(|external| {
            external.parameters.iter().any(ty_has_bytes) || ty_has_bytes(&external.result)
        })
        || artifact.functions.iter().any(|function| {
            function.parameters.iter().any(ty_has_bytes)
                || ty_has_bytes(&function.result)
                || function
                    .code
                    .iter()
                    .any(|instruction| match &instruction.op {
                        Op::ReadBytes | Op::WriteBytes => true,
                        Op::Const(value) => value_has_bytes(value),
                        _ => false,
                    })
        })
}

fn artifact_uses_python_keywords(artifact: &Artifact) -> bool {
    artifact.functions.iter().any(|function| {
        function
            .code
            .iter()
            .any(|instruction| matches!(instruction.op, Op::PythonCallKw(_, _, _)))
    })
}

fn artifact_uses_http(artifact: &Artifact) -> bool {
    artifact.functions.iter().any(|function| {
        function.parameters.iter().any(type_has_http_error)
            || type_has_http_error(&function.result)
            || function.code.iter().any(|instruction| {
                matches!(
                    instruction.op,
                    Op::HttpGet | Op::HttpErrorCode | Op::HttpErrorMessage | Op::HttpRequestBytes
                ) || matches!(&instruction.op, Op::Const(Value::HttpError(_)))
            })
    })
}

fn artifact_uses_http_request(artifact: &Artifact) -> bool {
    artifact.functions.iter().any(|function| {
        function.code.iter().any(|instruction| {
            matches!(
                instruction.op,
                Op::HttpRequest | Op::HttpRequestWithTimeout | Op::HttpRequestBytes
            )
        })
    })
}

fn artifact_uses_http_request_timeout(artifact: &Artifact) -> bool {
    artifact.functions.iter().any(|function| {
        function
            .code
            .iter()
            .any(|instruction| matches!(instruction.op, Op::HttpRequestWithTimeout))
    })
}

fn artifact_uses_http_request_bytes(artifact: &Artifact) -> bool {
    artifact.functions.iter().any(|function| {
        function
            .code
            .iter()
            .any(|instruction| matches!(instruction.op, Op::HttpRequestBytes))
    })
}

fn artifact_uses_clock(artifact: &Artifact) -> bool {
    artifact.functions.iter().any(|function| {
        function
            .code
            .iter()
            .any(|instruction| matches!(instruction.op, Op::UnixTimeMs | Op::TimeTodayLocal))
    })
}

fn artifact_uses_temporal_resolution(artifact: &Artifact) -> bool {
    artifact.functions.iter().any(|function| {
        function.code.iter().any(|instruction| {
            matches!(
                instruction.op,
                Op::TimeResolveLocal | Op::TimeZoneDataVersion
            )
        })
    })
}

fn artifact_uses_program_stderr(artifact: &Artifact) -> bool {
    artifact.functions.iter().any(|function| {
        function
            .code
            .iter()
            .any(|instruction| matches!(instruction.op, Op::PrintErr))
    })
}

fn artifact_uses_environment(artifact: &Artifact) -> bool {
    artifact.functions.iter().any(|function| {
        function
            .code
            .iter()
            .any(|instruction| matches!(instruction.op, Op::Environment))
    })
}

fn artifact_uses_typed_empty_collections(artifact: &Artifact) -> bool {
    artifact.functions.iter().any(|function| {
        function
            .code
            .iter()
            .any(|instruction| matches!(instruction.op, Op::MakeTypedList(_) | Op::MakeTypedMap(_)))
    })
}

fn artifact_uses_list_updates_or_float_sqrt(artifact: &Artifact) -> bool {
    artifact.functions.iter().any(|function| {
        function.code.iter().any(|instruction| {
            matches!(
                instruction.op,
                Op::ListPush | Op::ListSet | Op::FloatSqrt | Op::ListSetLocal(_)
            )
        })
    })
}

fn artifact_uses_json_construction(artifact: &Artifact) -> bool {
    artifact.functions.iter().any(|function| {
        function.code.iter().any(|instruction| {
            matches!(
                instruction.op,
                Op::JsonNull
                    | Op::JsonBool
                    | Op::JsonInt
                    | Op::JsonFloat
                    | Op::JsonText
                    | Op::JsonArray
                    | Op::JsonObject
                    | Op::JsonPush
                    | Op::JsonSet
            )
        })
    })
}

fn artifact_uses_json_columns(artifact: &Artifact) -> bool {
    artifact.functions.iter().any(|function| {
        function.code.iter().any(|instruction| {
            matches!(
                instruction.op,
                Op::JsonIntColumn | Op::JsonTextCategories | Op::JsonAsIntList
            )
        })
    })
}

fn artifact_uses_json_projection(artifact: &Artifact) -> bool {
    artifact.functions.iter().any(|function| {
        function.code.iter().any(|instruction| {
            matches!(
                instruction.op,
                Op::JsonProjectColumns | Op::JsonTextCategoriesValues
            )
        })
    })
}

fn artifact_uses_text_chars(artifact: &Artifact) -> bool {
    artifact.functions.iter().any(|function| {
        function
            .code
            .iter()
            .any(|instruction| matches!(instruction.op, Op::TextChars))
    })
}

fn artifact_uses_list_push_local(artifact: &Artifact) -> bool {
    artifact.functions.iter().any(|function| {
        function
            .code
            .iter()
            .any(|instruction| matches!(instruction.op, Op::ListPushLocal(_)))
    })
}

fn artifact_uses_json_push_local(artifact: &Artifact) -> bool {
    artifact.functions.iter().any(|function| {
        function
            .code
            .iter()
            .any(|instruction| matches!(instruction.op, Op::JsonPushLocal(_)))
    })
}

fn artifact_uses_json_set_local(artifact: &Artifact) -> bool {
    artifact.functions.iter().any(|function| {
        function
            .code
            .iter()
            .any(|instruction| matches!(instruction.op, Op::JsonSetLocal(_)))
    })
}

fn artifact_uses_byte_construction(artifact: &Artifact) -> bool {
    artifact.functions.iter().any(|function| {
        function.code.iter().any(|instruction| {
            matches!(
                instruction.op,
                Op::BytesFromInts | Op::BytesFromText | Op::BytesJoin | Op::Float64Bits
            )
        })
    })
}

fn artifact_uses_task4_data(artifact: &Artifact) -> bool {
    artifact.functions.iter().any(|function| {
        function.code.iter().any(|instruction| {
            matches!(
                instruction.op,
                Op::FileOpenLines
                    | Op::FileNextLine
                    | Op::FileClose
                    | Op::CsvOpen
                    | Op::CsvNext
                    | Op::CsvEncode
                    | Op::JsonTryGet
                    | Op::JsonTryIndex
                    | Op::JsonTryText
                    | Op::JsonTryInt
                    | Op::JsonTryBool
                    | Op::JsonTryFloat
                    | Op::TimeParseDate
                    | Op::TimeParseTimestamp
                    | Op::TimeFormatUtc
                    | Op::SemVerParse
                    | Op::SemVerCompare
                    | Op::CliParse
                    | Op::FsWalk
                    | Op::ListGetOption
                    | Op::MapGetOption
                    | Op::TextParseInt
                    | Op::TextParseFloat
            )
        })
    })
}

fn artifact_uses_sort_by(artifact: &Artifact) -> bool {
    artifact.functions.iter().any(|function| {
        function
            .code
            .iter()
            .any(|instruction| matches!(instruction.op, Op::SortBy(_)))
    })
}

fn artifact_uses_sort_by_int_key(artifact: &Artifact) -> bool {
    artifact.functions.iter().any(|function| {
        function
            .code
            .iter()
            .any(|instruction| matches!(instruction.op, Op::SortByIntKey(_)))
    })
}

fn artifact_uses_merge_sorted_by_int_key(artifact: &Artifact) -> bool {
    artifact.functions.iter().any(|function| {
        function
            .code
            .iter()
            .any(|instruction| matches!(instruction.op, Op::MergeSortedByIntKey(_)))
    })
}

fn artifact_uses_task5_services(artifact: &Artifact) -> bool {
    fn has_resource_type(ty: &Type) -> bool {
        match ty {
            Type::NativeResource(_) => true,
            Type::List(item) | Type::Map(item) | Type::Option(item) => has_resource_type(item),
            Type::Result(ok, err) => has_resource_type(ok) || has_resource_type(err),
            _ => false,
        }
    }

    artifact.records.iter().any(|record| {
        record
            .fields
            .iter()
            .any(|field| has_resource_type(&field.ty))
    }) || artifact.externals.iter().any(|external| {
        external.parameters.iter().any(has_resource_type) || has_resource_type(&external.result)
    }) || artifact.functions.iter().any(|function| {
        function.parameters.iter().any(has_resource_type)
            || has_resource_type(&function.result)
            || function.code.iter().any(|instruction| {
                matches!(
                    instruction.op,
                    Op::TextCaseFold
                        | Op::TextCollapseWhitespace
                        | Op::TextNormalize
                        | Op::CryptoHmacSha256
                        | Op::CryptoVerifyHmacSha256
                        | Op::SqliteOpen
                        | Op::SqliteBegin
                        | Op::SqliteExecute
                        | Op::SqliteQuery
                        | Op::SqliteCommit
                        | Op::SqliteRollback
                        | Op::SqliteClose
                        | Op::TomlParse
                        | Op::TomlHas
                        | Op::TomlGetText
                        | Op::TomlGetInt
                        | Op::TomlGetBool
                        | Op::TomlPutText
                        | Op::TomlPutInt
                        | Op::TomlPutBool
                        | Op::TomlRender
                        | Op::CalendarParse
                )
            })
    })
}

/// Validates that each named sort comparator/key function and its transitive call graph are pure.
pub fn validate_sort_comparator_effects(artifact: &Artifact) -> Result<(), String> {
    for caller in &artifact.functions {
        for instruction in &caller.code {
            let root = match instruction.op {
                Op::SortBy(root) | Op::SortByIntKey(root) | Op::MergeSortedByIntKey(root) => root,
                _ => continue,
            };
            let mut pending = vec![root];
            let mut seen = HashSet::new();
            while let Some(index) = pending.pop() {
                if !seen.insert(index) {
                    continue;
                }
                let Some(callee) = artifact.functions.get(index) else {
                    return Err(format!(
                        "sort comparator references missing function {index}"
                    ));
                };
                for instruction in &callee.code {
                    match instruction.op {
                        Op::Call(target, _)
                        | Op::SortBy(target)
                        | Op::SortByIntKey(target)
                        | Op::MergeSortedByIntKey(target) => pending.push(target),
                        Op::Print
                        | Op::PrintErr
                        | Op::ReadText
                        | Op::WriteText
                        | Op::WriteTextAtomic
                        | Op::ReadBytes
                        | Op::WriteBytes
                        | Op::Args
                        | Op::ArgsFrom
                        | Op::Environment
                        | Op::PythonCall(_, _)
                        | Op::PythonCallKw(_, _, _)
                        | Op::HttpGet
                        | Op::HttpRequest
                        | Op::HttpRequestWithTimeout
                        | Op::FileOpenLines
                        | Op::FileNextLine
                        | Op::FileClose
                        | Op::CsvOpen
                        | Op::CsvNext
                        | Op::FsWalk
                        | Op::ParallelMap(_) => {
                            return Err(format!(
                                "sort comparator `{}` reaches an effectful operation",
                                callee.name
                            ));
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    Ok(())
}

fn artifact_uses_atomic_text_write(artifact: &Artifact) -> bool {
    artifact.functions.iter().any(|function| {
        function
            .code
            .iter()
            .any(|instruction| matches!(instruction.op, Op::WriteTextAtomic))
    })
}

fn artifact_uses_args_from(artifact: &Artifact) -> bool {
    artifact.functions.iter().any(|function| {
        function
            .code
            .iter()
            .any(|instruction| matches!(instruction.op, Op::ArgsFrom))
    })
}

fn type_has_http_error(ty: &Type) -> bool {
    match ty {
        Type::HttpError => true,
        Type::List(item) | Type::Map(item) | Type::Option(item) => type_has_http_error(item),
        Type::Result(ok, err) => type_has_http_error(ok) || type_has_http_error(err),
        _ => false,
    }
}

const MAX_ARTIFACT_BYTES: usize = 64 * 1024 * 1024;
const MAX_ITEMS: usize = 1_000_000;

fn write_optional_string(w: &mut Writer, value: &Option<String>) -> Result<(), ArtifactError> {
    match value {
        Some(value) => {
            w.u8(1);
            w.string(value)?;
        }
        None => w.u8(0),
    }
    Ok(())
}

fn read_optional_string(r: &mut Reader<'_>) -> Result<Option<String>, ArtifactError> {
    match r.u8()? {
        0 => Ok(None),
        1 => Ok(Some(r.string()?)),
        tag => Err(ArtifactError::new(
            "E_ARTIFACT_METADATA",
            format!("unknown optional-string tag {tag}"),
        )),
    }
}

fn validate_digest(digest: Option<&str>, label: &str) -> Result<(), ArtifactError> {
    if digest.is_some_and(|value| {
        value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
    }) {
        return Err(ArtifactError::new(
            "E_ARTIFACT_METADATA",
            format!("{label} digest must be 64 hexadecimal characters"),
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactError {
    pub code: String,
    pub message: String,
}
impl ArtifactError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}
struct Writer(Vec<u8>);
impl Writer {
    fn u8(&mut self, n: u8) {
        self.0.push(n)
    }
    fn u32(&mut self, n: u32) {
        self.0.extend_from_slice(&n.to_le_bytes())
    }
    fn count(&mut self, n: usize) -> Result<(), ArtifactError> {
        if n > MAX_ITEMS {
            return Err(ArtifactError::new(
                "E_ARTIFACT_SIZE",
                "artifact collection exceeds item limit",
            ));
        }
        self.u32(n as u32);
        Ok(())
    }
    fn string(&mut self, s: &str) -> Result<(), ArtifactError> {
        self.count(s.len())?;
        self.0.extend_from_slice(s.as_bytes());
        Ok(())
    }
    fn blob(&mut self, bytes: &[u8]) -> Result<(), ArtifactError> {
        if bytes.len() > MAX_ARTIFACT_BYTES || bytes.len() > u32::MAX as usize {
            return Err(ArtifactError::new(
                "E_ARTIFACT_SIZE",
                "artifact byte value exceeds format limit",
            ));
        }
        self.u32(bytes.len() as u32);
        self.0.extend_from_slice(bytes);
        Ok(())
    }
    fn ty(&mut self, t: &Type) -> Result<(), ArtifactError> {
        match t {
            Type::Int => self.u8(0),
            Type::Bool => self.u8(1),
            Type::Text => self.u8(2),
            Type::Unit => self.u8(3),
            Type::List(item) => {
                self.u8(4);
                self.ty(item)?;
            }
            Type::Option(item) => {
                self.u8(10);
                self.ty(item)?;
            }
            Type::Map(item) => {
                self.u8(11);
                self.ty(item)?;
            }
            Type::Float64 => self.u8(12),
            Type::Record(name) => {
                self.u8(13);
                self.string(name)?;
            }
            Type::Bytes => self.u8(14),
            Type::Result(ok, err) => {
                self.u8(5);
                self.ty(ok)?;
                self.ty(err)?;
            }
            Type::TaskError => self.u8(6),
            Type::Json => self.u8(7),
            Type::JsonError => self.u8(8),
            Type::FileError => self.u8(9),
            Type::HttpError => self.u8(15),
            Type::NativeResource(name) => {
                self.u8(17);
                self.string(name)?;
            }
        }
        Ok(())
    }
    fn span(&mut self, s: Span) {
        self.u32(s.source_id);
        for v in [s.start, s.end, s.line, s.column] {
            self.u64(v as u64)
        }
    }
    fn u64(&mut self, n: u64) {
        self.0.extend_from_slice(&n.to_le_bytes())
    }
    fn value(&mut self, v: &Value) -> Result<(), ArtifactError> {
        self.value_at(v, 0)
    }
    fn value_at(&mut self, v: &Value, depth: usize) -> Result<(), ArtifactError> {
        if depth > 64 {
            return Err(ArtifactError::new(
                "E_ARTIFACT_NESTING",
                "value nesting exceeds 64 levels",
            ));
        }
        match v {
            Value::Int(i) => {
                self.u8(0);
                self.string(&i.to_string())?
            }
            Value::Bool(b) => {
                self.u8(1);
                self.u8(u8::from(*b))
            }
            Value::Text(s) => {
                self.u8(2);
                self.string(s)?
            }
            Value::Unit => self.u8(3),
            Value::List(items) => {
                self.u8(4);
                self.count(items.len())?;
                for item in items.iter() {
                    self.value_at(item, depth + 1)?;
                }
            }
            Value::Float64List(items) => {
                self.u8(4);
                self.count(items.len())?;
                for item in items.iter() {
                    self.value_at(&Value::Float64(*item), depth + 1)?;
                }
            }
            Value::ResultOk(value) => {
                self.u8(5);
                self.value_at(value, depth + 1)?;
            }
            Value::ResultErr(value) => {
                self.u8(6);
                self.value_at(value, depth + 1)?;
            }
            Value::TaskError(error) => {
                self.u8(7);
                self.string(&error.index.to_string())?;
                self.string(&error.code)?;
                self.string(&error.message)?;
                self.count(error.trace.len())?;
                for span in &error.trace {
                    self.span(*span);
                }
            }
            Value::Json(value) => {
                self.u8(8);
                let encoded = serde_json::to_string(&value.to_serde()).map_err(|error| {
                    ArtifactError::new(
                        "E_ARTIFACT_JSON",
                        format!("cannot encode JSON value: {error}"),
                    )
                })?;
                self.string(&encoded)?;
            }
            Value::JsonError(error) => {
                self.u8(9);
                self.string(&error.code)?;
                self.string(&error.message)?;
                self.count(error.line)?;
                self.count(error.column)?;
            }
            Value::FileError(error) => {
                self.u8(10);
                self.string(&error.code)?;
                self.string(&error.message)?;
                self.count(error.trace.len())?;
                for span in &error.trace {
                    self.span(*span);
                }
            }
            Value::OptionNone => self.u8(11),
            Value::OptionSome(value) => {
                self.u8(12);
                self.value_at(value, depth + 1)?;
            }
            Value::Map(items) => {
                self.u8(13);
                self.count(items.len())?;
                for (key, value) in items.iter() {
                    self.string(key)?;
                    self.value_at(value, depth + 1)?;
                }
            }
            Value::Float64(value) => {
                self.u8(14);
                self.u64(value.get().to_bits());
            }
            Value::Record(name, fields) => {
                self.u8(15);
                self.string(name)?;
                self.count(fields.len())?;
                for field in fields.iter() {
                    self.value_at(field, depth + 1)?;
                }
            }
            Value::Bytes(bytes) => {
                self.u8(16);
                self.blob(bytes)?;
            }
            Value::HttpError(error) => {
                self.u8(17);
                self.string(&error.code)?;
                self.string(&error.message)?;
                self.count(error.trace.len())?;
                for span in &error.trace {
                    self.span(*span);
                }
            }
            Value::NativeResource(_) => {
                return Err(ArtifactError::new(
                    "E_ARTIFACT_RESOURCE",
                    "native resource handles cannot be serialized as constants",
                ))
            }
        }
        Ok(())
    }
}
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], ArtifactError> {
        let end = self
            .at
            .checked_add(n)
            .ok_or_else(|| ArtifactError::new("E_ARTIFACT_SIZE", "artifact length overflow"))?;
        if end > self.bytes.len() {
            return Err(ArtifactError::new(
                "E_ARTIFACT_TRUNCATED",
                "artifact ended unexpectedly",
            ));
        }
        let out = &self.bytes[self.at..end];
        self.at = end;
        Ok(out)
    }
    fn u8(&mut self) -> Result<u8, ArtifactError> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, ArtifactError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, ArtifactError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn limit(&mut self) -> Result<usize, ArtifactError> {
        usize::try_from(self.u64()?)
            .map_err(|_| ArtifactError::new("E_ARTIFACT_LIMIT", "resource limit exceeds host size"))
    }
    fn count(&mut self) -> Result<usize, ArtifactError> {
        let n = self.u32()? as usize;
        if n > MAX_ITEMS {
            return Err(ArtifactError::new(
                "E_ARTIFACT_SIZE",
                "artifact collection exceeds item limit",
            ));
        }
        Ok(n)
    }
    fn string(&mut self) -> Result<String, ArtifactError> {
        let n = self.count()?;
        String::from_utf8(self.take(n)?.to_vec())
            .map_err(|_| ArtifactError::new("E_ARTIFACT_UTF8", "artifact contains invalid UTF-8"))
    }
    fn blob(&mut self) -> Result<Vec<u8>, ArtifactError> {
        let n = self.u32()? as usize;
        if n > MAX_ARTIFACT_BYTES {
            return Err(ArtifactError::new(
                "E_ARTIFACT_SIZE",
                "artifact byte value exceeds format limit",
            ));
        }
        Ok(self.take(n)?.to_vec())
    }
    fn ty(&mut self) -> Result<Type, ArtifactError> {
        self.ty_at(0)
    }
    fn ty_at(&mut self, depth: usize) -> Result<Type, ArtifactError> {
        if depth > 64 {
            return Err(ArtifactError::new(
                "E_ARTIFACT_NESTING",
                "type nesting exceeds 64 levels",
            ));
        }
        match self.u8()? {
            0 => Ok(Type::Int),
            1 => Ok(Type::Bool),
            2 => Ok(Type::Text),
            3 => Ok(Type::Unit),
            4 => Ok(Type::List(Box::new(self.ty_at(depth + 1)?))),
            5 => Ok(Type::Result(
                Box::new(self.ty_at(depth + 1)?),
                Box::new(self.ty_at(depth + 1)?),
            )),
            6 => Ok(Type::TaskError),
            7 => Ok(Type::Json),
            8 => Ok(Type::JsonError),
            9 => Ok(Type::FileError),
            10 => Ok(Type::Option(Box::new(self.ty_at(depth + 1)?))),
            11 => Ok(Type::Map(Box::new(self.ty_at(depth + 1)?))),
            12 => Ok(Type::Float64),
            13 => Ok(Type::Record(self.string()?)),
            14 => Ok(Type::Bytes),
            15 => Ok(Type::HttpError),
            17 => Ok(Type::NativeResource(self.string()?)),
            _ => Err(ArtifactError::new("E_ARTIFACT_TYPE", "unknown type tag")),
        }
    }
    fn span(&mut self) -> Result<Span, ArtifactError> {
        Ok(Span {
            source_id: self.u32()?,
            start: self.u64()? as usize,
            end: self.u64()? as usize,
            line: self.u64()? as usize,
            column: self.u64()? as usize,
        })
    }
    fn value(&mut self) -> Result<Value, ArtifactError> {
        self.value_at(0)
    }
    fn value_at(&mut self, depth: usize) -> Result<Value, ArtifactError> {
        if depth > 64 {
            return Err(ArtifactError::new(
                "E_ARTIFACT_NESTING",
                "value nesting exceeds 64 levels",
            ));
        }
        match self.u8()? {
            0 => BigInt::parse(&self.string()?)
                .map(Value::Int)
                .map_err(|_| ArtifactError::new("E_ARTIFACT_INT", "invalid integer encoding")),
            1 => match self.u8()? {
                0 => Ok(Value::Bool(false)),
                1 => Ok(Value::Bool(true)),
                _ => Err(ArtifactError::new(
                    "E_ARTIFACT_BOOL",
                    "invalid boolean encoding",
                )),
            },
            2 => Ok(Value::Text(self.string()?.into())),
            3 => Ok(Value::Unit),
            4 => {
                let n = self.count()?;
                let mut items = Vec::with_capacity(n);
                for _ in 0..n {
                    items.push(self.value_at(depth + 1)?);
                }
                Ok(Value::from_list_items(items))
            }
            5 => Ok(Value::ResultOk(Box::new(self.value_at(depth + 1)?))),
            6 => Ok(Value::ResultErr(Box::new(self.value_at(depth + 1)?))),
            7 => {
                let index = BigInt::parse(&self.string()?)
                    .map_err(|_| ArtifactError::new("E_ARTIFACT_INT", "invalid task index"))?;
                let code = self.string()?;
                let message = self.string()?;
                let n = self.count()?;
                let mut trace = Vec::with_capacity(n);
                for _ in 0..n {
                    trace.push(self.span()?);
                }
                Ok(Value::TaskError(TaskErrorValue {
                    index,
                    code,
                    message,
                    trace,
                }))
            }
            8 => serde_json::from_str(&self.string()?)
                .map(SharedJson::from_serde)
                .map(Value::Json)
                .map_err(|_| ArtifactError::new("E_ARTIFACT_JSON", "invalid JSON value encoding")),
            9 => Ok(Value::JsonError(JsonErrorValue {
                code: self.string()?,
                message: self.string()?,
                line: self.count()?,
                column: self.count()?,
            })),
            10 => Ok(Value::FileError(FileErrorValue {
                code: self.string()?,
                message: self.string()?,
                trace: {
                    let count = self.count()?;
                    let mut trace = Vec::with_capacity(count);
                    for _ in 0..count {
                        trace.push(self.span()?);
                    }
                    trace
                },
            })),
            11 => Ok(Value::OptionNone),
            12 => Ok(Value::OptionSome(Box::new(self.value_at(depth + 1)?))),
            13 => {
                let n = self.count()?;
                let mut items = BTreeMap::new();
                for _ in 0..n {
                    let key = self.string()?;
                    let value = self.value_at(depth + 1)?;
                    if items.insert(key, value).is_some() {
                        return Err(ArtifactError::new("E_ARTIFACT_MAP", "duplicate map key"));
                    }
                }
                Ok(Value::Map(Arc::new(items)))
            }
            14 => {
                let value = f64::from_bits(self.u64()?);
                Float64::new(value).map(Value::Float64).ok_or_else(|| {
                    ArtifactError::new("E_ARTIFACT_FLOAT", "non-finite Float64 encoding")
                })
            }
            15 => {
                let name = self.string()?;
                let count = self.count()?;
                let mut fields = Vec::with_capacity(count);
                for _ in 0..count {
                    fields.push(self.value_at(depth + 1)?);
                }
                Ok(Value::Record(name, Arc::new(fields)))
            }
            16 => Ok(Value::Bytes(Arc::new(self.blob()?))),
            17 => Ok(Value::HttpError(HttpErrorValue {
                code: self.string()?,
                message: self.string()?,
                trace: {
                    let count = self.count()?;
                    let mut trace = Vec::with_capacity(count);
                    for _ in 0..count {
                        trace.push(self.span()?);
                    }
                    trace
                },
            })),
            _ => Err(ArtifactError::new("E_ARTIFACT_VALUE", "unknown value tag")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeError {
    pub code: String,
    pub message: String,
    pub trace: Vec<Span>,
}

impl RuntimeError {
    pub fn new(code: impl Into<String>, message: impl Into<String>, span: Span) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            trace: vec![span],
        }
    }
}
