#!/bin/sh
# usage: render-homebrew-formula.sh VERSION > guion.rb
# Reads each archive's .sha256 from the published GitHub release v VERSION.
set -eu
version=${1:?version}
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
sha() {
  curl -fsSL "https://github.com/rmichaelthomas/guion/releases/download/v$version/guion-$1.tar.gz.sha256" | awk '{print $1}'
}
sed -e "s/@VERSION@/$version/g" \
    -e "s/@SHA_MACOS_AARCH64@/$(sha macos-aarch64)/" \
    -e "s/@SHA_MACOS_X86_64@/$(sha macos-x86_64)/" \
    -e "s/@SHA_DEBIAN_AARCH64@/$(sha debian-aarch64)/" \
    -e "s/@SHA_DEBIAN_X86_64@/$(sha debian-x86_64)/" \
    "$root/distribution/homebrew/guion.rb.in"
