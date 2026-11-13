//! `crypto_random_bytes`, `crypto_random_hex` and `crypto_random_base64url` (artifact format 68):
//! OS entropy, the 1 MiB ceiling, typed errors, and the embedding-only pinned source.

use guion_model::Value;
use guion_runtime::stdlib_services::crypto_hmac_sha256;
use guion_runtime::{EntropySource, HostPolicy, Runtime};
use std::collections::HashSet;

const MAX_BYTES: i64 = 1_048_576;

const PROGRAM: &str = r#"import "std:crypto"
import "std:encoding"
import "std:random"
import "std:text"
import "std:types"

fn main() -> Int:
    return 0

fn draw(count: Int) -> Result[Bytes, StdError]:
    return crypto_random_bytes(count)

fn draw_hex(count: Int) -> Result[Text, StdError]:
    return crypto_random_hex(count)

fn draw_base64url(count: Int) -> Result[Text, StdError]:
    return crypto_random_base64url(count)

fn seeded_bytes(seed: Int) -> Bytes:
    var rng = random_new(seed)
    var out: List[Int] = []
    var index = 0
    while index < 32:
        let step = random_next_i64(rng)
        set rng = step.rng
        set out = list_push(out, step.value % 256)
        set index = index + 1
    return bytes_from_ints(out)

fn seeded_sequence(seed: Int) -> List[Int]:
    var rng = random_new(seed)
    var out: List[Int] = []
    var index = 0
    while index < 8:
        let step = random_next_i64(rng)
        set rng = step.rng
        set out = list_push(out, step.value)
        set index = index + 1
    return out

fn max_bytes() -> Int:
    return crypto_random_max_bytes()

fn session_token(key: Bytes) -> Text:
    let nonce = unwrap(crypto_random_hex(32))
    let tag = hex_encode(crypto_hmac_sha256(key, bytes_from_text(nonce)))
    return nonce + "." + tag

fn verify_token(key: Bytes, token: Text) -> Bool:
    let parts = text_split(token, ".")
    if len(parts) != 2:
        return false
    match hex_decode(parts[1]):
        Ok(tag):
            return crypto_verify_hmac_sha256(key, bytes_from_text(parts[0]), tag)
        Err(problem):
            return false
"#;

fn instance(policy: HostPolicy) -> guion_runtime::Instance {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "guion-entropy-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&root).unwrap();
    let entry = root.join("main.gu");
    std::fs::write(&entry, PROGRAM).unwrap();
    let stdlib = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib");
    let artifact = guion_compiler::compile_project_with_roots(
        &entry,
        &root,
        Some(&stdlib),
        &std::collections::HashMap::new(),
    )
    .unwrap_or_else(|error| panic!("{} {}", error.code, error.message));
    let _ = std::fs::remove_dir_all(&root);
    Runtime::vm_only()
        .load(artifact, policy)
        .expect("artifact loads")
}

fn call(instance: &mut guion_runtime::Instance, name: &str, args: Vec<Value>) -> Value {
    instance
        .invoke(name, args)
        .unwrap_or_else(|error| panic!("{}: {}", error.code, error.message))
}

fn int(value: i64) -> Value {
    Value::Int(guion_model::BigInt::from_i64(value))
}

fn bytes_of(value: Value) -> Vec<u8> {
    match value {
        Value::ResultOk(inner) => match *inner {
            Value::Bytes(bytes) => bytes.to_vec(),
            other => panic!("expected Bytes, got {other:?}"),
        },
        other => panic!("expected Ok(Bytes), got {other:?}"),
    }
}

fn text_of(value: Value) -> String {
    match value {
        Value::ResultOk(inner) => match *inner {
            Value::Text(text) => text.as_ref().to_owned(),
            other => panic!("expected Text, got {other:?}"),
        },
        other => panic!("expected Ok(Text), got {other:?}"),
    }
}

/// The code of an `Err(StdError)` result.
fn error_code(value: Value) -> String {
    match value {
        Value::ResultErr(inner) => match *inner {
            Value::Record(name, fields) => {
                assert_eq!(&*name, "StdError");
                match &fields[0] {
                    Value::Text(code) => code.as_ref().to_owned(),
                    other => panic!("expected a Text code, got {other:?}"),
                }
            }
            other => panic!("expected a StdError, got {other:?}"),
        },
        other => panic!("expected Err(StdError), got {other:?}"),
    }
}

fn draw(instance: &mut guion_runtime::Instance, count: i64) -> Value {
    call(instance, "draw", vec![int(count)])
}

#[test]
fn a_thousand_draws_of_32_bytes_are_all_distinct_and_consecutive_calls_differ() {
    let mut vm = instance(HostPolicy::default());
    let first = bytes_of(draw(&mut vm, 32));
    let second = bytes_of(draw(&mut vm, 32));
    assert_eq!(first.len(), 32);
    assert_ne!(first, second);
    let mut seen = HashSet::new();
    seen.insert(first);
    seen.insert(second);
    for _ in 0..998 {
        assert!(seen.insert(bytes_of(draw(&mut vm, 32))), "a draw repeated");
    }
    assert_eq!(seen.len(), 1000);
}

#[test]
fn output_is_not_the_deterministic_generator_stream() {
    let mut vm = instance(HostPolicy::default());
    let drawn = bytes_of(draw(&mut vm, 32));
    for seed in [0, 1, 42] {
        let seeded = bytes_of(Value::ResultOk(Box::new(call(
            &mut vm,
            "seeded_bytes",
            vec![int(seed)],
        ))));
        assert_ne!(drawn, seeded, "matched the random_new({seed}) stream");
    }
}

#[test]
fn count_zero_is_empty_and_out_of_range_counts_are_typed_errors() {
    let mut vm = instance(HostPolicy::default());
    assert!(bytes_of(draw(&mut vm, 0)).is_empty());
    assert_eq!(error_code(draw(&mut vm, -1)), "E_RANDOM_COUNT");
    assert_eq!(error_code(draw(&mut vm, MAX_BYTES + 1)), "E_RANDOM_COUNT");
    assert_eq!(error_code(draw(&mut vm, i64::MIN)), "E_RANDOM_COUNT");
    assert_eq!(error_code(draw(&mut vm, i64::MAX)), "E_RANDOM_COUNT");
    assert_eq!(bytes_of(draw(&mut vm, MAX_BYTES)).len(), MAX_BYTES as usize);
    assert_eq!(
        error_code(call(&mut vm, "draw_hex", vec![int(-1)])),
        "E_RANDOM_COUNT"
    );
    assert_eq!(
        error_code(call(&mut vm, "draw_base64url", vec![int(MAX_BYTES + 1)])),
        "E_RANDOM_COUNT"
    );
    assert_eq!(call(&mut vm, "max_bytes", vec![]), int(MAX_BYTES));
}

/// Over 1 MiB drawn in 64 KiB chunks:
///
/// * No 16-byte block repeats. With 65,536 blocks the chance of a chance collision among uniform
///   128-bit values is about 2^32 / 2^128, which is zero for any practical purpose.
/// * Every byte value occurs between 3,700 and 4,500 times. Each count is Binomial(1,048,576,
///   1/256): mean 4,096, standard deviation about 63.9. The band is the mean plus or minus about
///   400, roughly 6.3 standard deviations, so one value falls outside it with probability near
///   3e-10 and any of the 256 values with probability near 8e-8 per run. A counting bug (a
///   repeated chunk, a stuck byte, a biased reduction) leaves the band by a wide margin.
#[test]
fn a_mebibyte_has_no_repeated_block_and_every_byte_value_in_the_band() {
    let mut vm = instance(HostPolicy::default());
    let mut all = Vec::with_capacity(MAX_BYTES as usize);
    for _ in 0..16 {
        all.extend(bytes_of(draw(&mut vm, 65_536)));
    }
    assert_eq!(all.len(), MAX_BYTES as usize);
    let mut blocks = HashSet::new();
    for block in all.chunks_exact(16) {
        assert!(blocks.insert(block.to_vec()), "a 16-byte block repeated");
    }
    let mut counts = [0usize; 256];
    for byte in &all {
        counts[usize::from(*byte)] += 1;
    }
    for (value, count) in counts.iter().enumerate() {
        assert!(
            (3_700..=4_500).contains(count),
            "byte value {value} occurred {count} times"
        );
    }
}

#[test]
fn hex_and_base64url_forms_count_random_bytes() {
    let mut vm = instance(HostPolicy::default());
    let hex = text_of(call(&mut vm, "draw_hex", vec![int(32)]));
    assert_eq!(hex.len(), 64);
    assert!(hex
        .chars()
        .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()));
    let url = text_of(call(&mut vm, "draw_base64url", vec![int(32)]));
    assert_eq!(url.len(), 44);
    assert!(url
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '='));
    assert_eq!(text_of(call(&mut vm, "draw_hex", vec![int(0)])), "");
    assert_ne!(hex, text_of(call(&mut vm, "draw_hex", vec![int(32)])));
}

#[test]
fn the_seeded_generator_is_still_deterministic() {
    let mut vm = instance(HostPolicy::default());
    let a = call(&mut vm, "seeded_sequence", vec![int(7)]);
    let _ = draw(&mut vm, 64);
    let b = call(&mut vm, "seeded_sequence", vec![int(7)]);
    assert_eq!(a, b);
    assert_ne!(a, call(&mut vm, "seeded_sequence", vec![int(8)]));
}

#[test]
fn a_pinned_source_makes_a_session_token_a_known_value_that_verifies() {
    let pinned: Vec<u8> = (0u8..32).collect();
    let policy = HostPolicy::default().with_entropy_source(EntropySource::fixed(pinned.clone()));
    let mut vm = instance(policy);
    let key = b"server-secret".to_vec();
    let token = text_of(Value::ResultOk(Box::new(call(
        &mut vm,
        "session_token",
        vec![Value::Bytes(guion_model::Shared::new(key.clone()))],
    ))));
    let nonce: String = pinned.iter().map(|byte| format!("{byte:02x}")).collect();
    assert_eq!(
        nonce,
        "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"
    );
    let tag: String = crypto_hmac_sha256(&key, nonce.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(token, format!("{nonce}.{tag}"));
    let mut verify = |token: &str, key: &[u8]| {
        call(
            &mut vm,
            "verify_token",
            vec![
                Value::Bytes(guion_model::Shared::new(key.to_vec())),
                Value::Text(token.into()),
            ],
        )
    };
    assert_eq!(verify(&token, &key), Value::Bool(true));
    assert_eq!(verify(&token, b"another-secret"), Value::Bool(false));
    let tampered = token.replacen('0', "1", 1);
    assert_eq!(verify(&tampered, &key), Value::Bool(false));
}

#[test]
fn a_pinned_source_continues_its_stream_and_a_failing_source_is_a_typed_error() {
    let policy = HostPolicy::default().with_entropy_source(EntropySource::fixed(vec![1, 2, 3]));
    let mut vm = instance(policy);
    assert_eq!(bytes_of(draw(&mut vm, 4)), vec![1, 2, 3, 1]);
    assert_eq!(bytes_of(draw(&mut vm, 2)), vec![2, 3]);

    let failing = EntropySource::new(|_| Err("the pinned source is unavailable".into()));
    let mut vm = instance(HostPolicy::default().with_entropy_source(failing));
    assert_eq!(error_code(draw(&mut vm, 8)), "E_ENTROPY_UNAVAILABLE");
    // A count of zero asks for nothing, so the source is never consulted.
    assert!(bytes_of(draw(&mut vm, 0)).is_empty());
}

#[test]
fn the_pinned_source_is_used_only_when_a_policy_sets_it() {
    assert!(HostPolicy::default().entropy_source.is_none());
    let pinned = HostPolicy::default().with_entropy_source(EntropySource::fixed(vec![0xAA; 32]));
    let mut with_pin = instance(pinned);
    assert_eq!(bytes_of(draw(&mut with_pin, 32)), vec![0xAA; 32]);
    // Another instance with an unset policy in the same process draws from the OS.
    let mut without = instance(HostPolicy::default());
    let one = bytes_of(draw(&mut without, 32));
    let two = bytes_of(draw(&mut without, 32));
    assert_ne!(one, vec![0xAA; 32]);
    assert_ne!(two, vec![0xAA; 32]);
    assert_ne!(one, two);
    // The pin did not leak into a later draw on the pinned instance's neighbour or back.
    assert_eq!(bytes_of(draw(&mut with_pin, 32)), vec![0xAA; 32]);
}

/// The format before `RandomBytes` existed.
const FORMAT_BEFORE_RANDOM_BYTES: u32 = 67;

#[test]
fn a_format_67_artifact_still_loads_and_runs_and_cannot_hold_random_bytes() {
    let source = "fn main() -> Unit:\n    print(21 * 2)\n";
    let mut artifact = guion_compiler::compile("old.gu", source).unwrap();
    artifact.format_version = FORMAT_BEFORE_RANDOM_BYTES;
    let bytes = artifact.to_bytes().expect("format 67 can still be written");
    let reread = guion_model::Artifact::from_bytes(&bytes).expect("format 67 can still be read");
    assert_eq!(reread.format_version, FORMAT_BEFORE_RANDOM_BYTES);
    let mut instance = Runtime::vm_only()
        .load(reread, HostPolicy::default())
        .unwrap();
    instance.invoke("main", vec![]).unwrap();
    assert_eq!(instance.take_output(), "42\n");

    // A program that draws random bytes carries the op, so it cannot be written below 68.
    let mut vm_artifact = {
        let root = std::env::temp_dir().join(format!("guion-entropy-gate-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let entry = root.join("main.gu");
        std::fs::write(
            &entry,
            "import \"std:crypto\"\nfn main() -> Int:\n    return len(unwrap(crypto_random_bytes(4)))\n",
        )
        .unwrap();
        let stdlib =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib");
        let artifact = guion_compiler::compile_project_with_roots(
            &entry,
            &root,
            Some(&stdlib),
            &std::collections::HashMap::new(),
        )
        .unwrap();
        let _ = std::fs::remove_dir_all(&root);
        artifact
    };
    assert!(vm_artifact.functions.iter().any(|function| function
        .code
        .iter()
        .any(|instruction| matches!(instruction.op, guion_model::Op::RandomBytes))));
    vm_artifact.format_version = FORMAT_BEFORE_RANDOM_BYTES;
    assert_eq!(
        vm_artifact.to_bytes().unwrap_err().code,
        "E_ARTIFACT_VERSION"
    );
}
