#!/usr/bin/env python3
"""Mutation check for src/tsa.rs: disables each check of tsa_verify (and the format rules around it)
in turn, runs the RFC 3161 tests, and reports whether a test failed (KILLED) or not (SURVIVED).
Every mutation must be KILLED; the script exits 1 if any survives.

usage: python3 crates/crypto/tests/mutate_tsa.py [NAME ...]     (from the repository root)

It edits crates/crypto/src/tsa.rs in place and always restores it, so do not run it while editing
that file. Set CARGO_TARGET_DIR to keep its builds apart from the main target directory.
"""
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
SOURCE = os.path.join(ROOT, "crates/crypto/src/tsa.rs")

MUTATIONS = {
    # 1: the token's structure
    "signed_data_oid": ("if oid_string(info.expect(OID)?.content)? != SIGNED_DATA_OID {",
                        "if oid_string(info.expect(OID)?.content)?.is_empty() {"),
    "tstinfo_oid": ("if oid_string(encap.expect(OID)?.content)? != TST_INFO_OID {",
                    "if oid_string(encap.expect(OID)?.content)?.is_empty() {"),
    "one_signer": ("if !signers.is_empty() {\n        return Err(format_error(\"the token has more than one SignerInfo\"));",
                   "if false {\n        return Err(format_error(\"the token has more than one SignerInfo\"));"),
    "imprint_params": ("if !plain {\n        return Err(format_error(\n            \"the imprint",
                       "if false {\n        return Err(format_error(\n            \"the imprint"),
    "digest_params": ("if !parsed.signer.digest_algorithm.1 {", "if false {"),
    "dup_content_type": ("if attributes.content_type.is_some() {", "if false {"),
    "dup_message_digest": ("if attributes.message_digest.is_some() {", "if false {"),
    "dup_ess_v2": ("if attributes.signing_certificate_v2.replace(value).is_some() {",
                   "if attributes.signing_certificate_v2.replace(value).is_some() && false {"),
    "digest_length": ("if digest.len() != 64 {\n        return Err(CryptoError::new(\n            \"E_TSA_DIGEST\",\n            format!(\n                \"the digest is",
                      "if false {\n        return Err(CryptoError::new(\n            \"E_TSA_DIGEST\",\n            format!(\n                \"the digest is"),
    # 2: the imprint
    "c2_imprint_alg": ("tst.imprint_algorithm != SHA512_OID || tst.imprint != digest", "tst.imprint != digest"),
    "c2_imprint": ("tst.imprint_algorithm != SHA512_OID || tst.imprint != digest", "tst.imprint_algorithm != SHA512_OID"),
    # 3: contentType and messageDigest
    "c3_content_type": ("if attributes.content_type.as_deref() != Some(TST_INFO_OID) {", "if false {"),
    "c3_message_digest": ("Some(wanted) if wanted == digest_hash.digest(parsed.e_content).as_slice() => {}", "_ => {}"),
    # 4: ESS
    "c4_absent": ("if attributes.signing_certificate_v2.is_none() && attributes.signing_certificate.is_none() {", "if false {"),
    "c4_match": ("Ok(wanted == actual.as_slice())", "Ok(true)"),
    "ess_v1_hash": ("\"sha1\" => Sha1::digest(certificate_der).to_vec(),", "\"sha1\" => Sha256::digest(certificate_der).to_vec(),"),
    # 5: the SignerIdentifier
    "c5_sid": ("if !signer_identifier_matches(parsed.signer.sid, &tsa)? {", "if false {"),
    # 6: the signature and its algorithm
    "c6_sig": ("if !signature_verifies(&parsed.signer, digest_hash, tsa_cert_der, &tsa)? {", "if false {"),
    "rsa_sig_alg_disagree": ("CertSignature::RsaPkcs1v15(hash) if hash == digest_hash => named,",
                             "CertSignature::RsaPkcs1v15(_) => CertSignature::RsaPkcs1v15(digest_hash),"),
    "pss_sig_alg_disagree": ("CertSignature::RsaPss(hash, _) if hash == digest_hash => named,",
                             "CertSignature::RsaPss(_, salt) => CertSignature::RsaPss(digest_hash, salt),"),
    "ecdsa_sig_alg_disagree": ("CertSignature::Ecdsa(hash) if hash == digest_hash => named,",
                               "CertSignature::Ecdsa(_) => CertSignature::Ecdsa(digest_hash),"),
    "tsa_key_alg_code": ("if error.code == \"E_X509_ALG\" {", "if false {"),
    # 7, 8, 9
    "c7_issuer": ("if !x509_verify_issued_by(tsa_cert_der, issuer_cert_der)? {", "if false {"),
    "c8_eku": ("if !has_time_stamping {", "if false {"),
    "c9_validity_lo": ("generated < not_before || generated > not_after", "generated > not_after"),
    "c9_validity_hi": ("generated < not_before || generated > not_after", "generated < not_before"),
    # TSTInfo and the response
    "accuracy_ceil": ("total += micros.div_ceil(1000);", "total += micros / 1000;"),
    "status_check": ("if status >= 2 {", "if status >= 6 {"),
    "leap": ("second > 59", "second > 60"),
}


def main():
    names = sys.argv[1:] or list(MUTATIONS)
    original = open(SOURCE).read()
    survived = []
    try:
        for name in names:
            before, after = MUTATIONS[name]
            count = original.count(before)
            if count != 1:
                print(f"{name} STALE (the text occurs {count} times; update the script)", flush=True)
                survived.append(name)
                continue
            open(SOURCE, "w").write(original.replace(before, after))
            run = subprocess.run(
                ["cargo", "test", "--locked", "--offline", "-p", "guion-crypto", "--test", "tsa_vectors", "--lib"],
                cwd=ROOT, capture_output=True, text=True)
            output = run.stdout + run.stderr
            failed = [line.split()[1] for line in output.splitlines() if line.startswith("test ") and line.endswith("FAILED")]
            if run.returncode != 0 and not failed:
                print(f"{name} BUILD-FAILED", flush=True)
                survived.append(name)
            elif failed:
                print(f"{name} KILLED by {', '.join(failed[:4])}", flush=True)
            else:
                print(f"{name} SURVIVED", flush=True)
                survived.append(name)
    finally:
        open(SOURCE, "w").write(original)
    if survived:
        print("not killed: " + ", ".join(survived))
        sys.exit(1)
    print(f"all {len(names)} mutations killed")


if __name__ == "__main__":
    main()
