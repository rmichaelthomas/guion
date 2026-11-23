# RFC 3161 fixtures

Everything here was made on 2026-11-23 with OpenSSL 3.6.3. `tsa_vectors.rs` reads these files, and
`stdlib_crypto.rs` (the Guion wrapper test) and the browser vector test in `crates/vm-wasm` read some of them.

## Live tokens (`digicert/`, `sectigo/`, `freetsa/`)

Each directory has `message.txt` (a one-line text naming the provider), `request.tsq`, `response.tsr` (the TSA's
answer, exactly as received), `tsa.pem` (the signing certificate), `issuer.pem` (the certificate that issued it)
and `root.pem` (the trust anchor, used only to cross-check with OpenSSL).

Commands, for `$P` in `digicert`, `sectigo`, `freetsa` (the message is `printf 'Guion RFC 3161 fixture message for %s\n' $P`):

```
openssl ts -query -data $P/message.txt -sha512 -cert -no_nonce -out $P/request.tsq
curl -H 'Content-Type: application/timestamp-query' --data-binary @$P/request.tsq \
     http://timestamp.digicert.com    -o digicert/response.tsr
curl -H 'Content-Type: application/timestamp-query' --data-binary @$P/request.tsq \
     http://timestamp.sectigo.com     -o sectigo/response.tsr
curl -H 'Content-Type: application/timestamp-query' --data-binary @$P/request.tsq \
     https://freetsa.org/tsr          -o freetsa/response.tsr
openssl ts -reply -in $P/response.tsr -token_out -out token.der      # the token alone
openssl pkcs7 -inform DER -in token.der -print_certs                 # the certificates it carries, in order
```

`tsa.pem` and `issuer.pem` are the first two certificates the token carries (FreeTSA's second one is its self-signed
root, which is its issuer). The roots came from the AIA "CA Issuers" URLs: DigiCert Trusted Root G4
(`http://cacerts.digicert.com/DigiCertTrustedRootG4.crt`), Sectigo Public Time Stamping Root R46
(the self-signed entry of `http://crt.sectigo.com/SectigoPublicTimeStampingRootR46.p7c`), and FreeTSA's root
(`http://www.freetsa.org/files/cacert.pem`, identical to the embedded one).

### Cross-checks with OpenSSL

```
openssl ts -verify -data digicert/message.txt -in digicert/response.tsr \
    -CAfile <(cat digicert/issuer.pem digicert/root.pem) -untrusted digicert/tsa.pem      # Verification: OK
openssl ts -verify -data freetsa/message.txt -in freetsa/response.tsr \
    -CAfile freetsa/root.pem -untrusted freetsa/tsa.pem                                   # Verification: OK
openssl cms -verify -inform DER -in sectigo/token.der -CAfile sectigo/root.pem \
    -purpose timestampsign -out /dev/null                                                 # CMS Verification successful
```

`openssl ts -verify` **fails** on the Sectigo token with `ess cert id not found`, although the signature, the
chain and the message imprint are fine: Sectigo's `ESSCertID` has the right SHA-1 of the signing certificate, but its
`issuerSerial` names the Root R46 and the root's serial number instead of the Sectigo CA R41 that issued the
signing certificate, and OpenSSL compares `issuerSerial`. `crypto_tsa_verify` compares the certificate hash only (the
hash names the certificate; `issuerSerial` adds nothing to it), so it accepts this token. The test
`live_tokens_verify` covers it. The imprint was checked separately: `openssl ts -reply -in sectigo/response.tsr -text`
prints a `Message data` equal to `sha512sum sectigo/message.txt`.

What the three tokens look like (`openssl ts -reply -in response.tsr -text`):

| | policy | genTime (UTC) | serial | signature | signer certificate (valid) |
|---|---|---|---|---|---|
| DigiCert | `2.16.840.1.114412.7.1` | 2026-11-23 19:40:59 (1795462859000 ms) | `33A1B07C58AB304D70F5DC92FE4A5AE5` | RSA, SHA-512 digest, ESSCertIDv2 | 2026-08-05 to 2037-11-04, RSA 4096 |
| Sectigo | `1.3.6.1.4.1.6449.2.1.1` | 2026-11-23 19:41:01 (1795462861000 ms) | `F29C67357AFF76A335019E2574ED5C19E8925E06` | RSA, ESSCertID (SHA-1) | 2026-03-25 to 2037-06-24, RSA |
| FreeTSA | `1.2.3.4.1` (OpenSSL prints `tsa_policy1`) | 2026-11-23 19:41:03 (1795462863000 ms) | `0A76A713` | ECDSA P-384 with SHA-512, certificate issued under an RSA root | 2026-02-15 to 2040-02-02 |

None carries `accuracy` or a nonce (the requests have none). Sectigo and FreeTSA put the TSA's name in the token.

### Terms and limits seen

- **DigiCert** (`http://timestamp.digicert.com`): no key and no account; the answer carries no rate-limit header. DigiCert
  publishes the server for its code-signing customers' use and says nothing about a request rate on its public
  page (the knowledge-base article is rendered by script and has no text to quote); treat it as a service for occasional
  use, not an archive-grade or load-bearing one.
- **Sectigo** (`http://timestamp.sectigo.com`): free, described on its resource page as the time-stamping server for
  SignTool and similar tools; no key, no rate-limit header, no published request limit. Its Terms of Use apply.
- **FreeTSA** (`https://freetsa.org/tsr`): free; the site asks users not to abuse it ("do not abuse") and publishes no
  numeric limit; it gives no availability guarantee. The signing certificate is renewed (the 2016 one stopped signing in
  March 2026), so pin the certificate you verified, not the host.

For a production service, run one request per draw (not a polling loop), keep the response, and expect any of the three to be
slow or down; the example in `language/examples/timestamp` makes one request per run.

## Local test TSA (`local/`)

`local/make.sh` rebuilds everything in `local/` from scratch with OpenSSL and `local/forge.py` (Python 3 with the
`cryptography` package, used only to regenerate). It makes a throwaway CA, TSA certificates and tokens. The keys are
not kept. **Tokens carry the time of the run; the expected values in `tsa_vectors.rs` (genTime `2026-11-23 19:48:31`,
1795463311 s) were taken from this run, so after regenerating, update them from
`openssl ts -reply -in X.tsr -text`.**

Certificates (all issued by `ca.pem`, RSA 2048, except the second CA): `tsa1.pem` and `tsa2.pem` (critical `timeStamping`
EKU, valid 2026 to 2046), `tsa-noeku.pem` (no EKU), `tsa-expired.pem` (valid only 2020), `tsa-p256.pem` and
`tsa-p384.pem` (EC keys). `other-ca.pem` is an unrelated CA.

Made by `openssl ts -reply -config X.cnf -section tsa_config -queryfile Q -out T` (config written by `make.sh`; the
requests by `openssl ts -query -data message.txt -sha512 -cert -no_nonce`):

| token | what it is |
|---|---|
| `good-v2.tsr` | TSA 1, `ess_cert_id_alg = sha256` (ESSCertIDv2), serial 2 |
| `good-v1.tsr` | TSA 1, `ess_cert_id_alg = sha1` (ESSCertID), serial 3 |
| `other-digest.tsr` | TSA 1 over `message2.txt` (another digest) |
| `sha256-imprint.tsr` | TSA 1 over the SHA-256 of `message.txt` (`-sha256` request: another imprint algorithm) |
| `tsa2.tsr` | the second TSA certificate, same CA |
| `expired.tsr` | signed by `tsa-expired.pem`: genTime 2026 is outside its 2020 validity |
| `accuracy.tsr` | TSA 1 with `accuracy = secs:1, millisecs:500, microsecs:100` and `clock_precision_digits = 3` (genTime `19:48:31.414`) |
| `precision6.tsr` | TSA 1 with `clock_precision_digits = 6` (genTime `19:48:31.427545`) |
| `rejection.tsr` | status 2 (rejection), `failInfo` unacceptedPolicy: made by `openssl ts -reply` for a request with `-tspolicy 1.2.3.99`, which the TSA's `other_policies` does not list, so OpenSSL itself produced it (no hand-built DER) |

`openssl ts -reply` refuses to sign with a certificate that lacks the critical `timeStamping` EKU, and cannot make a token
that is well signed but whose ESS id names another certificate, so `forge.py` builds these around `good-v2`'s TSTInfo and signs with
`cryptography`: `noeku.tsr` (signed by `tsa-noeku.pem`), `ess-mismatch.tsr` (TSA 1's signature, the ESSCertIDv2 hash of TSA 2's
certificate), `no-ess.tsr` (no signing-certificate attribute), `sha1-signer.tsr` (SHA-1 as the signer digest: `E_TSA_ALG`),
`rebuilt.tsr` (the unmodified rebuild, which verifies, so the others differ from it in one thing),
`rsa-sha512.tsr`, `rsa-sha384.tsr`, `pss.tsr` (RSASSA-PSS), `p256.tsr` and `p384.tsr` (ECDSA, signed by `tsa-p256.pem` and
`tsa-p384.pem`). `openssl ts -verify` accepts `rebuilt`, `rsa-sha512`, `rsa-sha384`, `p256` and `p384`; it does not support PSS
here, and `openssl cms -verify -purpose timestampsign` accepts `pss.tsr`.

The status values 3 to 5 (waiting, revocation warning, revocation notification), a granted response with no token, partial
`accuracy` (seconds only, millis only, micros only), fractions of 1 to 9 digits, a leap second and a non-UTC time are
covered by unit tests in `src/tsa.rs`, which build the DER directly.
