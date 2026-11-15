#!/bin/sh
# Install the latest Guion release for macOS or Debian-compatible Linux.
#
#   curl -fsSL https://raw.githubusercontent.com/rmichaelthomas/guion/main/distribution/get-guion.sh | sh
#   sh get-guion.sh [--version X.Y.Z] [--prefix DIR]
#
# It downloads the release archive for this machine and its .sha256 file, checks the checksum,
# and runs the archive's own install.sh (default prefix ~/.local). Nothing else is touched.
set -eu

version=latest
prefix=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --version) version=$2; shift 2 ;;
    --prefix) prefix=$2; shift 2 ;;
    *) echo "usage: get-guion.sh [--version X.Y.Z] [--prefix DIR]" >&2; exit 2 ;;
  esac
done

os=$(uname -s)
machine=$(uname -m)
case "$os-$machine" in
  Darwin-arm64) slug=macos-aarch64 ;;
  Darwin-x86_64) slug=macos-x86_64 ;;
  Linux-x86_64) slug=debian-x86_64 ;;
  Linux-aarch64|Linux-arm64) slug=debian-aarch64 ;;
  *) echo "no Guion build for $os $machine (Windows: download the .zip from the releases page)" >&2; exit 1 ;;
esac

base=https://github.com/rmichaelthomas/guion/releases
if [ "$version" = latest ]; then
  url=$base/latest/download
else
  url=$base/download/v${version#v}
fi
archive=guion-$slug.tar.gz

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
echo "Downloading $archive ($version)"
curl -fsSL -o "$tmp/$archive" "$url/$archive"
curl -fsSL -o "$tmp/$archive.sha256" "$url/$archive.sha256"
(
  cd "$tmp"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c "$archive.sha256"
  else
    shasum -a 256 -c "$archive.sha256"
  fi
)
tar -xzf "$tmp/$archive" -C "$tmp"

target=${prefix:-$HOME/.local}
# Re-running upgrades: remove the earlier copy this script installed, then install again.
if [ -d "$target/share/guion/guion-$slug" ]; then
  rm -rf "$target/share/guion/guion-$slug"
fi
sh "$tmp/guion-$slug/install.sh" "$target"
