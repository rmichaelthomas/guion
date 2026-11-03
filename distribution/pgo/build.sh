#!/bin/sh
# Build a profile-guided release binary. Usage: distribution/pgo/build.sh [OUT_DIR]
# Trains only on the programs in this directory and the AllHereHTML and text-scaling
# checks, never on the benchmarks the release is measured with.
# Requires the llvm-tools component (rustup component add llvm-tools), or LLVM_PROFDATA.
set -eu
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
OUT=${1:-"$ROOT/target/pgo"}
TARGET=$(rustc -vV | sed -n 's/^host: //p')
PROFDATA=${LLVM_PROFDATA:-$(find "$(rustc --print sysroot)/lib/rustlib/$TARGET/bin" -name llvm-profdata 2>/dev/null | head -n 1)}
[ -n "$PROFDATA" ] || { echo "llvm-profdata not found: install llvm-tools or set LLVM_PROFDATA" >&2; exit 2; }
PYTHON=${PYTHON:-python3}
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)
RAW="$OUT/raw"
TRAIN="$OUT/train"
rm -rf "$RAW" "$TRAIN"
mkdir -p "$RAW" "$TRAIN"
export CARGO_TARGET_DIR="$OUT/cargo"
build() {
    (cd "$ROOT" && RUSTFLAGS="$1" cargo build --release --locked --offline -p guion-cli --target "$TARGET")
}
build "-Cprofile-generate=$RAW"
GUION="$OUT/instrumented/guion"
mkdir -p "$OUT/instrumented"
cp "$CARGO_TARGET_DIR/$TARGET/release/guion" "$GUION"
cp "$ROOT"/distribution/pgo/clicks.gu "$ROOT"/distribution/pgo/items.gu "$TRAIN/"
cat > "$TRAIN/guion.toml" <<'TOML'
[project]
name = "pgo-training"
version = "0.1.0"
entry = "main.gu"

[runtime]
instruction_limit = 1000000000
allocation_limit_bytes = 8589934592
TOML
"$PYTHON" "$ROOT/distribution/pgo/generate_training_data.py" "$TRAIN"
export LLVM_PROFILE_FILE="$RAW/guion-%p-%m.profraw"
for program in clicks items; do
    cp "$TRAIN/$program.gu" "$TRAIN/main.gu"
    (cd "$TRAIN" && "$GUION" build main.gu -o "$program.gub" && "$GUION" run "$program.gub" >/dev/null)
done
(cd "$ROOT" && "$PYTHON" evaluation/completion/check_allherehtml.py --binary "$GUION" >/dev/null)
(cd "$ROOT" && "$PYTHON" evaluation/completion/test_text_scaling.py --binary "$GUION" >/dev/null)
unset LLVM_PROFILE_FILE
"$PROFDATA" merge -o "$OUT/guion.profdata" "$RAW"/*.profraw
build "-Cprofile-use=$OUT/guion.profdata -Cllvm-args=-pgo-warn-missing-function=false"
cp "$CARGO_TARGET_DIR/$TARGET/release/guion" "$OUT/guion"
echo "built $OUT/guion"
