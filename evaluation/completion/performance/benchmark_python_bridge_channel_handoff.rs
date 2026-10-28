//! Compare an unbounded response queue with a one-slot synchronous response queue.
//!
//! Each measured round trips one request and one response through two threads. The request
//! channel stays unbounded in both cases; only the response channel changes.

use std::sync::mpsc;
use std::thread;
use std::time::Instant;

const MESSAGES: usize = 100_000;
const ROUNDS: usize = 31;

fn run_unbounded() -> f64 {
    let (request_tx, request_rx) = mpsc::channel::<usize>();
    let (response_tx, response_rx) = mpsc::channel::<usize>();
    let worker = thread::spawn(move || {
        for _ in 0..MESSAGES {
            let value = request_rx.recv().unwrap();
            response_tx.send(value).unwrap();
        }
    });
    let started = Instant::now();
    for value in 0..MESSAGES {
        request_tx.send(value).unwrap();
        assert_eq!(response_rx.recv().unwrap(), value);
    }
    worker.join().unwrap();
    started.elapsed().as_nanos() as f64 / MESSAGES as f64
}

fn run_bounded() -> f64 {
    let (request_tx, request_rx) = mpsc::channel::<usize>();
    let (response_tx, response_rx) = mpsc::sync_channel::<usize>(1);
    let worker = thread::spawn(move || {
        for _ in 0..MESSAGES {
            let value = request_rx.recv().unwrap();
            response_tx.send(value).unwrap();
        }
    });
    let started = Instant::now();
    for value in 0..MESSAGES {
        request_tx.send(value).unwrap();
        assert_eq!(response_rx.recv().unwrap(), value);
    }
    worker.join().unwrap();
    started.elapsed().as_nanos() as f64 / MESSAGES as f64
}

fn as_json_array(values: &[f64]) -> String {
    let values = values
        .iter()
        .map(|value| format!("{value:.2}"))
        .collect::<Vec<_>>();
    format!("[{}]", values.join(","))
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted[sorted.len() / 2]
}

fn main() {
    let mut unbounded = Vec::with_capacity(ROUNDS);
    let mut bounded = Vec::with_capacity(ROUNDS);
    for round in 0..ROUNDS {
        if round % 2 == 0 {
            unbounded.push(run_unbounded());
            bounded.push(run_bounded());
        } else {
            bounded.push(run_bounded());
            unbounded.push(run_unbounded());
        }
    }
    let paired_improvement = unbounded
        .iter()
        .zip(&bounded)
        .map(|(unbounded, bounded)| unbounded - bounded)
        .collect::<Vec<_>>();
    println!(
        "{{\"schema_version\":1,\"messages_per_round\":{MESSAGES},\"rounds\":{ROUNDS},\"unbounded_ns_per_response\":{},\"bounded_one_slot_ns_per_response\":{},\"paired_unbounded_minus_bounded_ns\":{},\"median_unbounded_ns\":{:.2},\"median_bounded_ns\":{:.2},\"median_paired_improvement_ns\":{:.2},\"positive_paired_improvements\":{}}}",
        as_json_array(&unbounded),
        as_json_array(&bounded),
        as_json_array(&paired_improvement),
        median(&unbounded),
        median(&bounded),
        median(&paired_improvement),
        paired_improvement.iter().filter(|value| **value > 0.0).count()
    );
}
