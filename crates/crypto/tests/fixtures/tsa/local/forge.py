#!/usr/bin/env python3
"""Builds an RFC 3161 token (CMS SignedData) around the TSTInfo of an existing token, signed with
a chosen certificate and key. `openssl ts -reply` refuses to sign with a certificate that lacks the
critical timeStamping EKU, so tokens for the negative tests are assembled here.

usage: forge.py SOURCE.tsr SIGNER.pem SIGNER.key OUT.tsr [--ess-cert CERT.pem] [--digest sha256|sha1]
                [--no-ess] [--pss] [one of the single-defect options below]

The TSTInfo bytes are copied unchanged from SOURCE. By default the signed attributes are
contentType, messageDigest and signingCertificateV2 over SIGNER; --ess-cert names another
certificate for the signingCertificateV2 hash (a well-signed token whose ESS id does not match).

Each of these makes a token that is well signed but wrong in exactly one way (make-negatives.sh):
  --imprint-oid OID        the TSTInfo imprint names another hash algorithm (the 64 bytes are kept)
  --imprint-params         the imprint AlgorithmIdentifier carries INTEGER 0 instead of NULL
  --digest-params          the signer digest AlgorithmIdentifier carries INTEGER 0 instead of NULL
  --content-type OID       the signed contentType attribute names OID instead of id-ct-TSTInfo
  --sid-serial N           the SignerIdentifier names serial N instead of the signer's
  --outer-oid OID          the ContentInfo names OID instead of id-signedData
  --econtent-type OID      the EncapsulatedContentInfo names OID instead of id-ct-TSTInfo
  --two-signers            the SignerInfo appears twice
  --duplicate ATTR         the signed attribute content-type, message-digest or ess appears twice
  --sig-alg-hash HASH      the signature AlgorithmIdentifier names HASH (sha256WithRSA..., PSS
                           parameters, ecdsa-with-...) while the signer digest and the signature use
                           --digest
Requires the `cryptography` package; used only to regenerate fixtures.
"""
import sys, hashlib, argparse
from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import padding
from cryptography.hazmat.primitives.serialization import Encoding


def tlv(tag, body):
    n = len(body)
    if n < 0x80:
        head = bytes([tag, n])
    else:
        lb = n.to_bytes((n.bit_length() + 7) // 8, "big")
        head = bytes([tag, 0x80 | len(lb)]) + lb
    return head + body


def read(data, pos=0):
    tag = data[pos]
    n = data[pos + 1]
    pos += 2
    if n & 0x80:
        k = n & 0x7F
        n = int.from_bytes(data[pos:pos + k], "big")
        pos += k
    return tag, data[pos:pos + n], pos + n


def children(body):
    out, pos = [], 0
    while pos < len(body):
        tag, content, nxt = read(body, pos)
        out.append((tag, content))
        pos = nxt
    return out


def oid(dotted):
    parts = [int(x) for x in dotted.split(".")]
    body = bytes([parts[0] * 40 + parts[1]])
    for p in parts[2:]:
        chunk = [p & 0x7F]
        p >>= 7
        while p:
            chunk.append(0x80 | (p & 0x7F))
            p >>= 7
        body += bytes(reversed(chunk))
    return tlv(0x06, body)


def integer(value):
    n = max(1, (value.bit_length() + 8) // 8)
    return tlv(0x02, value.to_bytes(n, "big"))


def algid(dotted, null=True):
    return tlv(0x30, oid(dotted) + (b"\x05\x00" if null else b""))


HASHES = {"sha256": ("2.16.840.1.101.3.4.2.1", hashlib.sha256, hashes.SHA256),
          "sha384": ("2.16.840.1.101.3.4.2.2", hashlib.sha384, hashes.SHA384),
          "sha512": ("2.16.840.1.101.3.4.2.3", hashlib.sha512, hashes.SHA512),
          "sha1": ("1.3.14.3.2.26", hashlib.sha1, hashes.SHA1)}
ECDSA_OIDS = {"sha256": "1.2.840.10045.4.3.2", "sha384": "1.2.840.10045.4.3.3", "sha512": "1.2.840.10045.4.3.4"}

ap = argparse.ArgumentParser()
ap.add_argument("source"); ap.add_argument("signer"); ap.add_argument("key"); ap.add_argument("out")
ap.add_argument("--ess-cert"); ap.add_argument("--digest", default="sha256"); ap.add_argument("--no-ess", action="store_true")
ap.add_argument("--pss", action="store_true", help="sign an RSA key with RSASSA-PSS (hash = --digest, salt = digest length)")
ap.add_argument("--imprint-oid"); ap.add_argument("--imprint-params", action="store_true")
ap.add_argument("--digest-params", action="store_true"); ap.add_argument("--content-type")
ap.add_argument("--sid-serial", type=int); ap.add_argument("--outer-oid"); ap.add_argument("--econtent-type")
ap.add_argument("--two-signers", action="store_true")
ap.add_argument("--duplicate", choices=["content-type", "message-digest", "ess"])
ap.add_argument("--sig-alg-hash", choices=["sha256", "sha384", "sha512"])
args = ap.parse_args()
NOT_NULL = b"\x02\x01\x00"  # INTEGER 0: parameters that are present and not NULL

token = open(args.source, "rb").read()
# TimeStampResp ::= SEQUENCE { status, timeStampToken }; or a bare ContentInfo.
_, body, _ = read(token)
parts = children(body)
content_info = parts[1] if parts[0][0] == 0x30 and len(parts) > 1 and parts[1][0] == 0x30 and parts[0][1][:3] == b"\x02\x01\x00" else None
ci_body = content_info[1] if content_info else body
ci_parts = children(ci_body)
signed_data = children(children(ci_parts[1][1])[0][1])  # [0] EXPLICIT -> SignedData SEQUENCE
tstinfo_wrapper = signed_data[2][1]  # EncapsulatedContentInfo body
econtent_octets = children(children(tstinfo_wrapper)[1][1])[0][1]  # [0] -> OCTET STRING content

if args.imprint_oid or args.imprint_params:
    # TSTInfo ::= SEQUENCE { version, policy, messageImprint SEQUENCE { AlgorithmIdentifier, OCTET STRING }, ... }
    _, tst_body, _ = read(econtent_octets)
    fields = children(tst_body)
    alg, hashed = children(fields[2][1])
    alg_parts = children(alg[1])
    alg_oid = oid(args.imprint_oid) if args.imprint_oid else tlv(0x06, alg_parts[0][1])
    params = NOT_NULL if args.imprint_params else b"\x05\x00"
    fields[2] = (0x30, tlv(0x30, alg_oid + params) + tlv(hashed[0], hashed[1]))
    econtent_octets = tlv(0x30, b"".join(tlv(tag, body) for tag, body in fields))
econtent_type = oid(args.econtent_type) if args.econtent_type else oid("1.2.840.113549.1.9.16.1.4")
tstinfo_wrapper = econtent_type + tlv(0xA0, tlv(0x04, econtent_octets))

cert = x509.load_pem_x509_certificate(open(args.signer, "rb").read())
cert_der = cert.public_bytes(Encoding.DER)
key = serialization.load_pem_private_key(open(args.key, "rb").read(), None)
ess_der = x509.load_pem_x509_certificate(open(args.ess_cert, "rb").read()).public_bytes(Encoding.DER) if args.ess_cert else cert_der

dig_oid, dig_fn, dig_cls = HASHES[args.digest]
content_type = args.content_type or "1.2.840.113549.1.9.16.1.4"
attrs = [
    tlv(0x30, oid("1.2.840.113549.1.9.3") + tlv(0x31, oid(content_type))),
    tlv(0x30, oid("1.2.840.113549.1.9.4") + tlv(0x31, tlv(0x04, dig_fn(econtent_octets).digest()))),
]
if not args.no_ess:
    attrs.append(tlv(0x30, oid("1.2.840.113549.1.9.16.2.47") + tlv(0x31, tlv(0x30, tlv(0x30, tlv(0x30, tlv(0x04, hashlib.sha256(ess_der).digest())))))))
if args.duplicate:
    attrs.append(attrs[{"content-type": 0, "message-digest": 1, "ess": 2}[args.duplicate]])
attrs.sort()  # DER SET OF: ascending by encoding
signed_attrs_set = tlv(0x31, b"".join(attrs))
from cryptography.hazmat.primitives.asymmetric import ec, ed25519
RSA_OIDS = {"sha256": "1.2.840.113549.1.1.11", "sha384": "1.2.840.113549.1.1.12", "sha512": "1.2.840.113549.1.1.13"}
named = args.sig_alg_hash or args.digest
if isinstance(key, ed25519.Ed25519PrivateKey):
    signature = key.sign(signed_attrs_set)
    sig_alg = algid("1.3.101.112", null=False)
elif isinstance(key, ec.EllipticCurvePrivateKey):
    signature = key.sign(signed_attrs_set, ec.ECDSA(dig_cls()))
    sig_alg = algid(ECDSA_OIDS[named], null=False)
elif args.pss:
    salt = dig_cls().digest_size
    signature = key.sign(signed_attrs_set, padding.PSS(padding.MGF1(dig_cls()), salt), dig_cls())
    hash_alg = tlv(0x30, oid(HASHES[named][0]) + b"\x05\x00")
    params = tlv(0x30, tlv(0xA0, hash_alg) + tlv(0xA1, tlv(0x30, oid("1.2.840.113549.1.1.8") + hash_alg)) + tlv(0xA2, integer(salt)))
    sig_alg = tlv(0x30, oid("1.2.840.113549.1.1.10") + params)
else:
    signature = key.sign(signed_attrs_set, padding.PKCS1v15(), dig_cls())
    sig_alg = algid(RSA_OIDS[named]) if args.sig_alg_hash else algid("1.2.840.113549.1.1.1")

sid = tlv(0x30, cert.issuer.public_bytes() + integer(args.sid_serial if args.sid_serial is not None else cert.serial_number))
digest_alg = tlv(0x30, oid(dig_oid) + NOT_NULL) if args.digest_params else algid(dig_oid)
signer_info = tlv(0x30, integer(1) + sid + digest_alg + tlv(0xA0, b"".join(attrs)) + sig_alg + tlv(0x04, signature))
signer_infos = sorted([signer_info, signer_info]) if args.two_signers else [signer_info]
sd = tlv(0x30, integer(3) + tlv(0x31, algid(dig_oid)) + tlv(0x30, tstinfo_wrapper) + tlv(0xA0, cert_der) + tlv(0x31, b"".join(signer_infos)))
content_info_der = tlv(0x30, oid(args.outer_oid or "1.2.840.113549.1.7.2") + tlv(0xA0, sd))
granted = tlv(0x30, integer(0))
open(args.out, "wb").write(tlv(0x30, granted + content_info_der))
