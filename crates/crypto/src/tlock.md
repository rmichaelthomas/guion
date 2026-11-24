# drand timelock (tlock) in the age format

This is the construction `tlock.rs` and `age.rs` implement, written from the reference source
(not from memory) before coding. Paths are inside the Go module cache.

- TLOCK  = `github.com/drand/tlock@v1.2.0`
- KYBER  = `github.com/drand/kyber@v1.3.1`
- KBLS   = `github.com/drand/kyber-bls12381@v0.3.1`
- KILIC  = `github.com/kilic/bls12-381@v0.1.0`
- DRAND  = `github.com/drand/drand/v2@v2.0.2`
- AGE    = `filippo.io/age@v1.1.1`

Interop target: files `tle` v1.2.0 writes decrypt here and the other way round. Network is the
G1-signature scheme `bls-unchained-g1-rfc9380` (quicknet), the only one supported.

## 1. Which IBE variant

`TimeLock` switches on the scheme name (TLOCK `tlock.go:131-146`). `bls-unchained-g1-rfc9380`
(`SigsOnG1ID`, DRAND `crypto/schemes.go:214`) takes `ibe.EncryptCCAonG2(bls.NewBLS12381Suite(), publicKey, id, data)`
(`tlock.go:140-141`) and decrypts with `ibe.DecryptCCAonG2` (`tlock.go:193-199`; unmarshals the signature as a G1 point).
So: master public key on G2 (96 bytes), identity point and round signature on G1 (48 bytes),
ciphertext `U` on G2 (96 bytes). `NewBLS12381Suite()` uses the default RFC 9380 DST for G1.

## 2. Identity

`id = scheme.DigestBeacon(Beacon{Round})` = SHA-256 of the round as 8 big-endian bytes
(DRAND `crypto/schemes.go:233-237`, `binary.Write(h, BigEndian, uint64)`). This is the same 32-byte
message that is BLS-signed for the round, so the round signature is `s * Q_id` with
`Q_id = H1(id)` and `s` the network's secret key.

`H1`: RFC 9380 `hash_to_curve` on G1, suite `BLS12381G1_XMD:SHA-256_SSWU_RO_`, DST
`BLS_SIG_BLS12381G1_XMD:SHA-256_SSWU_RO_NUL_` (KBLS `kyber_g1.go:15`, `Hash` at `kyber_g1.go:158-168`).
Rust: `bls::hash_to_g1(id, DST)`.

## 3. Encryption (`EncryptCCAonG2`, KYBER `encrypt/ibe/ibe.go:144-190`)

Inputs: master `P_pub` in G2, `id`, `msg` (here the 16-byte age file key). Hash is SHA-256
(KBLS `kyber_group.go:169`, `Suite.Hash`), so `len(msg) <= 32` (ibe.go:145).

1. `Q_id = H1(id)`; `G_id = e(Q_id, P_pub)` in GT (ibe.go:154-155). Pairing argument order: G1 point first.
2. `sigma`: `len(msg)` random bytes (ibe.go:159-161). **Deviation, deterministic:** derived from the caller's 48 bytes, see section 7.
3. `r = H3(sigma, msg)` (section 4) (ibe.go:163).
4. `U = r * G2_generator` (ibe.go:168). G2 generator `Base()` = `bls12381.NewG2().One()` (KBLS `kyber_g2.go:55`), the standard generator, same as arkworks' `G2Affine::generator()`.
5. `V = sigma XOR H2(G_id^r)` (ibe.go:171-176); `G_id.Mul(r, G_id)` is exponentiation in GT (KBLS `kyber_gt.go:74-79`).
6. `W = msg XOR H4(sigma)` (ibe.go:179-183).

Output `(U, V, W)`. Decryption (ibe.go:193-227), given round signature `S` (the "private" G1 point):

1. `G = e(S, U)` (ibe.go:199; `Pair(private, c.U)`, G1 first). Since `S = s*Q_id` and `U = r*P`, `e(S,U) = e(Q_id, s*P)^r = e(Q_id, P_pub)^r = G_id^r`.
2. `sigma = V XOR H2(G)` with `len = len(W)` (ibe.go:200-207; error if `len(V) != len(W)`).
3. `msg = W XOR H4(sigma)` (ibe.go:209-215).
4. `r = H3(sigma, msg)`; accept only if `r * G2_generator == U` (ibe.go:217-226). This check is the authentication: a wrong signature yields a random `sigma`, so the check fails. Rust: mismatch is `E_TLOCK_DECRYPT`. The decryption does not need the network public key.

Also rejected: `len(W) > 32` (ibe.go:194).

## 4. H3, H4, H2 (ibe.go:229-312)

- `H3Tag = "IBE-H3"`, `H4Tag = "IBE-H4"`, `H2Tag = "IBE-H2"` (ibe.go:25-37).
- **H3(sigma, msg)** (ibe.go:230-280): `buffer = SHA-256("IBE-H3" || sigma || msg)`. Then for `i = 1, 2, ...` (uint16, up to 65534):
  `hashed = SHA-256(LE16(i) || buffer)`; the scalar field is 255 bits and the encoding 32 bytes, so `toMask = 256 - 255 = 1`;
  scalars are big-endian (`mod.BigEndian`, KYBER `group/mod/int.go:28,81`), so `hashed[0] >>= 1` (ibe.go:263-265);
  accept the first `hashed` whose big-endian value is `< r_order` (`UnmarshalBinary` rejects otherwise, `int.go:340-353`).
  `r_order = 0x73eda753299d7d483339d80809a1d80553bda402fffe5bfeffffffff00000001` (KBLS `kyber_scalar.go:10`). The result is the scalar `r`.
- **H4(sigma, n)** (ibe.go:282-294): `SHA-256("IBE-H4" || sigma)[..n]`.
- **H2(G, n)** (ibe.go:296-312): `SHA-256("IBE-H2" || GT_bytes(G))[..n]`.

### GT serialization (what H2 hashes)

`gt.MarshalTo` writes `ToBytes` (KBLS `kyber_gt.go:81-91`): 576 bytes, 12 Fp elements of 48 bytes big-endian.
Order (KILIC `fp12.go:59-65`, `fp6.go:53-60`, `fp2.go:44-49`): the Fp12 element is `c0 + c1*w`, Fp6 is `b0 + b1*v + b2*v^2`, Fp2 is `a0 + a1*u`;
the output is `c1, c0`, within each Fp6 `b2, b1, b0`, within each Fp2 `a1, a0`. That is the exact reverse of the flattened
arkworks order `c0.b0.a0, c0.b0.a1, c0.b1.a0, ...`. The tower (`u^2 = -1`, `v^3 = u+1`, `w^2 = v`) is identical in both libraries.
Rust: serialize the 12 `Fp` values of the `Fp12` in reverse flattened order, each 48-byte big-endian.

## 5. Ciphertext to stanza body (TLOCK `tlock.go:157-180`, `tlock_age.go:23-42`)

`body = U (96 bytes, compressed G2, ZCash encoding) || V (16) || W (16)`, 128 bytes total; `cipherVLen = cipherWLen = 16`
(`tlock.go:155-158`). Decoding requires exactly 128 bytes (`BytesToCiphertext`, `tlock.go:186-212`) and a valid G2 point (`UnmarshalBinary`).
Stanza: type `tlock`, args `[round decimal, chain hash lowercase hex]` (`tlock_age.go:34-38`), body as above.

## 6. age v1 container (AGE)

All numbers below are for one `tlock` recipient (what `tle` produces). Strings are ASCII.

- **File key**: 16 random bytes (`age.go:99`, `124-127`). This is the `msg` encrypted by the IBE.
- **Header** (`internal/format/format.go:108-156`):
  `age-encryption.org/v1\n`, then each stanza `-> tlock <round> <chainhash>\n` followed by the body in *unpadded* standard base64 (`RawStdEncoding.Strict`)
  wrapped at 64 columns, with `\n` after every full 64-char line and a final short line (possibly empty if the length is a multiple of 48 bytes) ending with `\n`;
  then `--- <mac>\n` with the 32-byte MAC in unpadded base64.
- **Header MAC** (`primitives.go:52-63`): `hmacKey = HKDF-SHA256(ikm = fileKey, salt = none, info = "header")[..32]`;
  `mac = HMAC-SHA256(hmacKey, header_bytes)` where `header_bytes` is everything from the intro line through the three characters `---` (no space, no mac, no newline) (`MarshalWithoutMAC`, format.go:136-147).
- **Payload**: 16 random nonce bytes follow the header immediately (`age.go:100,148-154`). `payloadKey = HKDF-SHA256(ikm = fileKey, salt = nonce, info = "payload")[..32]` (`primitives.go:65-72`).
- **STREAM** (`internal/stream/stream.go`): ChaCha20-Poly1305 (RFC 8439, 12-byte nonce, no AAD), chunks of 65536 plaintext bytes (`:18`), each encrypted to 65552 bytes (`:31-34`).
  The 12-byte nonce is an 11-byte big-endian counter starting at 0, incremented per chunk, then a final byte: `0x00` for every chunk but the last, `0x01` for the last (`:130-147`, `218-231`).
  The last chunk may be short but is never empty unless it is the only chunk (empty plaintext = one 16-byte chunk). A full-size final chunk is flagged last (the writer holds a full chunk back until more data or close, `:171-190`).
  The reader (`:89-127`) rejects a stream with no final chunk (EOF at a chunk boundary), a 16-byte (empty) final chunk after the first, and bytes after a last chunk.
- **Armor** (`armor/armor.go:23-26,44-58`): `-----BEGIN AGE ENCRYPTED FILE-----\n`, the whole binary file in *padded* standard base64 wrapped at 64 columns (`\n` after each full line),
  a final `\n` only if the last line is short and non-empty, then `-----END AGE ENCRYPTED FILE-----\n`. The reader (`armor.go:80-170`) ignores leading whitespace lines, requires the exact header line, accepts `\r\n`, requires strict canonical padded base64 per line of at most 64 columns, ends at a short line (`< 48` bytes) which must be followed by the footer line, and allows only trailing whitespace afterwards.
- **Header parse** (`format.go:235-293`, `167-217`): exact intro line; stanzas start with `->`, arguments space separated, each of ASCII 33..126; body lines strict unpadded base64 of at most 48 bytes each, ended by a line shorter than 48 bytes; the footer line is `--- <mac>` with exactly one argument, a 32-byte MAC.

`tle` writes armor when `--armor` is given, and its `Decrypt` auto-detects the armor header (TLOCK `tlock.go:97-102`).

## 7. Deterministic randomness (a Guion decision, not in the reference)

`tlock_encrypt` takes `randomness: &[u8; 48]` (the Guion wrapper passes `crypto_random_bytes(48)`) and derives the three random values
with HKDF-SHA256, `ikm = randomness`, salt = none, one `expand` per label:

| value | HKDF info | length |
| --- | --- | --- |
| age file key | `guion tlock v1 file key` | 16 |
| IBE `sigma` | `guion tlock v1 ibe sigma` | 16 |
| STREAM nonce | `guion tlock v1 stream nonce` | 16 |

Same inputs and randomness give byte-identical armor. The values are independent HKDF outputs of a uniformly random 48-byte input. Output is still a valid age file: decryptors only see ordinary random-looking values.
Reusing the same 48 bytes for two different messages to the same round reuses the file key; the Guion wrapper always draws fresh bytes.

## 8. Errors

- `E_TLOCK_FORMAT`: armor, header, stanza or body shape is wrong, the stanza is not a `tlock` stanza with two arguments, the round or chain hash does not parse, the point `U` is not a valid G2 point, or the payload is shorter than the 16-byte nonce.
- `E_TLOCK_DECRYPT`: header MAC mismatch, IBE `rP` check failure (wrong round signature), STREAM authentication failure, truncation or trailing data in the payload.
- `E_CRYPTO_INPUT`: wrong length or invalid point for the master public key (96) or round signature (48), plaintext or chain-hash arguments out of range.
