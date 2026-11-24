#!/bin/sh
# Generates the "validly signed, but not NIST's" fixtures the nist_verify tests use: a throwaway CA, a
# throwaway RSA leaf certificate for test.beacon.example valid 2026-01-01 to 2026-12-31, and three
# pulses (copies of nist-pulse-2038648.json) signed by that leaf with RSA PKCS#1 v1.5 / SHA-512, one
# whose timeStamp is inside the leaf's validity and one each after and before it. The tests re-pin the
# issuer, the host name and the known certificate ids to these to prove a pulse that checks out in
# every other way is still refused when ONE of them is wrong. The private keys are deleted; only the
# certificates and the pulses are kept. Needs OpenSSL 3 and python3.
set -eu
cd "$(dirname "$0")"
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "$T/ca.key" 2>/dev/null
openssl req -new -x509 -key "$T/ca.key" -subj '/O=Guion Test/CN=Test Beacon CA' -days 36500 -sha256 -out test-ca.pem 2>/dev/null
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "$T/leaf.key" 2>/dev/null
openssl req -new -key "$T/leaf.key" -subj '/O=Guion Test/CN=test.beacon.example' -out "$T/leaf.csr" 2>/dev/null
printf 'subjectAltName=DNS:test.beacon.example\n' > "$T/san.ext"
openssl x509 -req -in "$T/leaf.csr" -CA test-ca.pem -CAkey "$T/ca.key" -CAcreateserial -sha256 \
  -not_before 20260101000000Z -not_after 20261231235959Z -extfile "$T/san.ext" -out test-leaf.pem 2>/dev/null
rm -f test-ca.srl
openssl x509 -in test-leaf.pem -outform DER -out "$T/leaf.der"

python3 -I - "$T" <<'PY'
import hashlib, json, struct, subprocess, sys
tmp = sys.argv[1]
doc = json.load(open("nist-pulse-2038648.json"))
leaf_der = open(f"{tmp}/leaf.der", "rb").read()
certificate_id = hashlib.sha512(leaf_der).hexdigest()

def hexbytes(text):
    return bytes.fromhex(text)

def prefixed(data):
    return struct.pack(">I", len(data)) + data

def signed_bytes(p):
    lv = {e["type"]: e["value"] for e in p["listValues"]}
    return b"".join([
        prefixed(p["uri"].encode()), prefixed(p["version"].encode()),
        struct.pack(">I", p["cipherSuite"]), struct.pack(">I", p["period"]),
        prefixed(hexbytes(p["certificateId"])),
        struct.pack(">Q", p["chainIndex"]), struct.pack(">Q", p["pulseIndex"]),
        prefixed(p["timeStamp"].encode()),
        prefixed(hexbytes(p["localRandomValue"])),
        prefixed(hexbytes(p["external"]["sourceId"])), struct.pack(">I", p["external"]["statusCode"]),
        prefixed(hexbytes(p["external"]["value"])),
        prefixed(hexbytes(lv["previous"])), prefixed(hexbytes(lv["hour"])), prefixed(hexbytes(lv["day"])),
        prefixed(hexbytes(lv["month"])), prefixed(hexbytes(lv["year"])),
        prefixed(hexbytes(p["precommitmentValue"])), struct.pack(">I", p["statusCode"]),
    ])

for name, stamp in [("in-window", "2026-11-22T08:23:00.000Z"), ("after", "2027-03-01T00:00:00.000Z"), ("before", "2025-12-31T23:59:00.000Z")]:
    p = json.loads(json.dumps(doc["pulse"]))
    p["certificateId"] = certificate_id
    p["timeStamp"] = stamp
    message = signed_bytes(p)
    open(f"{tmp}/message.bin", "wb").write(message)
    subprocess.check_call(["openssl", "dgst", "-sha512", "-sign", f"{tmp}/leaf.key", "-out", f"{tmp}/sig.bin", f"{tmp}/message.bin"])
    signature = open(f"{tmp}/sig.bin", "rb").read()
    p["signatureValue"] = signature.hex().upper()
    p["outputValue"] = hashlib.sha512(message + signature).hexdigest().upper()
    json.dump({"pulse": p}, open(f"test-pulse-{name}.json", "w"), indent=2)
    open(f"test-pulse-{name}.json", "a").write("\n")
print("certificateId", certificate_id)
PY
