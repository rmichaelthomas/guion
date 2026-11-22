#!/bin/sh
# leaf-gentime.der: a throwaway self-issued P-256 certificate valid 2049-12-31T23:59:59Z (UTCTime, the
# last second RFC 5280 allows it) to 2055-01-01T00:00:00Z (GeneralizedTime, because the year is 2050 or
# later). It exercises the GeneralizedTime path of x509_validity. Only the validity matters to the test;
# the key is discarded. Kept apart from make_name_fixtures.sh so rerunning that script does not change
# the other fixtures. Needs OpenSSL 3.
set -eu
cd "$(dirname "$0")"
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$T/k.key" 2>/dev/null
openssl req -new -key "$T/k.key" -subj '/O=Guion Test/CN=gentime.example.test' -out "$T/k.csr" 2>/dev/null
openssl x509 -req -in "$T/k.csr" -signkey "$T/k.key" -sha256 \
  -not_before 20491231235959Z -not_after 20550101000000Z -outform DER -out leaf-gentime.der 2>/dev/null
