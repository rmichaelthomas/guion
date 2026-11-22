# beacons

Checking public randomness beacons in Guion. This covers multiformats, drand, and Twine with CURBy
(including the quantum chain CURBy-Q), and the NIST Randomness Beacon 2.0. Every function is plain Guion over `std:crypto`, `std:encoding` and
`http_get`.

Capabilities (declared in `guion.toml`): `network` to `api.drand.sh`, `drand.cloudflare.com`,
`random.colorado.edu` and `beacon.nist.gov`, and `clock`. A project that depends on the package
grants the hosts and the clock in its own `guion.toml`.

## multiformats (`src/multiformats.gu`)

- `base32_lower_encode(data: Bytes) -> Text`, `base32_lower_decode(text: Text) -> Result[Bytes, StdError]`:
  lower-case RFC 4648 base32, no padding. Decoding is strict (one spelling per byte string); `E_BASE32`.
- `varint_encode(value: Int) -> Bytes`: unsigned LEB128.
- `cid_v1(codec: Int, multihash_code: Int, digest: Bytes) -> Text`: CIDv1 text (multibase `b`).
- `cid_parse(text: Text) -> Result[CidParts, StdError]`, `record CidParts: version, codec, hash_code, digest`; `E_CID`.

## drand (`src/drand.gu`)

- `record DrandChain: hash, public_key, period, genesis_time, scheme`; `record DrandRound: round, randomness, signature`.
- `drand_quicknet() -> DrandChain`: the quicknet constants.
- `drand_fetch(chain, round) -> Result[DrandRound, StdError]`: `api.drand.sh`, then `drand.cloudflare.com`,
  both at `/<chain hash>/public/<round>`. The round is not checked; `E_DRAND_FETCH`, `E_DRAND_ROUND`.
- `drand_verify(chain, value) -> Result[Bool, StdError]`: BLS check of SHA-256(round, 8 bytes big-endian)
  and `randomness == SHA-256(signature)`. A bad value is `Ok(false)`; malformed bytes are `Err`; a round
  outside 1 to 2^63 - 1 is `E_DRAND_ROUND`.
- `drand_parse_round(body: Text, round) -> Result[DrandRound, StdError]`: reads one `public/<round>` answer; text
  that is not JSON or lacks `round`, `randomness` or `signature` (or has them in the wrong type) is `E_DRAND_FETCH`,
  another round is `E_DRAND_ROUND`.
- `drand_round_at(chain, unix_seconds) -> Int`, `drand_round_time(chain, round) -> Int`.
- `tlock_seal(chain, round, plaintext) -> Result[Text, StdError]`: armored age, interoperable with `tle`.
- `tlock_open(chain, armored) -> Result[Bytes, StdError]`: refuses a file sealed for another chain
  (`E_TLOCK_CHAIN`), refuses a round still in the future (`E_TLOCK_TOO_EARLY`, the message gives the round
  and its unix time), then fetches the round, verifies it (`E_DRAND_VERIFY`) and decrypts.
- `tlock_open_with(chain, armored, value: DrandRound)`: the same without the clock or network.
- `tlock_armor_round(armored)`, `tlock_armor_chain(armored)`: the round and chain hash from the header.

## Twine (`src/twine.gu`)

Protocol facts, from the Twine reference client (`@twine-protocol/twine-core` 0.1.0 and
`twine-http-store`), the `twine_spec_rng` crate and the CURBy client (`curby-js-client`), and checked
against live data:

- **Endpoints** (base `https://random.colorado.edu/api`): `GET /chains` (all chains), `GET
  /chains/<chain cid>`, `GET /chains/<chain cid>/pulses/latest`, `/pulses/<index>`, `/pulses/<pulse cid>`
  and `/pulses/<from>-<to>` (a range, higher index first, as a JSON array). Records are
  `{"cid": {"/": cid}, "data": {"content": ..., "signature": jws}}`. The host often answers a single
  pulse zstd-compressed (`content-encoding: zstd`, even though the client asks for no encoding);
  `http_get` decodes that, so `twine_fetch_pulse` fetches an index, a pulse CID or `latest` directly
  and `twine_verify_links` follows each `links[0]` CID.
- **A pulse** is `content: {chain, index, links, mixins, payload, source}`. `links` are CIDs of earlier
  pulses of the same chain; `mixins` are `{chain, value}` pairs naming pulses of other chains.
- **`links_radix` links** (32 for CURBy): a pulse at index n links first to n-1, then to
  floor((n-1)/32^k)*32^k for k = 1, 2, ... while 32^k <= n-1 (so 84647 has 4 links: 84646, 84640,
  83968 and 65536). Checked on every linked pulse in the fixtures; `twine_verify_links_in` enforces it
  for links that are in the pool and walks `links[0]`.
- **The CID** is CIDv1, codec 0x71, multihash 0x14 (SHA3-512), over the DAG-CBOR of the whole `data`
  object, so editing the content or the signature changes it.
- **What the JWS signs**: the compact JWS payload is the multihash `0x14 0x40 || SHA3-512(DAG-CBOR(data.content))`
  (reference client: `jose.compactVerify`, then compare the payload to `sha3512.digest(dagcbor(content))`).
  The signed bytes are the ASCII `base64url(header) "." base64url(payload)`, hashed with SHA-256 inside the RSA
  and ECDSA primitives. The chain's JWK is `content.key` (RS256 for CURBy; the Weather and Seam chains use ES256).
  CURBy-Q, CURBy-RNG, drand Mirror and CURBy-Timestamp share one RSA key.

Functions: `record TwineRecord: cid, content, signature`; `twine_record_from_json`,
`twine_records_from_json`, `twine_find`, `twine_cid`; `twine_verify(chain, record) -> Result[Bool, StdError]`
(CID, payload and signature; the chain against its own key first; `Ok(false)` on any mismatch,
`E_TWINE_ALG`, `E_TWINE_JWS`, `E_TWINE_KEY` on a record that cannot be read as a signed record);
`twine_verify_links_in(chain, record, depth, pool)` (offline) and `twine_verify_links(chain, record, depth)`
(fetches); `twine_fetch_chain`, `twine_fetch_latest`, `twine_fetch_pulse` (fetch only, `E_TWINE_FETCH`);
`base64url_nopad_decode`, `base64_nopad_decode`, `has_key`. Pin the chain's CID yourself: `twine_verify`
proves the chain record is self-consistent and the pulse belongs to it, not that it is the chain you meant.
RS256, PS256 and ES256 are implemented; the fixtures exercise RS256 and ES256 (no PS256 chain was found live).

## CURBy (`src/curby.gu`)

- `curby_q_chain()`, `curby_rng_chain()`: the chain CIDs (`bafyriqhijw5...` and `bafyriqci6f3...`).
- `curby_randomness(pulse) -> Result[Bytes, StdError]`: CURBy-Q (`twine/1.0.x/diqrng/1.0.x`). A round is
  three pulses with `payload.stage` "request", "precommit" and "randomness"; the randomness pulse's
  `payload.randomness` is a DAG-JSON byte string of **64 bytes** (`nBitsOut` 512), which is what the CURBy
  client returns. Other stages are `E_CURBY_STAGE`, an RNG pulse is `E_CURBY_NEEDS_PREVIOUS`. The Bell test
  and Trevisan extraction behind the bytes are CURBy's own verifier's job; this package checks the Twine
  records and reads the bytes where CURBy's client does. CURBy-Q has not produced a pulse since 2025-08-22
  (its latest is a "request" pulse), so the newest randomness is one pulse back.
- `curby_rng_randomness(chain, pulse, previous) -> Result[Bytes, StdError]`: CURBy-RNG (`twine/1.0.x/rng/1.0.x`,
  a modified NIST beacon). The randomness is the **64-byte digest in the pulse's own CID** (`extract_randomness`
  in `twine_spec_rng`, `extractRandomness` in the CURBy client), valid only when both pulses verify, `previous`
  is `links[0]`, the signature is RS256 (deterministic), and `SHA3-512(salt XOR digest(previous CID))` equals
  the `pre` multihash in `previous`'s payload. The crate also checks that timestamps are one period apart;
  this package does not. `E_CURBY_RNG` otherwise.

## NIST Beacon 2.0 (`src/nist.gu`)

Sources: NISTIR 8213 (draft), "A Reference for Randomness Beacons: Format and Protocol Version 2"
(https://nvlpubs.nist.gov/nistpubs/ir/2019/NIST.IR.8213-draft.pdf), sections 3.1 to 4.9 and Algorithm 2, and the
live service at https://beacon.nist.gov/beacon/2.0. Where the draft and the service differ, the service wins; this
was found by testing serializations against live pulses, and a third-party verifier guide
(https://beacon-6sg.pages.dev/guides/nist-verification) agreed with the service.

Facts, each confirmed on live pulses (pulse 2038648 of chain 2, 2026-11-22, and others):

- **Cipher suite 0**: SHA-512 for every hash and RSA PKCS#1 v1.5 with SHA-512 for the signature.
- **What is signed** (fields 1 to 19, in this order): `uri`, `version`, `cipherSuite`, `period`, `certificateId`,
  `chainIndex`, `pulseIndex`, `timeStamp`, `localRandomValue`, `external.sourceId`, `external.statusCode`,
  `external.value`, `previous`, `hour`, `day`, `month`, `year` (the five `listValues` by type), `precommitmentValue`,
  `statusCode`. Serialization: `uri`, `version`, `timeStamp` and every hex field (hex decoded) are a **4-byte**
  big-endian length then the bytes; `cipherSuite`, `period`, `external.statusCode` and `statusCode` are 4 bytes
  big-endian; `chainIndex` and `pulseIndex` are 8 bytes big-endian. `version` is the text `2.0` as served. The
  message is this serialization and the scheme hashes it. NISTIR 8213 (draft) says 8-byte length prefixes and an
  8-byte `external.statusCode`; the live service does not (tried all combinations: only 4 and 4 verify).
- **`outputValue`** = SHA-512(serialization of fields 1 to 19 || raw `signatureValue` bytes). The signature is
  appended raw, with no length prefix (the draft's Algorithm 3 prefixes it; the service does not).
- **`certificateId`** = SHA-512 of the signing certificate's **DER** (not of the PEM text), served as PEM at
  `https://beacon.nist.gov/beacon/2.0/certificate/<certificateId in hex>`. Pulses by index:
  `/beacon/2.0/chain/<chain>/pulse/<index>`; newest: `/beacon/2.0/pulse/last`. Hex in the JSON is upper case.
- **Precommitment**: `precommitmentValue` of pulse n = SHA-512(`localRandomValue` of pulse n+1), and pulse n+1's
  `previous` listValue = pulse n's `outputValue` (confirmed on 2038647 and 2038648 and on the pair across a UTC day
  boundary).

Functions:

- `record NistPulse`: the JSON pulse with hex fields decoded to `Bytes` (`uri, version, cipher_suite, period,
  certificate_id, chain_index, pulse_index, time_stamp, local_random_value, external_source_id,
  external_status_code, external_value, previous, hour, day, month, year, precommitment_value, status_code,
  signature, output_value`).
- `nist_pulse_from_json(answer: Json) -> Result[NistPulse, StdError]` (`E_NIST_FORMAT` for a missing or mistyped member,
  `listValues` that is not an array of `{type, value}`, or a number the signed serialization cannot hold: 4-byte fields
  0 to 2^32 - 1, `chainIndex` and `pulseIndex` 0 to 2^63 - 1).
- `nist_verify(pulse, certificate_pem: Text) -> Result[Bool, StdError]`: `Ok(true)` only for NIST's own pulse. All of
  these must hold: `certificateId` is SHA-512 of the certificate's DER **and is in one of four pinned id lists**
  (`nist_known_certificate_ids()`, `nist_legacy_certificate_ids()`, `nist_chain1_global_ca_g2_certificate_ids()`,
  `nist_chain1_secure_server_certificate_ids()`); the certificate was issued by **the one issuer paired with that list**
  (`nist_pinned_issuer_pem()`, `nist_legacy_issuer_pem()`, `nist_chain1_global_ca_g2_issuer_pem()`,
  `nist_chain1_secure_server_issuer_pem()`) and by no other; its SubjectAltName includes `nist_beacon_host()`
  (`engine.beacon.nist.gov`, read from the live certificate); the pulse's `timeStamp` lies within the certificate's
  `notBefore` and its `notAfter`, except that chain 1's first certificate is accepted until `nist_validity_end(id, notAfter)`
  (`nist_validity_extension(id)`, 2019-08-16T14:11:00Z, 0 for every other id); the RSA PKCS#1 v1.5 / SHA-512 signature
  verifies; and `outputValue` is the hash of the serialization and signature. The issuer alone is not enough: it is a public TLS CA
  that signs certificates for any customer. This takes the certificate PEM as a second argument (the brief's
  one-argument form cannot work offline). `Ok(false)` on any mismatch;
  `E_NIST_SUITE` for a cipher suite other than 0, `E_NIST_FORMAT` for a number the serialization cannot hold or a
  `timeStamp` that is not an ISO 8601 UTC time.
- `nist_verify_pinned(pulse, certificate_pem, issuer_pem, host, known_ids: List[Text])`: the same checks with the issuer,
  the host name and the known certificate ids named by the caller (the tests use it to give each check a certificate
  that satisfies all the others).
- `nist_verify_certificate(certificate_pem, issuer_pem) -> Result[Bool, StdError]`: one issuer link (`crypto_x509_verify_issued_by`): true when the issuer
  certificate's key signed the certificate. `nist_pinned_issuer_pem() -> Text` is the pinned issuer.
- `nist_verify_precommitment(previous, current) -> Result[Bool, StdError]`: `current` is the next pulse of the same
  chain, links to `previous`, and `previous.precommitment_value == SHA-512(current.local_random_value)`. It does
  not check signatures.
- `nist_fetch_last()`, `nist_fetch(chain, pulse)`, `nist_fetch_certificate(pulse)`: fetch only (`E_NIST_FETCH`).
- `nist_signed_bytes(pulse)`, `nist_output_value(pulse)`: the serialization and the recomputed output value.

**Four issuer pins (current, legacy, chain 1 Global CA G2, chain 1 Secure Server), and how to refresh them.** The issuer pin alone would accept a certificate DigiCert issued to anyone, so
`nist_verify` also requires a known certificate id, the host name and the validity window. Each known id is paired with the one issuer that must have signed it, so no pinned issuer stands for another issuer's ids: `nist_known_certificate_ids()` go with `nist_pinned_issuer_pem()`, and `nist_legacy_certificate_ids()` go with `nist_legacy_issuer_pem()` (below). A pulse is checked against the issuer paired with the list its id is in (current, legacy, chain 1 Global CA G2 or chain 1 Secure Server, below), and against that issuer only.

*The known certificate ids* are in `fixtures/nist-certificate-ids.txt` (one SHA-512 of a certificate's DER per line,
with the validity dates in comments) and copied into `nist_known_certificate_ids()` in `src/nist.gu`; a test keeps the
two equal. NIST rotates its beacon certificate about every year (the current one is valid 2026-10-21 to 2027-03-20),
and after a rotation `nist_verify` is `Ok(false)` for new pulses until the list is refreshed. To refresh:
1. fetch the newest pulse (`nist_fetch_last()`) and take its `certificateId`; sample earlier pulses
   (`/beacon/2.0/chain/2/pulse/<index>`) to find the ids in between;
2. for each id fetch `https://beacon.nist.gov/beacon/2.0/certificate/<id>`, check that `openssl x509 -outform DER` of it
   hashes (SHA-512) to that id, that its SubjectAltName is `engine.beacon.nist.gov`, and that
   `openssl verify -no_check_time -partial_chain -CAfile fixtures/nist-issuer.pem cert.pem` says OK (a certificate the
   pinned issuer did not sign does not belong on the list);
3. add the ids to `fixtures/nist-certificate-ids.txt` and `nist_known_certificate_ids()`; save the newest certificate as
   `fixtures/nist-certificate.pem` if the test fixtures move to newer pulses (and update the validity numbers in
   `test_nist_certificate_names_and_validity_are_read_from_the_certificate`);
4. record `tapes/nist-live.tape` again (below).

*The host name* `engine.beacon.nist.gov` is `nist_beacon_host()`; change it only if NIST serves the beacon from another
name (read it from the certificate's SubjectAltName).

**Pinned issuer.** NIST's beacon certificate (`engine.beacon.nist.gov`, a 4096-bit RSA key, SHA-256 with RSA) is issued by
`C=US, O=DigiCert Inc, CN=DigiCert Global G2 TLS RSA SHA256 2020 CA1`, an **intermediate** CA valid until 2031-03-29
(SHA-256 fingerprint `C8:02:5F:9F:C6:5F:DF:C9:5B:3C:A8:CC:78:67:B9:A5:87:B5:27:79:73:95:79:17:46:3F:C8:13:D0:B6:25:A9`).
That intermediate is what `nist_verify` pins (`fixtures/nist-issuer.pem`, copied into `nist_pinned_issuer_pem()` in
`src/nist.gu` so the library needs no file). It is itself issued by the self-signed root `DigiCert Global Root G2`
(saved as `fixtures/nist-issuer-root.pem` for the test that checks that link; the root is not pinned, because pinning the
intermediate is the narrower trust). The leaf certificate rotates; the intermediate it hangs from changes rarely. To
refresh the pin when NIST moves to a new issuer (`nist_verify` then returns `Ok(false)` for the new certificate):
1. fetch the certificate the newest pulse names: `nist_fetch_certificate(pulse)`, or
   `https://beacon.nist.gov/beacon/2.0/certificate/<certificateId in hex>`;
2. read its `Authority Information Access` "CA Issuers" URL (`openssl x509 -in cert.pem -noout -text`) and download it;
3. convert it (`openssl x509 -inform DER -in issuer.crt -out fixtures/nist-issuer.pem`), check
   `openssl verify -partial_chain -trusted_first -CAfile fixtures/nist-issuer.pem cert.pem` says OK, and confirm the
   fingerprint against DigiCert's published list (https://www.digicert.com/kb/digicert-root-certificates.htm);
4. paste the PEM into `nist_pinned_issuer_pem()` (the test `test_nist_pinned_issuer_is_the_saved_fixture` fails until the
   two match) and update the fingerprint above;
5. record `tapes/nist-live.tape` again (below).

**The oldest certificate, and its own issuer.** Chain 2's first pulse (`/beacon/2.0/chain/2/pulse/1`, 2022-09-21) names
certificate `ed8e1b97...983c2b`, valid 2022-09-15 to 2023-09-15. It was issued by a different intermediate,
`C=US, O=DigiCert Inc, CN=DigiCert TLS RSA SHA256 2020 CA1` (valid until 2031-04-13, SHA-256 fingerprint
`52:27:4C:57:CE:4D:EE:3B:49:DB:7A:7F:F7:08:C0:40:F7:71:89:8B:3B:E8:87:25:A8:6F:B4:43:01:82:FE:14`), which hangs from
`DigiCert Global Root CA` (not Global Root G2). That intermediate is the second pin: `fixtures/nist-legacy-issuer.pem`,
copied into `nist_legacy_issuer_pem()`, trusted only for the ids in `fixtures/nist-legacy-certificate-ids.txt` /
`nist_legacy_certificate_ids()` (a test keeps each pair equal to its fixture). Without it every pulse from the first year of
chain 2 would verify `false`. Certificate, intermediate and root are saved as `fixtures/nist-legacy-certificate.pem`,
`nist-legacy-issuer.pem` and `nist-legacy-root.pem`, and the pulse as `fixtures/nist-pulse-1.json`. To refresh the
legacy pin (it only needs it if NIST's certificate history is re-read or a pulse is found under another old certificate):
1. take the pulse's `certificateId` (`/beacon/2.0/chain/2/pulse/<n>`) and fetch
   `https://beacon.nist.gov/beacon/2.0/certificate/<id>`; check `openssl x509 -outform DER` of it hashes (SHA-512) to the id
   and its SubjectAltName is `engine.beacon.nist.gov`;
2. read its "CA Issuers" URL (`openssl x509 -in cert.pem -noout -text`), download it and convert it
   (`openssl x509 -inform DER -in issuer.crt -out issuer.pem`); `openssl verify -no_check_time -partial_chain -CAfile issuer.pem cert.pem`
   must say OK, and so must `openssl verify -no_check_time -partial_chain -CAfile root.pem issuer.pem` for the DigiCert
   root named in the issuer's own "CA Issuers" URL (confirm its fingerprint against DigiCert's published list);
3. if the issuer is the one already pinned, add the id to that issuer's list (the current or the legacy one) in both the
   fixture file and `src/nist.gu`; if it is a new issuer, add a new issuer function, id list and fixtures beside the
   legacy ones and a branch for it in `nist_verify`, so the new issuer is paired with its own ids only;
4. save a real pulse that names the certificate as `fixtures/nist-pulse-<n>.json` and add a test that it verifies.

**Chain 1 (2018-07-23 to 2022-04-29, 1801851 pulses), and its four certificates.** `nist_fetch(1, n)` reaches NIST's first
chain, which ended when chain 2 began; its pulses are serialized exactly as chain 2's (the signature and `outputValue`
of pulses under each of its certificates were checked with openssl against the layout above), so `nist_verify` handles
them with the same code. Its certificates (found by a binary search on `certificateId` from the first pulse to the
last, then fetching each from `/beacon/2.0/certificate/<id>`; all four name `engine.beacon.nist.gov` and each hashes to its id):

| pulses | valid | issuer (chain) | pin |
|---|---|---|---|
| 1 to 502024 | 2018-01-10 to 2019-01-11 | DigiCert Global CA G2 (Global Root G2) | `nist_chain1_global_ca_g2_*` |
| 502025 to 926636 | 2019-06-11 to 2020-06-11 | DigiCert Global CA G2 (Global Root G2) | `nist_chain1_global_ca_g2_*` |
| 926637 to 1414537 | 2020-06-10 to 2021-06-11 | DigiCert SHA2 Secure Server CA (Global Root CA) | `nist_chain1_secure_server_*` |
| 1414538 to 1801851 | 2021-06-08 to 2022-06-22 | DigiCert TLS RSA SHA256 2020 CA1 (Global Root CA) | the legacy pin |

Each id is paired as above: its ids go with its own issuer only (`fixtures/nist-chain1-certificate-ids.txt`, which holds the
first two groups, and `nist-legacy-certificate-ids.txt`; `fixtures/nist-chain1-issuer-*.pem`; a test keeps every list and
PEM equal to its fixture). **The first certificate had expired when it was last used.** It lapsed on 2019-01-11T12:00Z; the
beacon paused from 2018-12-26 to 2019-01-28 (pulses 220394 and 220395), then went on signing with it until pulse 502024
(2019-08-16T14:11:00Z), and pulse 502025 names the next one. Those pulses are genuine and verify, so the "valid when the
pulse was made" check of `nist_verify` takes a per-id extension: for that one id the window ends at 2019-08-16T14:11:00Z
(`nist_validity_extension` in `src/nist.gu`) instead of the certificate's `notAfter`, and for every other id it ends at
`notAfter`. `nist_verify_pinned` has no extension, so called by hand it is `false` for those pulses.
To refresh or re-read chain 1: take pulses at both ends and at each change (binary search on `certificateId` at
`/beacon/2.0/chain/1/pulse/<n>`; the coarse sample step used was 20000 pulses, so a certificate used for fewer pulses than
that and then replaced by the same one again would be missed), then run steps 1 to 4 of "The oldest certificate"
above for each new id, and also compare each `timeStamp` with the certificate's `notBefore` and `notAfter` (`openssl x509 -noout -dates`):
a pulse outside them needs an extension like the one above, bounded to the last pulse seen under that certificate and
only after checking the signature with openssl. Save a real pulse per certificate as `fixtures/nist-chain1-pulse-<n>.json`
and add it to `test_nist_chain_1_pulses_verify_under_each_of_its_certificates`.

NIST tests run offline on fixtures saved live (`nist-pulse-2038647/2038648/2038144/2038145.json`,
`nist-certificate.pem`, and `nist-pulse-1.json` with `nist-legacy-certificate.pem` for the oldest certificate): the last pulse, two consecutive pulses, a pair across the 2026-11-21/22 UTC midnight, a
tampered `localRandomValue` (false), and other tampering. Forged-but-validly-issued pulses
(`test-ca.pem`, `test-leaf.pem`, `test-pulse-in-window/after/before.json`, made by `fixtures/make_test_pulses.sh`, keys
discarded) prove each check on its own: a leaf from a re-pinned issuer with a non-NIST name, an unknown certificate id,
or a `timeStamp` outside the leaf's validity is `false`, and the same leaf with every pin matching is `true`. `live_nist.gu` is the live check (newest pulse, five
earlier pulses with their precommitments, and the day-boundary pair), recorded to `tapes/nist-live.tape`; record
again with `guion run --allow-http-host beacon.nist.gov --record tapes/nist-live.tape live_nist.gu`.

## Bad shapes

Every record these functions read comes from the network, so none of them stops the program on a malformed answer:
a Twine record whose `cid` is not `{"/": text}` is `E_TWINE_FORMAT`, a JWS header with no `alg` is `E_TWINE_ALG`, a
chain record with no `key` is `E_TWINE_KEY`, `links` that is not an array counts as no links, a CURBy payload that
lacks a member or has it in the wrong shape is `E_CURBY_STAGE`, `E_CURBY_RANDOMNESS` or `E_CURBY_RNG`, a NIST pulse
with a missing or mistyped member is `E_NIST_FORMAT`, and a drand answer is `E_DRAND_FETCH`. `json_items(value)`
(`twine.gu`) reads a JSON array as a list, or `None`; `twine_link(pulse, n)` and `twine_link_count(pulse)` read the
`links` of a pulse without raising.

A `timeStamp` fraction of any length is only checked for digits, and the date must exist (2026-02-31 and 2100-02-29 are
`E_NIST_FORMAT`). `twine_skip_links_hold(chain, pulse, pool)` (used by `twine_verify_links_in`) is `false`, never an
abort, for a chain whose `links_radix` is above 2^32 or a pulse whose skip links pass 10^18.

## Tests

Twine and CURBy tests run offline on fixtures saved live from `random.colorado.edu` (`twine-chains.json`,
`curby-q-pulses.json`, `curby-rng-pulses.json`, `twine-weather-pulses.json`), including flipped content,
signature and CID bytes (also with the CID recomputed so only the signature check can catch it), a swapped
key, a foreign pulse, an unknown algorithm, and the randomness values CURBy's JavaScript client gives for the
same pulses. `live_curby.gu` is the live check and `tapes/curby-live.tape` its recording
(`guion run --replay tapes/curby-live.tape live_curby.gu`); to record again:
`guion run --allow-http-host random.colorado.edu --record tapes/curby-live.tape live_curby.gu`.
A replay of an edited library still runs when every outside call (kind, URL, body, order) is the one
recorded, with a note that the program hash differs; `--replay-strict` refuses it. Record again when a
library change alters what is fetched.

`guion test` (in this directory; it finds `tests.gu`) runs offline: fixtures in `fixtures/` (made by Go `tle` v1.2.0 on quicknet) are read
from the package directory. `live_drand.gu` is the live check; `tapes/drand-100.tape` is its recorded run
(100 consecutive rounds verified, plus `tlock_open` of the round-1000 fixture), and
`guion run --replay tapes/drand-100.tape live_drand.gu` replays it with no grants. To record again:

    guion run --allow-http-host api.drand.sh --allow-http-host drand.cloudflare.com --allow-clock \
        --record tapes/drand-100.tape live_drand.gu
