#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
TEMP=$(mktemp -d "${TMPDIR:-/tmp}/veritas-install-test.XXXXXX")
trap 'rm -rf "$TEMP"' EXIT HUP INT TERM
PACKAGE="$TEMP/veritas-fixture-0.1.0-test"
PREFIX="$TEMP/Veritas Prefix 🔬"
mkdir -p "$PACKAGE/toolchains" "$PACKAGE/licenses/rust/example-1.0" "$PACKAGE/docs"
cat > "$PACKAGE/veritas" <<'RUNTIME'
#!/bin/sh
printf 'runtime\n'
printf '<%s>\n' "$@"
RUNTIME
chmod 755 "$PACKAGE/veritas"
printf '%s\n' 'fixture notice' > "$PACKAGE/licenses/rust/example-1.0/LICENSE-MIT"
printf '%s\n' 'fixture reference' > "$PACKAGE/docs/reference.md"
cp "$ROOT/distribution/install.sh" "$PACKAGE/install.sh"

sh "$PACKAGE/install.sh" "$PREFIX"

DATA="$PREFIX/share/veritas/$(basename "$PACKAGE")"
test -x "$PREFIX/bin/veritas"
test -x "$DATA/veritas"
test -f "$DATA/licenses/rust/example-1.0/LICENSE-MIT"
test -f "$DATA/docs/reference.md"
OUTPUT=$("$PREFIX/bin/veritas" run 'argument with spaces')
EXPECTED=$(printf 'runtime\n<run>\n<argument with spaces>')
test "$OUTPUT" = "$EXPECTED"

if sh "$PACKAGE/install.sh" "$PREFIX" >"$TEMP/reinstall.out" 2>"$TEMP/reinstall.err"; then
  echo "reinstall unexpectedly replaced an existing version" >&2
  exit 1
fi
grep -q "already exists" "$TEMP/reinstall.err"

FOREIGN_PREFIX="$TEMP/foreign"
mkdir -p "$FOREIGN_PREFIX/bin"
printf '%s\n' '#!/bin/sh' 'echo user program' > "$FOREIGN_PREFIX/bin/veritas"
chmod 755 "$FOREIGN_PREFIX/bin/veritas"
if sh "$PACKAGE/install.sh" "$FOREIGN_PREFIX" >"$TEMP/foreign.out" 2>"$TEMP/foreign.err"; then
  echo "installer unexpectedly replaced an unrelated command" >&2
  exit 1
fi
grep -q "refusing to replace" "$TEMP/foreign.err"

echo "installer relocation, arguments, notices, and collision checks passed"
