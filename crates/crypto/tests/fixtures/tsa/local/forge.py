#!/usr/bin/env python3
"""Builds an RFC 3161 token (CMS SignedData) around the TSTInfo of an existing token, signed with
a chosen certificate and key. `openssl ts -reply` refuses to sign with a certificate that lacks the
critical timeStamping EKU, so tokens for the negative tests are assembled here.

usage: forge.py SOURCE.tsr SIGNER.pem SIGNER.key OUT.tsr [--ess-cert CERT.pem] [--digest sha256|sha1]
                [--no-ess] [--pss]

The TSTInfo bytes are copied unchanged from SOURCE. By default the signed attributes are
contentType, messageDigest and signingCertificateV2 over SIGNER; --ess-cert names another
certificate for the signingCertificateV2 hash (a well-signed token whose ESS id does not match).
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
args = ap.parse_args()

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

cert = x509.load_pem_x509_certificate(open(args.signer, "rb").read())
cert_der = cert.public_bytes(Encoding.DER)
key = serialization.load_pem_private_key(open(args.key, "rb").read(), None)
ess_der = x509.load_pem_x509_certificate(open(args.ess_cert, "rb").read()).public_bytes(Encoding.DER) if args.ess_cert else cert_der

dig_oid, dig_fn, dig_cls = HASHES[args.digest]
attrs = [
    tlv(0x30, oid("1.2.840.113549.1.9.3") + tlv(0x31, oid("1.2.840.113549.1.9.16.1.4"))),
    tlv(0x30, oid("1.2.840.113549.1.9.4") + tlv(0x31, tlv(0x04, dig_fn(econtent_octets).digest()))),
]
if not args.no_ess:
    attrs.append(tlv(0x30, oid("1.2.840.113549.1.9.16.2.47") + tlv(0x31, tlv(0x30, tlv(0x30, tlv(0x30, tlv(0x04, hashlib.sha256(ess_der).digest())))))))
attrs.sort()  # DER SET OF: ascending by encoding
signed_attrs_set = tlv(0x31, b"".join(attrs))
from cryptography.hazmat.primitives.asymmetric import ec
if isinstance(key, ec.EllipticCurvePrivateKey):
    signature = key.sign(signed_attrs_set, ec.ECDSA(dig_cls()))
    sig_alg = algid(ECDSA_OIDS[args.digest], null=False)
elif args.pss:
    salt = dig_cls().digest_size
    signature = key.sign(signed_attrs_set, padding.PSS(padding.MGF1(dig_cls()), salt), dig_cls())
    hash_alg = tlv(0x30, oid(dig_oid) + b"\x05\x00")
    params = tlv(0x30, tlv(0xA0, hash_alg) + tlv(0xA1, tlv(0x30, oid("1.2.840.113549.1.1.8") + hash_alg)) + tlv(0xA2, integer(salt)))
    sig_alg = tlv(0x30, oid("1.2.840.113549.1.1.10") + params)
else:
    signature = key.sign(signed_attrs_set, padding.PKCS1v15(), dig_cls())
    sig_alg = algid("1.2.840.113549.1.1.1")

sid = tlv(0x30, cert.issuer.public_bytes() + integer(cert.serial_number))
signer_info = tlv(0x30, integer(1) + sid + algid(dig_oid) + tlv(0xA0, b"".join(attrs)) + sig_alg + tlv(0x04, signature))
sd = tlv(0x30, integer(3) + tlv(0x31, algid(dig_oid)) + tlv(0x30, tstinfo_wrapper) + tlv(0xA0, cert_der) + tlv(0x31, signer_info))
content_info_der = tlv(0x30, oid("1.2.840.113549.1.7.2") + tlv(0xA0, sd))
granted = tlv(0x30, integer(0))
open(args.out, "wb").write(tlv(0x30, granted + content_info_der))
