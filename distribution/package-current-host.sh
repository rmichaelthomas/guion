#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
OUT=${1:-"$ROOT/distribution/out"}
case "$OUT" in
  /*) ;;
  *) OUT="$ROOT/$OUT" ;;
esac
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m | tr '[:upper:]' '[:lower:]')
VERSION=$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT/Cargo.toml" | head -n 1)
NAME="guion-$VERSION-$OS-$ARCH"
BUILD_DIR=${CARGO_TARGET_DIR:-"$ROOT/target"}
case "$BUILD_DIR" in
  /*) ;;
  *) BUILD_DIR="$ROOT/$BUILD_DIR" ;;
esac
case "$OS-$ARCH" in
  darwin-arm64)
    MANIFEST="$ROOT/distribution/toolchains/manifest.toml"
    UV_ARCHIVE_NAME="uv-aarch64-apple-darwin.tar.gz"
    RUST_TARGET="aarch64-apple-darwin"
    ;;
  linux-aarch64)
    MANIFEST="$ROOT/distribution/toolchains/manifest-linux-aarch64.toml"
    UV_ARCHIVE_NAME="uv-aarch64-unknown-linux-gnu.tar.gz"
    RUST_TARGET="aarch64-unknown-linux-gnu"
    ;;
  *)
    echo "no release target is defined for $OS-$ARCH" >&2
    exit 1
    ;;
esac
UV_VERSION=$(sed -n 's/^uv_version = "\([^\"]*\)"/\1/p' "$MANIFEST")
UV_TARGET=$(sed -n 's/^target = "\([^\"]*\)"/\1/p' "$MANIFEST")
UV_ARCHIVE_SHA256=$(sed -n 's/^archive_sha256 = "\([^\"]*\)"/\1/p' "$MANIFEST")
UV_BINARY_SHA256=$(sed -n 's/^binary_sha256 = "\([^\"]*\)"/\1/p' "$MANIFEST")
UV_ARCHIVE_BINARY=$(sed -n 's/^archive_binary_path = "\([^\"]*\)"/\1/p' "$MANIFEST")
UV_ARCHIVE=${GUION_UV_ARCHIVE:-"$ROOT/distribution/toolchains/cache/$UV_ARCHIVE_NAME"}
UV_BINARY=${GUION_UV_BINARY:-}
if [ -z "$UV_VERSION" ] || [ -z "$UV_TARGET" ] || [ -z "$UV_ARCHIVE_SHA256" ] || [ -z "$UV_BINARY_SHA256" ] || [ -z "$UV_ARCHIVE_BINARY" ]; then
  echo "release toolchain manifest is incomplete: $MANIFEST" >&2
  exit 1
fi
if command -v shasum >/dev/null 2>&1; then
  sha256() { shasum -a 256 "$1" | awk '{print $1}'; }
elif command -v sha256sum >/dev/null 2>&1; then
  sha256() { sha256sum "$1" | awk '{print $1}'; }
else
  echo "no SHA-256 utility found (need shasum or sha256sum)" >&2
  exit 1
fi
if [ -n "$UV_BINARY" ]; then
  if [ ! -f "$UV_BINARY" ]; then
    echo "pinned uv binary is missing: $UV_BINARY" >&2
    exit 1
  fi
  if [ "$(sha256 "$UV_BINARY")" != "$UV_BINARY_SHA256" ]; then
    echo "pinned uv binary checksum mismatch" >&2
    exit 1
  fi
else
  if [ ! -f "$UV_ARCHIVE" ]; then
    echo "pinned uv $UV_VERSION archive is missing: $UV_ARCHIVE" >&2
    echo "download the archive URL in distribution/toolchains/manifest.toml and set GUION_UV_ARCHIVE" >&2
    echo "or provide a binary matching binary_sha256 with GUION_UV_BINARY" >&2
    exit 1
  fi
  if [ "$(sha256 "$UV_ARCHIVE")" != "$UV_ARCHIVE_SHA256" ]; then
    echo "pinned uv archive checksum mismatch" >&2
    exit 1
  fi
fi
sh "$ROOT/distribution/tests/test_install.sh"
(cd "$ROOT" && python3 -m unittest discover -s distribution/tests -p 'test_*.py')
CARGO_TARGET_DIR="$BUILD_DIR" cargo build --release --offline --workspace --manifest-path "$ROOT/Cargo.toml"
CARGO_TARGET_DIR="$BUILD_DIR" cargo build --release --offline -p guion-runtime --example runtime_lifecycle_stress --manifest-path "$ROOT/Cargo.toml"
mkdir -p "$OUT"
if [ -e "$OUT/$NAME" ] || [ -e "$OUT/$NAME.tar.gz" ] || [ -e "$OUT/$NAME.tar.gz.sha256" ]; then
  echo "refusing to overwrite an existing package in $OUT" >&2
  exit 1
fi
TEMP=$(mktemp -d "$OUT/.$NAME.staging.XXXXXX")
trap 'rm -rf "$TEMP"' EXIT HUP INT TERM
STAGING="$TEMP/$NAME"
mkdir -p "$STAGING/distribution" "$STAGING/docs" "$STAGING/language/examples" "$STAGING/toolchains" "$STAGING/licenses" "$STAGING/evaluation/development/famous_benchmarks/upstream/pyperformance-1.14.0" "$STAGING/evaluation/bin" "$STAGING/evaluation/briefs" "$STAGING/evaluation/results" "$STAGING/evaluation/tests" "$STAGING/examples/python_interop/.guion/python-bridge" "$STAGING/examples/feed_digest/.guion/python-bridge" "$STAGING/examples/task_ledger/src" "$STAGING/examples/task_graph" "$STAGING/examples/feed_inbox/.guion/python-bridge" "$STAGING/examples/feed_inbox/src" "$STAGING/examples/feed_inbox/examples" "$STAGING/examples/inventory_reconcile/src" "$STAGING/examples/api_sync/src" "$STAGING/examples/package_analysis/.guion/python-bridge"
mkdir -p "$STAGING/evaluation/protocol" "$STAGING/evaluation/fixtures" "$STAGING/evaluation/review/independent-v1/product-information" "$STAGING/evaluation/protocol/prompts" "$STAGING/evaluation/sandbox"
mkdir -p "$STAGING/evaluation/calibration/python-reference" "$STAGING/evaluation/calibration/results"
python3 "$ROOT/distribution/collect-rust-licenses.py" "$STAGING/licenses/rust" "$RUST_TARGET"
mkdir -p "$TEMP/uv"
if [ -n "$UV_BINARY" ]; then
  cp "$UV_BINARY" "$STAGING/toolchains/uv"
else
  tar -xzf "$UV_ARCHIVE" -C "$TEMP/uv"
  cp "$TEMP/uv/$UV_ARCHIVE_BINARY" "$STAGING/toolchains/uv"
fi
chmod 755 "$STAGING/toolchains/uv"
UV_INSTALLED_VERSION=$("$STAGING/toolchains/uv" --version | awk '{print $2}')
if [ "$UV_INSTALLED_VERSION" != "$UV_VERSION" ]; then
  echo "pinned uv archive contains version $UV_INSTALLED_VERSION, expected $UV_VERSION" >&2
  exit 1
fi
cp "$BUILD_DIR/release/guion" "$STAGING/guion"
cp "$MANIFEST" "$STAGING/toolchains/manifest.toml"
cp "$ROOT/distribution/package-current-host.sh" "$STAGING/distribution/package-current-host.sh"
cp "$ROOT/distribution/licenses/uv-LICENSE-MIT.txt" "$ROOT/distribution/licenses/uv-LICENSE-APACHE.txt" "$ROOT/distribution/licenses/tcl-license.terms" "$ROOT/distribution/licenses/tk-license.terms" "$ROOT/distribution/licenses/UPSTREAM-NOTICES.md" "$STAGING/licenses/"
cp "$ROOT/README.md" "$STAGING/README.md"
cp "$ROOT/distribution/install.sh" "$STAGING/install.sh"
cp "$ROOT/docs/reference.md" "$ROOT/docs/compatibility-and-migrations.md" "$ROOT/docs/architecture.md" "$ROOT/docs/prior-language-build-lessons.md" "$ROOT/docs/veritas_checkpoint_v1_1_foundation_build.md" "$ROOT/docs/veritas_checkpoint_v1_2_numeric_foundation.md" "$ROOT/docs/veritas_checkpoint_v1_3_integer_arithmetic.md" "$ROOT/docs/veritas_checkpoint_v1_4_records.md" "$ROOT/docs/veritas_checkpoint_v1_5_module_graph.md" "$ROOT/docs/veritas_checkpoint_v1_6_option_result_matching.md" "$ROOT/docs/veritas_checkpoint_v1_7_resource_accounting.md" "$ROOT/docs/veritas_checkpoint_v1_8_artifact_policy.md" "$ROOT/docs/veritas_checkpoint_v1_9_provenance.md" "$ROOT/docs/veritas_checkpoint_v1_10_bytes.md" "$ROOT/docs/veritas_checkpoint_v1_11_python_keyword_calls.md" "$ROOT/docs/veritas_checkpoint_v1_13_feed_digest.md" "$ROOT/docs/veritas_checkpoint_v1_14_json_state.md" "$ROOT/docs/veritas_checkpoint_v1_15_atomic_state.md" "$ROOT/docs/veritas_checkpoint_v1_16_python_json_abi.md" "$ROOT/docs/veritas_checkpoint_v1_17_numpy_arrays.md" "$ROOT/docs/veritas_checkpoint_v1_18_runtime_lifecycle.md" "$ROOT/docs/veritas_checkpoint_v1_19_integrated_application.md" "$ROOT/docs/veritas_checkpoint_v1_20_current_host_packaging.md" "$ROOT/docs/veritas_checkpoint_v1_21_package_integrity.md" "$ROOT/docs/veritas_checkpoint_v1_22_dependency_provenance.md" "$ROOT/docs/veritas_checkpoint_v1_23_native_provenance.md" "$ROOT/docs/veritas_checkpoint_v1_24_standalone_notices.md" "$ROOT/docs/veritas_checkpoint_v1_25_native_python_owners.md" "$ROOT/docs/veritas_checkpoint_v1_26_local_installer.md" "$ROOT/docs/veritas_checkpoint_v1_27_json_test_reports.md" "$ROOT/docs/veritas_checkpoint_v1_28_offline_cache_integrity.md" "$ROOT/docs/veritas_checkpoint_v1_29_structured_api_docs.md" "$ROOT/docs/veritas_checkpoint_v1_30_shipped_docs_repair.md" "$ROOT/docs/veritas_checkpoint_v1_31_relocatable_evaluation_kit.md" "$ROOT/docs/veritas_checkpoint_v1_32_structured_build_reports.md" "$ROOT/docs/veritas_checkpoint_v1_33_structured_package_reports.md" "$ROOT/docs/veritas_checkpoint_v1_34_structured_sync_reports.md" "$STAGING/docs/"
cp "$ROOT/docs/veritas_checkpoint_v1_35_sync_error_taxonomy.md" "$STAGING/docs/"
cp "$ROOT/docs/veritas_checkpoint_v1_36_structured_doctor.md" "$STAGING/docs/"
cp "$ROOT/docs/veritas_checkpoint_v1_37_python_package_docs.md" "$STAGING/docs/"
cp "$ROOT/docs/veritas_checkpoint_v1_38_package_destination_safety.md" "$STAGING/docs/"
cp "$ROOT/docs/veritas_checkpoint_v1_39_archive_validation.md" "$STAGING/docs/"
cp "$ROOT/docs/veritas_checkpoint_v1_40_python_native_image_observation.md" "$STAGING/docs/"
cp "$ROOT/docs/veritas_checkpoint_v1_41_offline_wheel_native_provenance.md" "$STAGING/docs/"
cp "$ROOT/docs/veritas_checkpoint_v1_42_dependency_update_recovery.md" "$STAGING/docs/"
cp "$ROOT/docs/veritas_checkpoint_v1_43_compatibility_policy.md" "$STAGING/docs/"
cp "$ROOT/docs/veritas_checkpoint_v1_45_evaluation_ledger.md" "$STAGING/docs/"
cp "$ROOT/docs/veritas_checkpoint_v1_46_packaged_lifecycle_stress.md" "$STAGING/docs/"
cp "$ROOT/docs/veritas_checkpoint_v1_47_static_capability_inspection.md" "$ROOT/docs/reuse-inventory.md" "$STAGING/docs/"
cp "$ROOT/evaluation/protocol/README.md" "$STAGING/docs/evaluation-protocol.md"
cp "$ROOT/evaluation/protocol/README.md" "$ROOT/evaluation/protocol/case-runner.md" "$STAGING/evaluation/protocol/"
cp "$ROOT/evaluation/protocol/ledger.py" "$ROOT/evaluation/protocol/recommendations.py" "$ROOT/evaluation/protocol/case_runner.py" "$ROOT/evaluation/protocol/schedule.py" "$ROOT/evaluation/protocol/stage_delivery.py" "$ROOT/evaluation/protocol/record_prerun.py" "$ROOT/evaluation/protocol/render_choice_prompt.py" "$ROOT/evaluation/protocol/usage_evidence.py" "$ROOT/evaluation/protocol/campaign-config.json" "$STAGING/evaluation/protocol/"
cp "$ROOT/evaluation/protocol/case-runner.md" "$STAGING/docs/"
cp "$ROOT/evaluation/sandbox/Dockerfile" "$ROOT/evaluation/sandbox/build-images.sh" "$ROOT/evaluation/sandbox/run-case.sh" "$ROOT/evaluation/sandbox/candidate-python.json" "$ROOT/evaluation/sandbox/candidate-veritas.json" "$STAGING/evaluation/sandbox/"
cp -R "$ROOT/evaluation/sandbox/dependencies" "$STAGING/evaluation/sandbox/"
cp "$ROOT/evaluation/protocol/prompts/"*.md "$STAGING/evaluation/protocol/prompts/"
cp "$ROOT/evaluation/briefs/"*.md "$STAGING/evaluation/briefs/"
cp -R "$ROOT/evaluation/briefs/independent" "$STAGING/evaluation/briefs/"
cp -R "$ROOT/evaluation/fixtures/independent-v1" "$STAGING/evaluation/fixtures/"
cp "$ROOT/evaluation/review/independent-v1/reviewer-instructions.md" "$ROOT/evaluation/review/independent-v1/review-form.json" "$ROOT/evaluation/review/independent-v1/review-result-v2.md" "$ROOT/evaluation/review/independent-v1/review-result-v3.md" "$ROOT/evaluation/review/independent-v1/review-result-v4.md" "$ROOT/evaluation/review/independent-v1/review-result-v5.md" "$ROOT/evaluation/review/independent-v1/review-result-v10.md" "$ROOT/evaluation/review/independent-v1/review-result-v11.md" "$ROOT/evaluation/review/independent-v1/review-result-v12.md" "$ROOT/evaluation/review/independent-v1/review-result-v13.md" "$ROOT/evaluation/review/independent-v1/review-result-v14.md" "$ROOT/evaluation/review/independent-v1/review-result-v15.md" "$ROOT/evaluation/review/independent-v1/review-result-v16.md" "$ROOT/evaluation/review/independent-v1/review-result-v17.md" "$ROOT/evaluation/review/independent-v1/review-result-v18.md" "$ROOT/evaluation/review/independent-v1/review-result-v19.md" "$ROOT/evaluation/review/independent-v1/review-result-final-pass.md" "$STAGING/evaluation/review/independent-v1/"
python3 - "$ROOT/evaluation/review/independent-v1/artifact-hashes.json" "$STAGING/evaluation/review/independent-v1/artifact-hashes.json" <<'PY'
import json
import sys
from pathlib import Path

source, destination = map(Path, sys.argv[1:])
manifest = json.loads(source.read_text(encoding="utf-8"))
manifest["artifacts"] = [
    artifact for artifact in manifest["artifacts"]
    if artifact.get("distribution_include", True) and not artifact.get("review_packet_only", False)
]
destination.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
PY
cp "$ROOT/evaluation/review/independent-v1/product-information/"*.md "$STAGING/evaluation/review/independent-v1/product-information/"
cp "$ROOT/evaluation/review/independent-v1/product-information/bundle-manifest.json" "$STAGING/evaluation/review/independent-v1/product-information/"
cp "$ROOT/evaluation/calibration/python-reference/main.py" "$ROOT/evaluation/calibration/python-reference/requirements.txt" "$STAGING/evaluation/calibration/python-reference/"
cp "$ROOT/evaluation/calibration/results/python-reference-calibration.json" "$STAGING/evaluation/calibration/results/"
cp "$ROOT/evaluation/calibration/run_reference_calibration.py" "$STAGING/evaluation/calibration/"
cp "$ROOT/evaluation/results/"*.json "$STAGING/evaluation/results/"
cp "$ROOT/evaluation/tests/"*.py "$STAGING/evaluation/tests/"
cp "$ROOT/evaluation/development/famous_benchmarks/runner.py" "$ROOT/evaluation/development/famous_benchmarks/upstream_probe.py" "$ROOT/evaluation/development/famous_benchmarks/richards.vr" "$ROOT/evaluation/development/famous_benchmarks/richards_reference.py" "$STAGING/evaluation/development/famous_benchmarks/"
cp "$ROOT/evaluation/development/famous_benchmarks/upstream/pyperformance-1.14.0/"*.py "$STAGING/evaluation/development/famous_benchmarks/upstream/pyperformance-1.14.0/"
cp "$ROOT/evaluation/development/famous_benchmarks/upstream/pyperformance-1.14.0/LICENSE.txt" "$STAGING/evaluation/development/famous_benchmarks/upstream/pyperformance-1.14.0/"
cp "$ROOT/evaluation/development/run_known.py" "$STAGING/evaluation/development/"
cp "$ROOT/evaluation/development/README.md" "$ROOT/evaluation/development/benchmark_runtime_lifecycle.py" "$ROOT/evaluation/development/benchmark_sales_report.py" "$ROOT/evaluation/development/runtime_lifecycle_stress.md" "$STAGING/evaluation/development/"
cp "$ROOT/crates/runtime/examples/runtime_lifecycle_stress.rs" "$STAGING/evaluation/development/"
cp "$BUILD_DIR/release/examples/runtime_lifecycle_stress" "$STAGING/evaluation/bin/"
cp "$ROOT/language/spec.md" "$ROOT/language/host-api.md" "$ROOT/language/python-abi.md" "$STAGING/language/"
cp "$ROOT/language/examples/"*.gu "$ROOT/language/examples/input.txt" "$STAGING/language/examples/"
cp "$ROOT/evaluation/development/python_interop/README.md" "$ROOT/evaluation/development/python_interop/main.vr" "$ROOT/evaluation/development/python_interop/veritas.toml" "$ROOT/evaluation/development/python_interop/veritas.lock" "$STAGING/examples/python_interop/"
cp "$ROOT/evaluation/development/python_interop/.veritas/python-bridge/pyproject.toml" "$ROOT/evaluation/development/python_interop/.veritas/python-bridge/uv.lock" "$STAGING/examples/python_interop/.guion/python-bridge/"
cp "$ROOT/evaluation/development/feed_digest/README.md" "$ROOT/evaluation/development/feed_digest/main.vr" "$ROOT/evaluation/development/feed_digest/veritas_feed.py" "$ROOT/evaluation/development/feed_digest/python_baseline.py" "$ROOT/evaluation/development/feed_digest/veritas.toml" "$ROOT/evaluation/development/feed_digest/veritas.lock" "$STAGING/examples/feed_digest/"
cp "$ROOT/evaluation/development/feed_digest/.veritas/python-bridge/pyproject.toml" "$ROOT/evaluation/development/feed_digest/.veritas/python-bridge/uv.lock" "$STAGING/examples/feed_digest/.guion/python-bridge/"
cp "$ROOT/evaluation/development/task_ledger/README.md" "$ROOT/evaluation/development/task_ledger/.gitignore" "$ROOT/evaluation/development/task_ledger/main.vr" "$ROOT/evaluation/development/task_ledger/veritas.toml" "$STAGING/examples/task_ledger/"
cp "$ROOT/evaluation/development/task_ledger/python_baseline.py" "$STAGING/examples/task_ledger/"
cp "$ROOT/evaluation/development/task_ledger/src/"*.vr "$STAGING/examples/task_ledger/src/"
cp "$ROOT/evaluation/development/task_graph/README.md" "$ROOT/evaluation/development/task_graph/main.vr" "$ROOT/evaluation/development/task_graph/reference.py" "$ROOT/evaluation/development/task_graph/veritas.toml" "$STAGING/examples/task_graph/"
cp "$ROOT/evaluation/development/inventory_reconcile/README.md" "$ROOT/evaluation/development/inventory_reconcile/main.vr" "$ROOT/evaluation/development/inventory_reconcile/reference.py" "$ROOT/evaluation/development/inventory_reconcile/veritas.toml" "$STAGING/examples/inventory_reconcile/"
cp "$ROOT/evaluation/development/inventory_reconcile/src/"*.vr "$STAGING/examples/inventory_reconcile/src/"
cp "$ROOT/evaluation/development/api_sync/README.md" "$ROOT/evaluation/development/api_sync/main.vr" "$ROOT/evaluation/development/api_sync/reference.py" "$ROOT/evaluation/development/api_sync/veritas.toml" "$STAGING/examples/api_sync/"
cp "$ROOT/evaluation/development/api_sync/src/"*.vr "$STAGING/examples/api_sync/src/"
cp "$ROOT/evaluation/development/package_analysis/README.md" "$ROOT/evaluation/development/package_analysis/main.vr" "$ROOT/evaluation/development/package_analysis/analysis.py" "$ROOT/evaluation/development/package_analysis/reference.py" "$ROOT/evaluation/development/package_analysis/dataset.json" "$ROOT/evaluation/development/package_analysis/veritas.toml" "$ROOT/evaluation/development/package_analysis/veritas.lock" "$STAGING/examples/package_analysis/"
cp "$ROOT/evaluation/development/package_analysis/.veritas/python-bridge/pyproject.toml" "$ROOT/evaluation/development/package_analysis/.veritas/python-bridge/uv.lock" "$STAGING/examples/package_analysis/.guion/python-bridge/"
rename_legacy_examples() {
  python3 - "$STAGING/examples" <<'PY'
from pathlib import Path
import os
import sys

root = Path(sys.argv[1])
for current, directories, files in os.walk(root, topdown=False):
    base = Path(current)
    for name in files:
        source = base / name
        target_name = name
        if target_name.endswith('.vr'):
            target_name = target_name[:-3] + '.gu'
        elif target_name == 'veritas.toml':
            target_name = 'guion.toml'
        elif target_name == 'veritas.lock':
            target_name = 'guion.lock'
        elif target_name == 'veritas_feed.py':
            target_name = 'guion_feed.py'
        target = base / target_name
        if target != source:
            source.rename(target)
    for name in directories:
        if name == '.veritas':
            (base / name).rename(base / '.guion')

for path in root.rglob('*'):
    if not path.is_file() or path.suffix not in {'.gu', '.toml', '.md', '.py', '.json', '.txt'} and path.name != '.gitignore':
        continue
    try:
        text = path.read_text(encoding='utf-8')
    except (UnicodeDecodeError, OSError):
        continue
    updated = (text.replace('VERITAS', 'GUION')
                   .replace('Veritas', 'Guion')
                   .replace('veritas', 'guion')
                   .replace('.vrb', '.gub')
                   .replace('.vr', '.gu')
                   .replace('.veritas', '.guion'))
    if updated != text:
        path.write_text(updated, encoding='utf-8')
PY
}
rename_legacy_examples
(
  cd "$STAGING"
  for PROJECT in examples/*; do
    if [ -f "$PROJECT/guion.toml" ] && grep -q '^\[python\]' "$PROJECT/guion.toml"; then
      GUION_UV=toolchains/uv ./guion sync --update "$PROJECT"
    fi
  done
  ./guion package examples/task_ledger --output examples/task_ledger_bundle
  ./guion verify-package examples/task_ledger_bundle
  ./guion package examples/task_graph --output examples/task_graph_bundle
  ./guion verify-package examples/task_graph_bundle
  printf '%s\n' '{"tasks":[{"id":"ship","depends_on":["build"]},{"id":"build","depends_on":[]}]}' > examples/task_graph_bundle/tasks.json
  (cd examples/task_graph_bundle && ./run tasks.json plan.json > /dev/null)
  python3 -c 'import json; assert json.load(open("examples/task_graph_bundle/plan.json")) == {"order": ["build", "ship"], "count": 2}'
  test -f examples/task_ledger_bundle/licenses/RUST-DEPENDENCIES.md
  test -d examples/task_ledger_bundle/licenses/rust
  cp -R examples/task_ledger_bundle "$TEMP/task-ledger-tampered"
  NOTICE_FILE=$(find "$TEMP/task-ledger-tampered/licenses/rust" -type f -print -quit)
  if [ -z "$NOTICE_FILE" ]; then
    echo "Rust notice tree is unexpectedly empty" >&2
    exit 1
  fi
  printf '%s\n' 'tampered notice' >> "$NOTICE_FILE"
  if ./guion verify-package "$TEMP/task-ledger-tampered" > /dev/null 2> "$TEMP/tamper-error.txt"; then
    echo "tampered Rust notice unexpectedly passed package verification" >&2
    exit 1
  fi
  if ! grep -q "Rust dependency notice integrity check failed" "$TEMP/tamper-error.txt"; then
    cat "$TEMP/tamper-error.txt" >&2
    echo "Rust notice tampering failed for an unexpected reason" >&2
    exit 1
  fi
  GUION_UV="$STAGING/toolchains/uv" ./guion package examples/python_interop --output examples/python_interop_bundle
  GUION_UV="$STAGING/toolchains/uv" ./guion verify-package examples/python_interop_bundle
  GUION_UV="$STAGING/toolchains/uv" ./guion package examples/package_analysis --output examples/package_analysis_bundle
  GUION_UV="$STAGING/toolchains/uv" ./guion verify-package examples/package_analysis_bundle
)
cp "$ROOT/evaluation/development/feed_inbox/README.md" "$ROOT/evaluation/development/feed_inbox/feed_inbox.py" "$ROOT/evaluation/development/feed_inbox/python_baseline.py" "$ROOT/evaluation/development/feed_inbox/main.vr" "$ROOT/evaluation/development/feed_inbox/veritas.toml" "$ROOT/evaluation/development/feed_inbox/veritas.lock" "$STAGING/examples/feed_inbox/"
cp "$ROOT/evaluation/development/feed_inbox/.veritas/python-bridge/pyproject.toml" "$ROOT/evaluation/development/feed_inbox/.veritas/python-bridge/uv.lock" "$STAGING/examples/feed_inbox/.guion/python-bridge/"
cp "$ROOT/evaluation/development/feed_inbox/src/"*.vr "$STAGING/examples/feed_inbox/src/"
cp "$ROOT/evaluation/development/feed_inbox/examples/"*.xml "$STAGING/examples/feed_inbox/examples/"
rename_legacy_examples
(
  cd "$STAGING"
  GUION_UV=toolchains/uv ./guion build examples/feed_inbox
)
for PROJECT in "$STAGING"/examples/*; do
  case "$(basename "$PROJECT")" in
    *_bundle) continue ;;
  esac
  PYTHON_ENV="$PROJECT/.guion/python-bridge/.venv"
  if [ -d "$PYTHON_ENV" ]; then
    rm -rf "$PYTHON_ENV"
  fi
done
if [ "$OS-$ARCH" = "linux-aarch64" ]; then
  python3 "$ROOT/distribution/audit_linux_dependencies.py" "$STAGING" --output "$STAGING/linux-dependencies.json"
  PLATFORM_NOTE="Supported Linux host: Debian 12-compatible GNU/Linux ARM64 with glibc; other distributions, libc implementations, and architectures are unverified"
else
  PLATFORM_NOTE="Supported host: macOS ARM64; Linux is distributed as a separate target package"
fi
printf '%s\n' \
  "Name: $NAME" \
  "Host: $OS-$ARCH" \
  "$PLATFORM_NOTE" \
  "Pinned toolchain: uv $UV_VERSION ($UV_TARGET)" \
  "Runtime: Rust-hosted Guion bytecode VM" \
  "Included: CLI binary, local reference/specification, executable examples including reconciliation, API sync, package-backed analysis, feeds, task ledger and task graph planning; source-free task-ledger, task-graph, NumPy/orjson interop and analysis app bundles" \
  "Python interop: bundled uv $UV_VERSION provisions the pinned managed CPython bridge on guion sync" \
  "Evaluation: review-pending briefs and executable cases, candidate-agnostic runner with source-free delivery acceptance, reviewer packet, choice-safe product-information bundle, matched-build and choice prompts, evidence protocols, and Python/NumPy FFI stress runner included under evaluation/" \
  "Managed environments: per-project uv.lock and veritas.lock; CPython is acquired on first sync" \
  "Cross-host portability: not included; package requires its named operating system, architecture, and runtime ABI" \
  "Third-party notices: uv, Tcl/Tk and target-specific Rust notices in licenses/; app bundles carry Rust and Python dependency provenance plus active and cached native image records" \
  "Linux binary dependency audit: linux-dependencies.json (included in Linux packages)" \
  "Install: sh install.sh [PREFIX] installs an isolated versioned tree and a PATH command; it does not edit shell startup files" \
  "Package integrity: recorded hashes are verified by the application launcher; unsigned archive does not authenticate its publisher" \
  "Verification: see doctor.txt; this package is not signed or notarized" \
  > "$STAGING/PACKAGE.txt"
mv "$STAGING" "$OUT/$NAME"
(cd "$OUT/$NAME" && GUION_UV=toolchains/uv ./guion doctor) > "$OUT/$NAME/doctor.txt"
INSTALL_PREFIX="$TEMP/Guion install 🔬"
sh "$OUT/$NAME/install.sh" "$INSTALL_PREFIX"
INSTALLED_ROOT="$INSTALL_PREFIX/share/guion/$NAME"
"$INSTALL_PREFIX/bin/guion" verify-package "$INSTALLED_ROOT/examples/python_interop_bundle"
"$INSTALL_PREFIX/bin/guion" verify-package "$INSTALLED_ROOT/examples/package_analysis_bundle"
"$INSTALLED_ROOT/examples/python_interop_bundle/run" > "$TEMP/installed-run-1.txt"
"$INSTALLED_ROOT/examples/python_interop_bundle/run" > "$TEMP/installed-run-2.txt"
cmp "$TEMP/installed-run-1.txt" "$TEMP/installed-run-2.txt"
grep -qx 15 "$TEMP/installed-run-1.txt"
(cd "$INSTALLED_ROOT/examples/package_analysis_bundle" && \
  UV_OFFLINE=1 ./run dataset.json .acceptance-report.json > "$TEMP/installed-package-analysis.txt")
test "$(cat "$TEMP/installed-package-analysis.txt")" = "$(printf 'Rows:\n3\nTotal:\n41')"
python3 - "$INSTALLED_ROOT/examples/package_analysis_bundle/.acceptance-report.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as report_file:
    report = json.load(report_file)
assert report["count"] == 3 and report["total"] == 41, report
PY
rm "$INSTALLED_ROOT/examples/package_analysis_bundle/.acceptance-report.json"
(cd "$INSTALLED_ROOT" && \
  GUION_BINARY="$INSTALLED_ROOT/guion" \
  GUION_INSTALL_ROOT="$INSTALLED_ROOT" \
  GUION_UV="$INSTALLED_ROOT/toolchains/uv" \
  UV_CACHE_DIR="${UV_CACHE_DIR:-${TMPDIR:-/tmp}/guion-uv-interop}" \
  GUION_PACKAGE_UV_CACHE="${GUION_PACKAGE_UV_CACHE:-${TMPDIR:-/tmp}/guion-package-uv-cache}" \
  UV_PYTHON_INSTALL_DIR="${UV_PYTHON_INSTALL_DIR:-${TMPDIR:-/tmp}/guion-uv-python}" \
  python3 -m unittest \
    evaluation.tests.test_shipped_docs_repair \
    evaluation.tests.test_independent_kit \
    evaluation.tests.test_case_runner \
    evaluation.tests.test_inventory_reconcile \
    evaluation.tests.test_api_sync \
    evaluation.tests.test_task_graph \
    evaluation.tests.test_package_analysis \
    evaluation.tests.test_famous_benchmark_sources \
    evaluation.tests.test_richards_veritas)
COPYFILE_DISABLE=1 tar -czf "$TEMP/$NAME.tar.gz" -C "$OUT" "$NAME"
python3 "$ROOT/distribution/verify_release_archive.py" "$TEMP/$NAME.tar.gz" "$NAME"
if command -v shasum >/dev/null 2>&1; then
  (cd "$TEMP" && shasum -a 256 "$NAME.tar.gz") > "$TEMP/$NAME.tar.gz.sha256"
elif command -v sha256sum >/dev/null 2>&1; then
  (cd "$TEMP" && sha256sum "$NAME.tar.gz") > "$TEMP/$NAME.tar.gz.sha256"
else
  echo "no SHA-256 utility found (need shasum or sha256sum)" >&2
  exit 1
fi
mv "$TEMP/$NAME.tar.gz" "$OUT/$NAME.tar.gz"
mv "$TEMP/$NAME.tar.gz.sha256" "$OUT/$NAME.tar.gz.sha256"
printf 'created %s\n' "$OUT/$NAME.tar.gz"
