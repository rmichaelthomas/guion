#!/bin/sh
# Generates the 8192-bit RSA fixture (modulus, exponent, PKCS#1 v1.5 SHA-256 signature over
# message.bin). Key generation takes a while. The private key is discarded.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cd "$here"
openssl genrsa -out "$tmp/rsa8192.pem" 8192 2>/dev/null
openssl rsa -in "$tmp/rsa8192.pem" -noout -modulus | sed 's/^Modulus=//' | tr 'A-F' 'a-f' | tr -d '\n' > rsa8192_modulus.hex
openssl dgst -sha256 -sign "$tmp/rsa8192.pem" -out rsa8192_pkcs1_sha256.sig message.bin
