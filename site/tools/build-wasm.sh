#!/bin/sh
# Build the two playground modules from this repository and record their hashes.
#   guion_vm.wasm        guion-vm-wasm (browser ABI 1.0)
#   guion_compiler.wasm  site/tools/wasm-compiler (single-file compile adapter)
# Needs Rust 1.97.1 with the wasm32-unknown-unknown target. Run from anywhere.
set -eu

SITE_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
REPO_DIR=$(dirname "$SITE_DIR")
OUT_DIR="$SITE_DIR/public/playground"
TARGET_DIR=${CARGO_TARGET_DIR:-"$REPO_DIR/target/site-wasm"}
PATH="$HOME/.cargo/bin:$PATH"

mkdir -p "$OUT_DIR"

cargo build --manifest-path "$REPO_DIR/Cargo.toml" --package guion-vm-wasm \
  --target wasm32-unknown-unknown --release --locked --offline --target-dir "$TARGET_DIR"
cp "$TARGET_DIR/wasm32-unknown-unknown/release/guion_vm_wasm.wasm" "$OUT_DIR/guion_vm.wasm"

cargo build --manifest-path "$SITE_DIR/tools/wasm-compiler/Cargo.toml" \
  --target wasm32-unknown-unknown --release --locked --offline --target-dir "$TARGET_DIR"
cp "$TARGET_DIR/wasm32-unknown-unknown/release/guion_site_compiler_wasm.wasm" "$OUT_DIR/guion_compiler.wasm"

node --input-type=module - "$OUT_DIR" "$SITE_DIR/tools/wasm-compiler/Cargo.lock" "$(rustc --version)" <<'JS'
import { readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import path from 'node:path';
const [outDir, lockPath, rustc] = process.argv.slice(2);
const sha = (bytes) => createHash('sha256').update(bytes).digest('hex');
const files = {};
for (const name of ['guion_compiler.wasm', 'guion_vm.wasm']) {
  const bytes = readFileSync(path.join(outDir, name));
  files[name] = { bytes: bytes.byteLength, sha256: sha(bytes) };
}
const record = { rustc, adapterCargoLockSha256: sha(readFileSync(lockPath)), files };
writeFileSync(path.join(outDir, 'wasm.build.json'), `${JSON.stringify(record, null, 2)}\n`);
console.log(`Built ${Object.keys(files).join(' and ')}`);
JS
