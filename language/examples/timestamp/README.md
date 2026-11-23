# timestamp

A trusted timestamp (RFC 3161) from DigiCert, checked in Guion.

`main.gu` hashes a message with SHA-512, builds a timestamp request with `crypto_tsa_request` and a fresh
8-byte nonce (`crypto_random_bytes`, with the first byte forced into `0x01` to `0x7f` because a nonce is a
minimal positive INTEGER), POSTs it to `http://timestamp.digicert.com` with `http_request_bytes`, takes the
token out of the answer with `crypto_tsa_response_token`, and verifies it with `crypto_tsa_verify` against the
two DigiCert certificates pinned in the program (the signing certificate and its issuer). It then checks
that the TSA echoed the nonce and prints the time the token vouches for.

```
guion run main.gu                                         # needs the network host in guion.toml
guion run --replay tapes/tsa-live.tape main.gu            # no network: replays the recorded answer
guion run --record tapes/tsa-live.tape main.gu            # records again
```

`tapes/tsa-live.tape` is a real run: every host call the program made (the random bytes and the HTTP
exchange) in order. The test `timestamp_example_replays_offline` in `crates/cli/tests/stdlib_crypto.rs`
replays it. A new recording has a new time, and it needs the pinned certificates to still be the ones
DigiCert signs with; `crypto_tsa_certificates(token)` lists the certificates a token carries when they
rotate.

What a verified token proves: that the TSA signed this digest at `gen_time_ms`, with the pinned key. It does not
prove the message is true, and it does not look at today's date or at revocation. Pin the certificates and
decide for yourself how much to trust the TSA behind them.

Terms of the free service: see `crates/crypto/tests/fixtures/tsa/README.md`.
