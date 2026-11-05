#!/bin/sh
# Run, before merging, everything CI runs on main after the merge.
#
#   scripts/pre-merge-check.sh          the checks every change needs
#   scripts/pre-merge-check.sh speed    also time the compute benchmarks (Richards and the rest)
#
# Run the speed check when a change touches the compiler, VM or native backend,
# and before updating published speed numbers. Docs, site copy and CLI-only
# changes do not need it. It needs python3 with the packages in
# evaluation/completion/requirements-real-task.txt.
set -e
cd "$(dirname "$0")/.."

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --locked -p guion-cli
python3 evaluation/completion/test_text_scaling.py --binary target/debug/guion
python3 evaluation/completion/check_allherehtml.py --binary target/debug/guion
cargo build --locked --target wasm32-unknown-unknown -p guion-vm-wasm
cargo xtask closeout --check-schema
cargo xtask agent-reference --check
python3 evaluation/agent-choice/validate_switch_conditions.py

if [ "$1" = "speed" ]; then
  cargo build --locked --release -p guion-cli
  python3 evaluation/completion/performance/session.py baseline
fi

echo "pre-merge checks passed"
