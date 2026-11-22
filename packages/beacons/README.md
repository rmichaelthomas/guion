# beacons

Checking public randomness beacons in Guion. This first part covers multiformats and drand; CURBy
and NIST come in later parts. Every function is plain Guion over `std:crypto`, `std:encoding` and
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

## Tests

`guion test tests.gu` runs offline: fixtures in `fixtures/` (made by Go `tle` v1.2.0 on quicknet) are read
from the package directory. `live_drand.gu` is the live check; `tapes/drand-100.tape` is its recorded run
(100 consecutive rounds verified, plus `tlock_open` of the round-1000 fixture), and
`guion run --replay tapes/drand-100.tape live_drand.gu` replays it with no grants. To record again:

    guion run --allow-http-host api.drand.sh --allow-http-host drand.cloudflare.com --allow-clock \
        --record tapes/drand-100.tape live_drand.gu
