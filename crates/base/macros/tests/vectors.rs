//! `vectors!` end to end: each JSON file under `vectors/` becomes one `#[test]`.
#![allow(
    clippy::panic,
    clippy::expect_used,
    reason = "tests fail loudly by design"
)]
#![allow(
    clippy::redundant_closure,
    reason = "the closure is the 08-MACROS.md example, kept verbatim"
)]

use serde_json::Value;

fn run(text: &str) {
    let vector: Value = serde_json::from_str(text).expect("vector is valid JSON");
    let input = vector
        .get("input")
        .and_then(Value::as_f64)
        .expect("input is a number");
    let expected = vector
        .get("expected")
        .and_then(Value::as_f64)
        .expect("expected is a number");
    let tolerance = vector
        .get("tolerance")
        .and_then(Value::as_f64)
        .expect("tolerance is a number");
    let got = input * 2.0;
    assert!(
        (got - expected).abs() <= tolerance,
        "got {got}, want {expected} within {tolerance}"
    );
}

kontra_macros::vectors!("vectors/*.json", |v| run(v));
