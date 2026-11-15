#!/bin/sh
# Lay out a package directory around a prebuilt `guion` binary: bin/guion (or guion.exe), the
# standard library under share/guion/stdlib (where the binary looks for it), the licences, the
# docs the library points to, and install.sh. Used for the Intel Mac and Windows archives, which
# are not built by `cargo xtask release`.
#
# usage: stage-binary-package.sh SLUG BINARY OUTPUT_DIRECTORY
set -eu
slug=$1
binary=$2
out=$3
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
dest="$out/guion-$slug"
rm -rf "$dest"
mkdir -p "$dest/bin" "$dest/share/guion/docs"
case "$binary" in
  *.exe) cp "$binary" "$dest/bin/guion.exe" ;;
  *) cp "$binary" "$dest/bin/guion"; chmod 755 "$dest/bin/guion" ;;
esac
cp -R "$root/language/stdlib" "$dest/share/guion/stdlib"
cp "$root/distribution/README.md" "$dest/share/guion/README.md"
cp "$root/LICENSE" "$root/THIRD-PARTY-NOTICES.md" "$dest/share/guion/"
cp "$root"/docs/*.md "$dest/share/guion/docs/"
cp "$root/docs/agents/guion-reference.md" "$dest/bin/guion-reference.md"
case "$binary" in
  *.exe) ;;
  *) cp "$root/distribution/install.sh" "$dest/install.sh" ;;
esac
echo "$dest"
