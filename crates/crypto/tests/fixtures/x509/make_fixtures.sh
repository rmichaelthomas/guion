#!/bin/sh
# Regenerates the issuer-check fixtures: certificates (DER) signed by throwaway CAs. The private keys
# are deleted at the end; only certificates are kept. Needs OpenSSL 3.
set -eu
cd "$(dirname "$0")"
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
SUBJ_CA='/O=Guion Test/CN=Issuer CA'

ca() { # name keyalg-args...
  name=$1; shift
  openssl genpkey "$@" -out "$T/$name.key" 2>/dev/null
  openssl req -new -x509 -key "$T/$name.key" -subj "$SUBJ_CA" -days 36500 -out "$T/$name.pem" "${DIGEST:--sha256}" 2>/dev/null
  openssl x509 -in "$T/$name.pem" -outform DER -out "$name-ca.der"
}
leaf() { # ca-name out-name digest-flag [extra -sigopt args...]
  ca=$1; out=$2; digest=$3; shift 3
  openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$T/leaf.key" 2>/dev/null
  openssl req -new -key "$T/leaf.key" -subj "/O=Guion Test/CN=$out" -out "$T/leaf.csr" 2>/dev/null
  openssl x509 -req -in "$T/leaf.csr" -CA "$T/$ca.pem" -CAkey "$T/$ca.key" -CAcreateserial -days 36500 \
    "$digest" "$@" -outform DER -out "$out.der" 2>/dev/null
}

ca rsa -algorithm RSA -pkeyopt rsa_keygen_bits:2048
leaf rsa leaf-rsa-sha256 -sha256
leaf rsa leaf-rsa-sha384 -sha384
leaf rsa leaf-rsa-sha512 -sha512
leaf rsa leaf-rsa-pss-sha256 -sha256 -sigopt rsa_padding_mode:pss -sigopt rsa_pss_saltlen:32 -sigopt rsa_mgf1_md:sha256
leaf rsa leaf-rsa-pss-sha384 -sha384 -sigopt rsa_padding_mode:pss -sigopt rsa_pss_saltlen:48 -sigopt rsa_mgf1_md:sha384
leaf rsa leaf-rsa-sha1 -sha1
# An unrelated RSA CA with the same subject name: the name chains, the signature does not.
ca rsa-other -algorithm RSA -pkeyopt rsa_keygen_bits:2048

ca p256 -algorithm EC -pkeyopt ec_paramgen_curve:P-256
leaf p256 leaf-p256-sha256 -sha256
leaf p256 leaf-p256-sha384 -sha384
ca p384 -algorithm EC -pkeyopt ec_paramgen_curve:P-384
leaf p384 leaf-p384-sha384 -sha384
leaf p384 leaf-p384-sha256 -sha256
ca p521 -algorithm EC -pkeyopt ec_paramgen_curve:P-521
leaf p521 leaf-p521-sha512 -sha512
ca ed25519 -algorithm ED25519
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$T/leaf.key" 2>/dev/null
openssl req -new -key "$T/leaf.key" -subj "/O=Guion Test/CN=leaf-ed25519" -out "$T/leaf.csr" 2>/dev/null
openssl x509 -req -in "$T/leaf.csr" -CA "$T/ed25519.pem" -CAkey "$T/ed25519.key" -CAcreateserial -days 36500 -outform DER -out leaf-ed25519.der 2>/dev/null
