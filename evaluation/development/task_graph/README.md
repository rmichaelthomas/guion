# Task graph planner

This CLI reads a JSON task graph, verifies task identifiers and dependencies, and writes a deterministic topological plan. It reports duplicate identifiers, missing dependencies, and cycles without publishing a partial plan. Add `--priority` to prefer higher-priority ready tasks while preserving dependency order; ties are resolved by task identifier.

Input shape:

```json
{"tasks":[{"id":"compile","depends_on":[],"priority":1},{"id":"test","depends_on":["compile"],"priority":8}]}
```

Run from this directory with:

```sh
veritas run . tasks.json plan.json
veritas run . tasks.json plan.json --priority
```

The CLI restricts file access to this project directory. The output is written atomically after the whole graph is validated and planned.

`reference.py` is the standard-library behavior reference. The acceptance suite compares results and errors, tests a changed priority requirement, and runs a compiled artifact after removing its source file. These known author-run cases are integration evidence, not independent recommendation evidence.
