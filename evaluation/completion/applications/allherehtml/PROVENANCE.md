# AllHereHTML regression fixture

This snapshot comes from `/Users/rmichaelthomas/allherehtml/guion` at commit
`a6f3291`. It contains the Guion builder, manifest, template, three source
examples, and the generated page used as an output oracle. The originating
project's `guion/QA.md` records the original checks and browser smoke test.

- `AllHereHTML.gu` SHA-256: `e1d4054a9c2d341d5b535aafe7a4d529d00c25762321dfc400875d1d990e4e37`
- Expected page SHA-256: `4e56c9c90df40f1220505a94d45687410be6308d49e53306e2c7e7d21d1e4cc5`

`evaluation/completion/check_allherehtml.py` runs the fixture in repository-local
scratch. It checks the language tests, exact generated output, and refusal to
replace a valid page when the template contains a script or is unavailable.
