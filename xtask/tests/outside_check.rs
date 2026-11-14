use serde_json::{json, Value};
use xtask::outside_check::{evaluate, NOISE_ALLOWANCE_PERCENT};

fn baseline() -> Value {
    json!({"hello_ms": 6.0, "rows": {"5000": {"guion_ms": 20.0}, "200000": {"guion_ms": 400.0}}})
}

fn measured(hello: f64, small: f64, large: f64) -> Value {
    json!({"hello_ms": hello, "rows": {"5000": {"guion_ms": small}, "200000": {"guion_ms": large}}})
}

#[test]
fn faster_and_within_allowance_passes() {
    let (report, failures) = evaluate(
        &baseline(),
        &measured(5.0, 20.9, 419.9),
        NOISE_ALLOWANCE_PERCENT,
    );
    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(report.len(), 3);
}

#[test]
fn a_median_more_than_five_percent_slower_fails() {
    let (_, failures) = evaluate(
        &baseline(),
        &measured(6.0, 20.0, 420.5),
        NOISE_ALLOWANCE_PERCENT,
    );
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(failures[0].contains("200000 rows"));
}

#[test]
fn a_missing_measurement_fails() {
    let measured = json!({"hello_ms": 6.0, "rows": {"5000": {"guion_ms": 20.0}}});
    let (_, failures) = evaluate(&baseline(), &measured, NOISE_ALLOWANCE_PERCENT);
    assert_eq!(failures.len(), 1);
}
