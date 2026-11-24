#!/bin/sh
# Rebuilds local/negative/: one token per check of tsa_verify that is well signed but wrong in
# exactly that one way, so each check has a test that fails without it (see the mutation script
# crates/crypto/tests/mutate_tsa.py). Separate from make.sh so the tokens there keep their genTime.
# Everything here is test-only: a throwaway CA and TSA keys, which are not kept. Run from this
# directory:  sh make-negatives.sh
# Tokens carry the wall-clock time of the run; the tests only compare them with `rebuilt.tsr`
# (which verifies) and expect Some, None or an error code, so nothing else needs updating.
set -eu
rm -rf negative-work && mkdir negative-work && cd negative-work
mkdir -p ca/newcerts && : > ca/index.txt && echo 2000 > ca/serial

cat > ca.cnf <<'EOF'
[ca]
default_ca = CA_default
[CA_default]
dir = ./ca
database = $dir/index.txt
new_certs_dir = $dir/newcerts
serial = $dir/serial
default_md = sha256
policy = policy_any
unique_subject = no
copy_extensions = none
[policy_any]
commonName = supplied
[req]
distinguished_name = dn
prompt = no
[dn]
CN = x
[v3_ca]
basicConstraints = critical,CA:true
keyUsage = critical,keyCertSign,cRLSign
subjectKeyIdentifier = hash
[v3_tsa]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,timeStamping
subjectKeyIdentifier = hash
EOF

openssl genrsa -out ca.key 2048 2>/dev/null
openssl req -new -x509 -key ca.key -sha256 -subj '/CN=Guion Negative TSA CA' -days 36500 \
  -config ca.cnf -extensions v3_ca -out ca.pem

sign() { # name startdate enddate (the key is already in name.key)
  openssl req -new -key "$1.key" -subj "/CN=Guion Negative $1" -config ca.cnf -out "$1.csr"
  openssl ca -batch -config ca.cnf -cert ca.pem -keyfile ca.key -in "$1.csr" -out "$1.pem" \
    -notext -startdate "$2" -enddate "$3" -extensions v3_tsa 2>/dev/null
}
openssl genrsa -out tsa.key 2048 2>/dev/null;                     sign tsa 20260101000000Z 20460101000000Z
# Valid only from 2030: a token from today is before its notBefore.
openssl genrsa -out tsa-future.key 2048 2>/dev/null;              sign tsa-future 20300101000000Z 20460101000000Z
openssl ecparam -name prime256v1 -genkey -noout -out tsa-p256.key; sign tsa-p256 20260101000000Z 20460101000000Z
# Keys tsa_verify does not support for a TSA: P-521 and Ed25519.
openssl ecparam -name secp521r1 -genkey -noout -out tsa-p521.key;  sign tsa-p521 20260101000000Z 20460101000000Z
openssl genpkey -algorithm ed25519 -out tsa-ed25519.key;          sign tsa-ed25519 20260101000000Z 20460101000000Z

cat > tsa.cnf <<'EOF'
oid_section = oids
[oids]
tsa_policy1 = 1.2.3.4.1
[tsa]
default_tsa = tsa_config
[tsa_config]
serial = ./tsa.serial
crypto_device = builtin
signer_cert = tsa.pem
certs = tsa.pem
signer_key = tsa.key
signer_digest = sha256
default_policy = tsa_policy1
digests = sha512
ordering = no
tsa_name = no
ess_cert_id_chain = no
ess_cert_id_alg = sha256
EOF
echo 01 > tsa.serial
# The request is local/request.tsq: the SHA-512 of local/message.txt.
openssl ts -reply -config tsa.cnf -section tsa_config -queryfile ../request.tsq -out base.tsr 2>/dev/null

forge() { # out signer [options]
  out=$1; signer=$2; shift 2
  python3 ../forge.py base.tsr "$signer.pem" "$signer.key" "$out.tsr" "$@"
}
forge rebuilt tsa
forge imprint-sha3 tsa --imprint-oid 2.16.840.1.101.3.4.2.10
forge imprint-params tsa --imprint-params
forge digest-params tsa --digest-params
forge content-type-data tsa --content-type 1.2.840.113549.1.7.1
forge sid-other-serial tsa --sid-serial 7
forge outer-oid tsa --outer-oid 1.2.840.113549.1.7.1
forge econtent-type tsa --econtent-type 1.2.840.113549.1.7.1
forge two-signers tsa --two-signers
forge duplicate-content-type tsa --duplicate content-type
forge duplicate-message-digest tsa --duplicate message-digest
forge duplicate-ess tsa --duplicate ess
forge rsa-sha384-names tsa --sig-alg-hash sha384
forge pss-sha384-names tsa --pss --sig-alg-hash sha384
forge ecdsa-sha384-names tsa-p256 --sig-alg-hash sha384
forge future tsa-future
forge p521 tsa-p521 --digest sha512
forge ed25519 tsa-ed25519

rm -rf ../negative && mkdir ../negative
cp ca.pem tsa.pem tsa-future.pem tsa-p256.pem tsa-p521.pem tsa-ed25519.pem ./*.tsr ../negative
cd .. && rm -rf negative-work
