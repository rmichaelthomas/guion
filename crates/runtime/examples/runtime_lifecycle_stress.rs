use std::{
    env, process, thread,
    time::{Duration, Instant},
};
use veritas_compiler::compile;
use veritas_model::{BigInt, Value};
use veritas_runtime::{HostPolicy, Runtime};

const SOURCE: &str = r#"
extern python "numpy:arange" fn vector(length: Int) -> List[Int]

fn sum_array(length: Int) -> Int:
    let values = vector(length)
    var index = 0
    var total = 0
    while index < len(values):
        set total = total + values[index]
        set index = index + 1
    print(total)
    return total

fn main() -> Int:
    return 0
"#;

fn positive_usize(value: &str, name: &str) -> usize {
    value
        .parse::<usize>()
        .ok()
        .filter(|n| *n > 0)
        .unwrap_or_else(|| {
            eprintln!("{name} must be a positive integer");
            process::exit(2)
        })
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 7 {
        eprintln!("usage: runtime_lifecycle_stress PYTHON ARRAY_SIZE BURST_CALLS PACED_CALLS PACE_MS WARMUPS");
        process::exit(2);
    }
    let python = &args[1];
    let array_size = positive_usize(&args[2], "array size");
    let burst_calls = positive_usize(&args[3], "burst calls");
    let paced_calls = positive_usize(&args[4], "paced calls");
    let pace_ms = args[5].parse::<u64>().unwrap_or_else(|_| {
        eprintln!("pace must be a nonnegative integer");
        process::exit(2)
    });
    let warmups = positive_usize(&args[6], "warmups");

    let artifact = compile("runtime-lifecycle-stress.vr", SOURCE).unwrap_or_else(|diagnostic| {
        eprintln!("compile failed: {diagnostic:?}");
        process::exit(1)
    });
    let mut instance = Runtime::new()
        .load(
            artifact,
            HostPolicy::default().with_python(python, Duration::from_secs(30)),
        )
        .unwrap_or_else(|error| {
            eprintln!("runtime load failed: {error:?}");
            process::exit(1)
        });
    let expected = BigInt::from_usize(array_size.saturating_mul(array_size.saturating_sub(1)) / 2);

    for _ in 0..warmups {
        invoke_checked(&mut instance, array_size, &expected);
    }
    let burst_started = Instant::now();
    let mut burst_ns = Vec::with_capacity(burst_calls);
    for _ in 0..burst_calls {
        let start = Instant::now();
        invoke_checked(&mut instance, array_size, &expected);
        burst_ns.push(start.elapsed().as_nanos());
    }
    let burst_elapsed_ns = burst_started.elapsed().as_nanos();

    let pace = Duration::from_millis(pace_ms);
    let paced_started = Instant::now();
    let mut next_deadline = paced_started;
    let mut paced_ns = Vec::with_capacity(paced_calls);
    let mut scheduling_lateness_ns = Vec::with_capacity(paced_calls);
    for _ in 0..paced_calls {
        if pace_ms > 0 {
            next_deadline += pace;
            let now = Instant::now();
            if next_deadline > now {
                thread::sleep(next_deadline - now);
            }
        }
        let actual_start = Instant::now();
        scheduling_lateness_ns.push(
            actual_start
                .saturating_duration_since(next_deadline)
                .as_nanos(),
        );
        invoke_checked(&mut instance, array_size, &expected);
        paced_ns.push(actual_start.elapsed().as_nanos());
    }
    let paced_elapsed_ns = paced_started.elapsed().as_nanos();

    let close_started = Instant::now();
    instance.close();
    let close_ns = close_started.elapsed().as_nanos();
    let use_after_close_code = instance
        .invoke(
            "sum_array",
            vec![Value::Int(BigInt::from_usize(array_size))],
        )
        .expect_err("invoking a closed instance must fail")
        .code;
    assert_eq!(use_after_close_code, "R_INSTANCE_CLOSED");

    println!(
        "{}",
        serde_json::json!({
            "array_size": array_size,
            "expected_sum": expected.to_string(),
            "warmups": warmups,
            "burst": {"latency_ns": burst_ns, "elapsed_ns": burst_elapsed_ns},
            "paced": {"latency_ns": paced_ns, "scheduling_lateness_ns": scheduling_lateness_ns, "elapsed_ns": paced_elapsed_ns, "pace_ms": pace_ms},
            "close_ns": close_ns,
            "use_after_close_error": use_after_close_code,
            "correctness": "all calls returned the exact arithmetic sum and expected output"
        })
    );
}

fn invoke_checked(instance: &mut veritas_runtime::Instance, array_size: usize, expected: &BigInt) {
    let result = instance
        .invoke(
            "sum_array",
            vec![Value::Int(BigInt::from_usize(array_size))],
        )
        .unwrap_or_else(|error| {
            eprintln!("invoke failed: {error:?}");
            process::exit(1)
        });
    assert_eq!(
        result,
        Value::Int(expected.clone()),
        "returned value mismatch"
    );
    assert_eq!(
        instance.take_output(),
        format!("{expected}\n"),
        "printed output mismatch"
    );
}
