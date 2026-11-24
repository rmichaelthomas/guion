#!/bin/sh
# Certificates for the name, validity, PSS salt and algorithm-consistency tests. One P-256 CA and one
# RSA CA, both throwaway; their keys are discarded. Needs OpenSSL 3.
set -eu
cd "$(dirname "$0")"
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT

openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$T/nca.key" 2>/dev/null
openssl req -new -x509 -key "$T/nca.key" -subj '/O=Guion Test/CN=Name CA' -days 36500 -sha256 -out "$T/nca.pem" 2>/dev/null
openssl x509 -in "$T/nca.pem" -outform DER -out name-ca.der

openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$T/leaf.key" 2>/dev/null
leaf() { # out subject-dn days [extfile]
  out=$1; subj=$2; days=$3; ext=${4:-}
  openssl req -new -key "$T/leaf.key" -subj "$subj" -out "$T/leaf.csr" 2>/dev/null
  if [ -n "$ext" ]; then
    openssl x509 -req -in "$T/leaf.csr" -CA "$T/nca.pem" -CAkey "$T/nca.key" -CAcreateserial -days "$days" -sha256 \
      -extfile "$ext" -outform DER -out "$out.der" 2>/dev/null
  else
    openssl x509 -req -in "$T/leaf.csr" -CA "$T/nca.pem" -CAkey "$T/nca.key" -CAcreateserial -days "$days" -sha256 \
      -outform DER -out "$out.der" 2>/dev/null
  fi
}
printf 'subjectAltName=DNS:a.example.test,DNS:b.example.test,IP:192.0.2.1,email:x@example.test\n' > "$T/san.ext"
leaf leaf-san '/O=Guion Test/CN=cn.example.test' 36500 "$T/san.ext"
leaf leaf-cn-only '/O=Guion Test/CN=cn-only.example.test' 36500
leaf leaf-no-names '/O=Guion Test' 36500
printf 'subjectAltName=IP:192.0.2.1\n' > "$T/ip.ext"
leaf leaf-san-ip-only '/O=Guion Test/CN=ignored.example.test' 36500 "$T/ip.ext"

# A certificate with a short, known validity window (notBefore/notAfter pinned with -not_before/-not_after).
openssl req -new -key "$T/leaf.key" -subj '/O=Guion Test/CN=dated.example.test' -out "$T/leaf.csr" 2>/dev/null
openssl x509 -req -in "$T/leaf.csr" -CA "$T/nca.pem" -CAkey "$T/nca.key" -CAcreateserial -sha256 \
  -not_before 20240102030405Z -not_after 20250607080910Z -outform DER -out leaf-dated.der 2>/dev/null

# RSASSA-PSS with a salt length that is not the digest length.
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "$T/rca.key" 2>/dev/null
openssl req -new -x509 -key "$T/rca.key" -subj '/O=Guion Test/CN=Name RSA CA' -days 36500 -sha256 -out "$T/rca.pem" 2>/dev/null
openssl x509 -in "$T/rca.pem" -outform DER -out name-rsa-ca.der
openssl req -new -key "$T/leaf.key" -subj '/O=Guion Test/CN=pss-salt' -out "$T/leaf.csr" 2>/dev/null
for salt in 0 20 64; do
  openssl x509 -req -in "$T/leaf.csr" -CA "$T/rca.pem" -CAkey "$T/rca.key" -CAcreateserial -days 36500 -sha256 \
    -sigopt rsa_padding_mode:pss -sigopt rsa_pss_saltlen:$salt -sigopt rsa_mgf1_md:sha256 \
    -outform DER -out leaf-pss-salt$salt.der 2>/dev/null
done
openssl x509 -req -in "$T/leaf.csr" -CA "$T/rca.pem" -CAkey "$T/rca.key" -CAcreateserial -days 36500 -sha256 \
  -outform DER -out leaf-name-rsa-sha256.der 2>/dev/null
