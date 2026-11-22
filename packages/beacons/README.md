# beacons

Checking public randomness beacons in Guion. This covers multiformats, drand, and Twine with CURBy
(including the quantum chain CURBy-Q); NIST comes in a later part. Every function is plain Guion over `std:crypto`, `std:encoding` and
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
  and `randomness == SHA-256(signature)`. A bad value is `Ok(false)`; malformed bytes are `Err`.
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
  chain, a single pulse by CID, and many single pulses by index zstd-compressed (`content-encoding:
  zstd`, even when not asked), which `http_get` cannot read. Range answers and `/chains` are plain
  JSON, so `twine_fetch_pulse` fetches an index as `<i>-<i>` and `twine_fetch_chain` reads `/chains`.
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

## Tests

Twine and CURBy tests run offline on fixtures saved live from `random.colorado.edu` (`twine-chains.json`,
`curby-q-pulses.json`, `curby-rng-pulses.json`, `twine-weather-pulses.json`), including flipped content,
signature and CID bytes (also with the CID recomputed so only the signature check can catch it), a swapped
key, a foreign pulse, an unknown algorithm, and the randomness values CURBy's JavaScript client gives for the
same pulses. `live_curby.gu` is the live check and `tapes/curby-live.tape` its recording
(`guion run --replay tapes/curby-live.tape live_curby.gu`); to record again:
`guion run --allow-http-host random.colorado.edu --record tapes/curby-live.tape live_curby.gu`.
Any edit under `src/` invalidates both tapes (`R_TAPE`); record again.

`guion test tests.gu` runs offline: fixtures in `fixtures/` (made by Go `tle` v1.2.0 on quicknet) are read
from the package directory. `live_drand.gu` is the live check; `tapes/drand-100.tape` is its recorded run
(100 consecutive rounds verified, plus `tlock_open` of the round-1000 fixture), and
`guion run --replay tapes/drand-100.tape live_drand.gu` replays it with no grants. To record again:

    guion run --allow-http-host api.drand.sh --allow-http-host drand.cloudflare.com --allow-clock \
        --record tapes/drand-100.tape live_drand.gu
