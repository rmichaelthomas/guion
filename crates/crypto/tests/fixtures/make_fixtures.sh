#!/bin/sh
# Generates the signature and certificate fixtures with openssl. Run once; the outputs are
# committed and the tests read them, they never invoke openssl. Private keys live in a temporary
# directory and are discarded, so rerunning replaces every fixture with a fresh set.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cd "$here"

printf 'guion crypto fixture message' > message.bin

# RSA 2048: modulus and exponent as hex, and three signatures over message.bin.
openssl genrsa -out "$tmp/rsa.pem" 2048 2>/dev/null
openssl rsa -in "$tmp/rsa.pem" -noout -modulus | sed 's/^Modulus=//' | tr 'A-F' 'a-f' | tr -d '\n' > rsa_modulus.hex
printf '010001' > rsa_exponent.hex
openssl dgst -sha256 -sign "$tmp/rsa.pem" -out rsa_pkcs1_sha256.sig message.bin
openssl dgst -sha512 -sign "$tmp/rsa.pem" -out rsa_pkcs1_sha512.sig message.bin
openssl dgst -sha256 -sign "$tmp/rsa.pem" -sigopt rsa_padding_mode:pss -sigopt rsa_pss_saltlen:32 \
  -sigopt rsa_mgf1_md:sha256 -out rsa_pss_sha256.sig message.bin

# ECDSA P-256: SEC1 uncompressed public key (65 bytes) and the signature as raw r||s (64 bytes).
openssl ecparam -name prime256v1 -genkey -noout -out "$tmp/ec.pem" 2>/dev/null
openssl ec -in "$tmp/ec.pem" -pubout -outform DER 2>/dev/null | tail -c 65 > ec_public_sec1.bin
openssl dgst -sha256 -sign "$tmp/ec.pem" -out "$tmp/ec_sig.der" message.bin
python3 -I - "$tmp/ec_sig.der" ec_signature_rs.bin <<'PY'
import sys
d = open(sys.argv[1], 'rb').read()
assert d[0] == 0x30
i = 2 if d[1] < 0x80 else 2 + (d[1] & 0x7F)
parts = []
for _ in range(2):
    assert d[i] == 0x02
    n = d[i + 1]
    v = d[i + 2:i + 2 + n]
    i += 2 + n
    parts.append(v.lstrip(b'\x00').rjust(32, b'\x00'))
open(sys.argv[2], 'wb').write(parts[0] + parts[1])
PY

# X.509: one RSA self-signed certificate and one EC certificate.
openssl req -x509 -new -key "$tmp/rsa.pem" -sha256 -days 36500 -subj '/CN=guion rsa fixture' -out rsa_cert.pem
openssl x509 -in rsa_cert.pem -outform DER -out rsa_cert.der
openssl req -x509 -new -key "$tmp/ec.pem" -sha256 -days 36500 -subj '/CN=guion ec fixture' -out ec_cert.pem
openssl x509 -in ec_cert.pem -outform DER -out ec_cert.der
rm -f ec_cert.pem
