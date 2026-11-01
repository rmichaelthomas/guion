#!/bin/sh
# Run the same fast checks CI runs on a pull request, before pushing.
set -e
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
echo "pre-push checks passed"
