# Ed25519 fixtures

- `rfc8032_section_7_1.json`: the five Ed25519 vectors of RFC 8032 section 7.1 (TEST 1, TEST 2, TEST 3,
  TEST 1024, SHA(abc)), extracted from the RFC text (https://www.rfc-editor.org/rfc/rfc8032.txt, fetched
  2026-11-23) with the page headers and footers removed. Fields are hex: `seed` (the RFC's SECRET KEY),
  `public_key`, `message`, `signature`. TEST 1 matches the values the plan cross-checked (seed
  `9d61b19d...7f60`, public key `d75a9801...511a`, empty message, signature `e5564300...100b`); where the
  RFC and any other text differ, the RFC wins.
- `speccheck_cases.json`: the 12 cases of `cases.json` from `novifinancial/ed25519-speccheck`
  (https://github.com/novifinancial/ed25519-speccheck, `cases.json`, fetched 2026-11-23), the cases of
  "Taming the Many EdDSAs" (Chalkias, Garillot, Nikolaenko, 2020). Each case gains `number` (0 to 11, the
  order in the file) and `verify_strict`, what `ed25519-dalek`'s `VerifyingKey::verify_strict` returns
  (recorded by running it on 2.2.0): only case 3 verifies; the small-order keys and R values, the
  non-canonical R and S, and the mixed-order cases that need the cofactored equation are all rejected.
