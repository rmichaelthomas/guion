#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
NAME=$(basename "$ROOT")
case "$NAME" in
  *[!A-Za-z0-9._-]*|"")
    echo "distribution directory name must contain only letters, digits, dots, underscores, and hyphens" >&2
    exit 2
    ;;
esac

if [ "$#" -gt 1 ]; then
  echo "Usage: sh install.sh [PREFIX]" >&2
  exit 2
fi
if [ "$#" -eq 1 ]; then
  PREFIX=$1
else
  if [ -z "${HOME:-}" ]; then
    echo "HOME is not set; pass an install prefix explicitly" >&2
    exit 2
  fi
  PREFIX="$HOME/.local"
fi

if [ -x "$ROOT/bin/veritas" ]; then
  RUNTIME_RELATIVE="bin/veritas"
elif [ -x "$ROOT/veritas" ]; then
  RUNTIME_RELATIVE="veritas"
else
  echo "Veritas runtime is missing or not executable: $ROOT/bin/veritas" >&2
  exit 1
fi
mkdir -p "$PREFIX"
PREFIX=$(CDPATH= cd -- "$PREFIX" && pwd)
BIN="$PREFIX/bin"
DATA_PARENT="$PREFIX/share/veritas"
DEST="$DATA_PARENT/$NAME"
LAUNCHER="$BIN/veritas"

if [ -e "$DEST" ]; then
  echo "Veritas $NAME already exists at $DEST; choose another prefix or remove that version deliberately" >&2
  exit 1
fi
if [ -L "$LAUNCHER" ] || { [ -e "$LAUNCHER" ] && { [ ! -f "$LAUNCHER" ] || ! grep -q '# Veritas managed launcher' "$LAUNCHER"; }; }; then
  echo "refusing to replace an existing command at $LAUNCHER" >&2
  exit 1
fi

mkdir -p "$BIN" "$DATA_PARENT"
STAGE=$(mktemp -d "$DATA_PARENT/.$NAME.install.XXXXXX")
LAUNCHER_TMP="$BIN/.veritas-launcher.$$"
cleanup() {
  if [ -n "${STAGE:-}" ] && [ -d "$STAGE" ]; then
    rm -rf "$STAGE"
  fi
  if [ -n "${LAUNCHER_TMP:-}" ] && [ -e "$LAUNCHER_TMP" ]; then
    rm -f "$LAUNCHER_TMP"
  fi
}
trap cleanup EXIT HUP INT TERM

cp -R "$ROOT/." "$STAGE/"
mv "$STAGE" "$DEST"
STAGE=

cat > "$LAUNCHER_TMP" <<LAUNCHER
#!/bin/sh
# Veritas managed launcher
set -eu
HERE=\$(CDPATH= cd -- "\$(dirname -- "\$0")" && pwd)
exec "\$HERE/../share/veritas/$NAME/$RUNTIME_RELATIVE" "\$@"
LAUNCHER
chmod 755 "$LAUNCHER_TMP"
mv -f "$LAUNCHER_TMP" "$LAUNCHER"
LAUNCHER_TMP=

printf 'Installed Veritas %s\n' "$NAME"
printf 'Command: %s\n' "$LAUNCHER"
case ":${PATH:-}:" in
  *":$BIN:"*) ;;
  *) printf 'Add %s to PATH to invoke `veritas` from any directory.\n' "$BIN" ;;
esac
