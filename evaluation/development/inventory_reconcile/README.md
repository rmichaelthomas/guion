# Inventory reconciliation

This multi-module CLI applies an ordered stream of inventory quantity adjustments to a JSON inventory document. It validates duplicate SKUs and nonnegative quantities, adds new SKUs at their first adjustment, and publishes the complete result with an atomic replacement only after every change is valid. Each adjustment carries a reason and source; the output preserves an ordered audit record with the resulting balance.

The input files have these shapes:

```json
{"items":[{"sku":"A-1","quantity":10}]}
```

```json
{"adjustments":[{"sku":"A-1","delta":-2,"reason":"damaged","source":"inspection"},{"sku":"B-1","delta":7,"reason":"opening stock","source":"receiving"}]}
```

The output stores quantities in a SKU-keyed object and keeps each adjustment in input order, for example `{"items":{"A-1":8,"B-1":7},"changes":[{"balance":8,"delta":-2,"reason":"damaged","sku":"A-1","source":"inspection"}]}`. JSON encoding sorts object keys for stable bytes. The display report gives the adjustment count, product count, and final unit total.

Run it with:

```sh
veritas run . inventory.json adjustments.json reconciled.json
```

`reference.py` is an independent Python standard-library implementation. The acceptance test compares output, status, and exact persisted bytes for generated and invalid inputs. This is a known workload for implementation and repair checks, not an independent assistant evaluation.
