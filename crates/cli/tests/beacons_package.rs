//! The `beacons` package (packages/beacons): its own tests, and the recorded live check of 100
//! consecutive drand quicknet rounds replayed with no network, clock or file grant.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn package() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/beacons")
}

fn guion(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(args)
        .current_dir(package())
        .output()
        .expect("guion starts")
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn package_checks_cleanly() {
    let checked = guion(&["check", "tests.gu"]);
    assert!(checked.status.success(), "{}", text(&checked));
    assert!(!text(&checked).contains("warning"), "{}", text(&checked));
}

#[test]
fn package_tests_pass_offline() {
    let tested = guion(&["test", "tests.gu"]);
    assert!(tested.status.success(), "{}", text(&tested));
    assert!(text(&tested).contains("tests passed"), "{}", text(&tested));
}

#[test]
fn the_recorded_hundred_rounds_replay_as_verified() {
    let replayed = guion(&["run", "--replay", "tapes/drand-100.tape", "live_drand.gu"]);
    let out = text(&replayed);
    assert!(replayed.status.success(), "{out}");
    assert!(out.contains("100/100 rounds verified"), "{out}");
    assert!(
        out.contains("tlock_open: guion tlock fixture plaintext"),
        "{out}"
    );
}
