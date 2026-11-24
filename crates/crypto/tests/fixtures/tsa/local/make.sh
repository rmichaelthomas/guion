#!/bin/sh
# Rebuilds the local test TSA fixtures. Everything here is test-only: the keys are throwaway
# and committed on purpose. Run from this directory:  sh make.sh
# Tokens carry the wall-clock time of the run, so the README records the genTime each one has;
# do not regenerate unless you also update crates/crypto/tests/tsa_vectors.rs and README.md.
set -eu
rm -rf work && mkdir work && cd work
mkdir -p ca/newcerts && : > ca/index.txt && echo 1000 > ca/serial

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
[v3_tsa_noeku]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
subjectKeyIdentifier = hash
EOF

# The CA (self-signed, 2048-bit RSA).
openssl genrsa -out ca.key 2048 2>/dev/null
openssl req -new -x509 -key ca.key -sha256 -subj '/CN=Guion Test TSA CA' -days 36500 \
  -config ca.cnf -extensions v3_ca -out ca.pem

issue() { # name subject startdate enddate extensions
  openssl genrsa -out "$1.key" 2048 2>/dev/null
  openssl req -new -key "$1.key" -subj "$2" -config ca.cnf -out "$1.csr"
  openssl ca -batch -config ca.cnf -cert ca.pem -keyfile ca.key -in "$1.csr" -out "$1.pem" \
    -notext -startdate "$3" -enddate "$4" -extensions "$5" 2>/dev/null
}
issue tsa1 '/CN=Guion Test TSA 1' 20260101000000Z 20460101000000Z v3_tsa
issue tsa2 '/CN=Guion Test TSA 2' 20260101000000Z 20460101000000Z v3_tsa
issue tsa-noeku '/CN=Guion Test TSA without EKU' 20260101000000Z 20460101000000Z v3_tsa_noeku
# TSA certificates with EC keys (P-256, P-384) and an RSA one that signs with PSS: the CA is RSA.
issue_ec() { # name subject curve
  openssl ecparam -name "$3" -genkey -noout -out "$1.key"
  openssl req -new -key "$1.key" -subj "$2" -config ca.cnf -out "$1.csr"
  openssl ca -batch -config ca.cnf -cert ca.pem -keyfile ca.key -in "$1.csr" -out "$1.pem" \
    -notext -startdate 20260101000000Z -enddate 20460101000000Z -extensions v3_tsa 2>/dev/null
}
issue_ec tsa-p256 '/CN=Guion Test TSA P-256' prime256v1
issue_ec tsa-p384 '/CN=Guion Test TSA P-384' secp384r1
issue tsa-expired '/CN=Guion Test TSA expired' 20200101000000Z 20210101000000Z v3_tsa

# A second, unrelated CA, to test a wrong issuer.
openssl genrsa -out other-ca.key 2048 2>/dev/null
openssl req -new -x509 -key other-ca.key -sha256 -subj '/CN=Guion Test Other CA' -days 36500 \
  -config ca.cnf -extensions v3_ca -out other-ca.pem

tsa_cnf() { # name cert key ess_alg precision accuracy(yes|no)
  cat > "$1.cnf" <<EOF
oid_section = oids
[oids]
tsa_policy1 = 1.2.3.4.1
[tsa]
default_tsa = tsa_config
[tsa_config]
serial = ./tsa.serial
crypto_device = builtin
signer_cert = $2
certs = $2
signer_key = $3
signer_digest = sha256
default_policy = tsa_policy1
other_policies = tsa_policy1
digests = sha256, sha512
$( [ "$6" = yes ] && echo 'accuracy = secs:1, millisecs:500, microsecs:100' )
clock_precision_digits = $5
ordering = no
tsa_name = no
ess_cert_id_chain = no
ess_cert_id_alg = $4
EOF
}
echo 01 > tsa.serial
printf 'Guion local TSA fixture message\n' > message.txt
printf 'Guion local TSA fixture, another message\n' > message2.txt
openssl ts -query -data message.txt -sha512 -cert -no_nonce -out request.tsq 2>/dev/null
openssl ts -query -data message2.txt -sha512 -cert -no_nonce -out request2.tsq 2>/dev/null
openssl ts -query -data message.txt -sha256 -cert -no_nonce -out request-sha256.tsq 2>/dev/null
openssl ts -query -data message.txt -sha512 -cert -no_nonce -tspolicy 1.2.3.99 -out request-badpolicy.tsq 2>/dev/null

reply() { # config request out
  openssl ts -reply -config "$1" -section tsa_config -queryfile "$2" -out "$3" 2>/dev/null
}
tsa_cnf v2 tsa1.pem tsa1.key sha256 0 no;       reply v2.cnf request.tsq good-v2.tsr
tsa_cnf v1 tsa1.pem tsa1.key sha1 0 no;         reply v1.cnf request.tsq good-v1.tsr
reply v2.cnf request2.tsq other-digest.tsr
reply v2.cnf request-sha256.tsq sha256-imprint.tsr
tsa_cnf t2 tsa2.pem tsa2.key sha256 0 no;       reply t2.cnf request.tsq tsa2.tsr
tsa_cnf ex tsa-expired.pem tsa-expired.key sha256 0 no; reply ex.cnf request.tsq expired.tsr
tsa_cnf ac tsa1.pem tsa1.key sha256 3 yes;      reply ac.cnf request.tsq accuracy.tsr
tsa_cnf p6 tsa1.pem tsa1.key sha256 6 no;       reply p6.cnf request.tsq precision6.tsr
reply v2.cnf request-badpolicy.tsq rejection.tsr

# Tokens openssl will not make: assembled around good-v2's TSTInfo by forge.py.
python3 ../forge.py good-v2.tsr tsa-noeku.pem tsa-noeku.key noeku.tsr
python3 ../forge.py good-v2.tsr tsa1.pem tsa1.key ess-mismatch.tsr --ess-cert tsa2.pem
python3 ../forge.py good-v2.tsr tsa1.pem tsa1.key sha1-signer.tsr --digest sha1
python3 ../forge.py good-v2.tsr tsa1.pem tsa1.key no-ess.tsr --no-ess
python3 ../forge.py good-v2.tsr tsa1.pem tsa1.key rebuilt.tsr
python3 ../forge.py good-v2.tsr tsa1.pem tsa1.key rsa-sha512.tsr --digest sha512
python3 ../forge.py good-v2.tsr tsa1.pem tsa1.key rsa-sha384.tsr --digest sha384
python3 ../forge.py good-v2.tsr tsa1.pem tsa1.key pss.tsr --pss
python3 ../forge.py good-v2.tsr tsa-p256.pem tsa-p256.key p256.tsr
python3 ../forge.py good-v2.tsr tsa-p384.pem tsa-p384.key p384.tsr --digest sha384

# Keep the public material (keys are throwaway and not kept), then clean up.
cp ca.pem other-ca.pem tsa1.pem tsa2.pem tsa-noeku.pem tsa-expired.pem tsa-p256.pem tsa-p384.pem message.txt message2.txt \
   request.tsq request2.tsq request-sha256.tsq request-badpolicy.tsq ./*.tsr ..
cd .. && rm -rf work
