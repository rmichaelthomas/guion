use guion_compiler::compile_project;
use guion_model::{Artifact, Value};
use guion_runtime::{HostPolicy, Instance, Runtime};
use std::{fs, path::PathBuf, thread};

fn compiler_output(runtime: &mut Instance, entry: &str, source: &str) -> Vec<u8> {
    let result = runtime
        .invoke(
            "compiler_compile_project",
            vec![
                Value::Text(entry.into()),
                Value::Text(source.into()),
                Value::Text(String::new()),
                Value::Json(guion_model::SharedJson::from_serde(serde_json::json!({}))),
            ],
        )
        .expect("Guion compiler entry returns a Result value");
    match result {
        Value::ResultOk(bytes) => match *bytes {
            Value::Bytes(bytes) => bytes.as_ref().clone(),
            value => panic!("compiler success must contain Bytes, got {value:?}"),
        },
        Value::ResultErr(error) => panic!("Guion compiler rejected its source: {error:?}"),
        value => panic!("compiler entry must return Result, got {value:?}"),
    }
}

#[test]
fn compiler_stage_equality() {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("resolve repository root");
    thread::Builder::new()
        .name("guion-three-stage-bootstrap".into())
        .spawn(move || {
            let compiler_entry = "language/self_host/compiler.gu";
            let compiler_source = fs::read_to_string(repository.join(compiler_entry))
                .expect("read self-hosted compiler source");

            // C1 is the checked-in Guion compiler source compiled by the
            // explicit stage-zero Rust reference compiler.
            let c1 = compile_project(&repository.join(compiler_entry), &repository)
                .expect("stage zero builds C1");
            let policy = HostPolicy {
                instruction_limit: 500_000_000,
                allocation_limit_bytes: 8_589_934_592,
                call_depth_limit: 160,
                max_collection_items: 1_000_000,
                ..HostPolicy::default().with_file_access(&repository, true, false)
            };
            let mut c1_runtime = Runtime::new()
                .load(c1, policy.clone())
                .expect("native verifier accepts C1");

            // C1 compiles the complete compiler to C2. The returned bytes are
            // immediately parsed and verified by the native artifact reader.
            let c2_bytes = compiler_output(&mut c1_runtime, compiler_entry, &compiler_source);
            let c2 = Artifact::from_bytes(&c2_bytes).expect("native reader accepts C2");
            let mut c2_runtime = Runtime::new()
                .load(c2, policy)
                .expect("native verifier accepts C2");

            // C2 recompiles the exact same relative source graph to C3. No
            // root normalization is needed because the source identity is
            // relative and build metadata is absent in both stages.
            let c3_bytes = compiler_output(&mut c2_runtime, compiler_entry, &compiler_source);
            let c3 = Artifact::from_bytes(&c3_bytes).expect("native reader accepts C3");
            assert_eq!(
                c2_bytes, c3_bytes,
                "C2 and C3 canonical artifact bytes differ"
            );
            Runtime::new()
                .load(c3, HostPolicy::default())
                .expect("native verifier accepts C3");
            let stage_directory = repository.join("target/self-hosted-bootstrap");
            fs::create_dir_all(&stage_directory).expect("create reproducible stage cache");
            fs::write(stage_directory.join("stage2.gub"), &c2_bytes)
                .expect("save verified C2 for conformance");
            fs::write(stage_directory.join("stage3.gub"), &c3_bytes)
                .expect("save verified C3 for conformance");
        })
        .expect("start bootstrap worker")
        .join()
        .expect("three-stage bootstrap completes without a host stack failure");
}
