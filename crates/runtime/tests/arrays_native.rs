//! Array code and machine code interoperate: functions that use arrays stay in the VM with a
//! stated reason, the functions around them may still compile to machine code, and the output
//! is the VM's either way.

use guion_compiler::compile;
use guion_runtime::{HostPolicy, Runtime};

const SOURCE: &str = r#"fn count(n: Int) -> Int:
    var i = 0
    var total = 0
    while i < n:
        set total = total + i % 7
        set i = i + 1
    return total

fn fill(n: Int) -> Array[f64]:
    var a = array_f64_zeros([n])
    var i = 0
    while i < n:
        set a = array_set(a, i, int_to_float(count(i % 50)))
        set i = i + 1
    return a

fn scale(n: Int) -> Float64:
    return array_sum(array_mul_scalar(fill(n), 0.5))

fn driver(n: Int) -> Int:
    var i = 0
    var total = 0
    while i < n:
        set total = total + float_to_int(scale(10))
        set i = i + 1
    return total

fn main() -> Int:
    print(count(100000))
    print(scale(2000))
    print(driver(200))
    return 0
"#;

#[test]
fn array_functions_run_in_the_vm_and_match_with_machine_code_on_and_off() {
    let artifact = compile("arrays-native.gu", SOURCE).expect("program compiles");
    let policy = HostPolicy {
        instruction_limit: 1_000_000_000,
        ..HostPolicy::default()
    };
    let mut native = Runtime::new()
        .load(artifact.clone(), policy.clone())
        .expect("native runtime loads");
    let mut vm = Runtime::vm_only()
        .load(artifact, policy)
        .expect("VM runtime loads");
    native.invoke("main", vec![]).expect("native program runs");
    vm.invoke("main", vec![]).expect("VM program runs");
    let output = native.take_output();
    assert_eq!(output, vm.take_output());
    assert_eq!(output.lines().count(), 3, "{output}");

    let compiled = native.native_function_names();
    assert!(
        compiled.iter().any(|name| name == "count"),
        "the integer loop should compile to machine code; compiled: {compiled:?}"
    );
    assert!(
        !compiled
            .iter()
            .any(|name| name == "fill" || name == "scale"),
        "array functions must stay in the VM; compiled: {compiled:?}"
    );
    let profile = native.native_compile_profile();
    let rejected = profile["rejected_functions"]
        .as_array()
        .expect("native compile profile lists rejected functions");
    let fill = rejected
        .iter()
        .find(|function| function["name"] == "fill")
        .unwrap_or_else(|| panic!("fill should be listed as VM-run: {profile}"));
    assert!(
        fill["reason_code"]
            .as_str()
            .is_some_and(|code| !code.is_empty()),
        "{fill}"
    );
    assert_eq!(native.native_deoptimizations(), 0);
}
