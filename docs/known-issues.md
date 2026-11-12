# Known issues

This list covers open problems only. Each row has a status and the batch that will close it. When an item is fixed, its status changes to `fixed in <short commit>`.

| # | Problem | Status | Closed by |
|---|---------|--------|-----------|
| 1 | Reading CSV files is about 45 times slower than pandas. | fixed in 8cdf52c | Batch 1 |
| 2 | Calling a small helper inside a large guest loop costs microseconds per call. | fixed in ae485c5 | Batch 1 |
| 3 | Multiplying two 512 by 512 f64 matrices is about 25 times slower than NumPy. | open | Batch 3 |
| 4 | Element-wise array maths costs about 2 times NumPy's cost per element. | fixed in db55344 (10M `a*b+c` compute estimate 1.45 times NumPy, down from 2.1; an in-place `set r = array_add(r, b)` loop runs at 0.86 times NumPy) | Batch 1 |
| 5 | The self-hosted compiler emits `ArraySet` where it could emit `ArraySetLocal`. | fixed in 4c19507 | Batch 1 |
| 6 | Large files need the project to raise `max_file_bytes`, and the allocation-limit error does not say how to raise it. | fixed in c071ffd | Batch 1 |
| 7 | `array_i64_range` can panic in debug builds on extreme operands. | fixed in 6fb8d20 | Batch 2 |
| 8 | Temporary buffers in matmul, sort and argsort, factorize and join_index are not charged to the allocation budget. | fixed in 6fb8d20 | Batch 2 |
| 9 | Array error messages use internal operation names. `array_clip` with low greater than high raises `R_ARRAY_SHAPE`. | fixed in be924e7 | Batch 2 |
| 10 | The Rust and self-hosted compilers give different error codes for `Array[i128]` and `Array[Array[f64]]`. | open | Batch 2 |
| 11 | The spec says element types outside `Array[...]` should give `E_TYPE_UNKNOWN`. Both compilers give `E_TYPE_ANNOTATION`. | open | Batch 2 |
| 12 | `catalog_array_builtin` can return `Ok(None)` after it has emitted argument code. Only a test guards this. | open | Batch 2 |
| 13 | Empty text becomes missing after a CSV round trip. | fixed in 8cdf52c | Batch 1 |
| 14 | `frame_show` misaligns wide characters and text that contains newlines. | open | Batch 2 |
| 15 | A join fails when the `<name>_right` column name is already taken. | open | Batch 2 |
| 16 | `column_from_codes` does not check its codes. | open | Batch 2 |
| 17 | Very large and very small floats are written in full decimal form. | open | Batch 2 |
| 18 | JSONL keys first seen after line 1 are dropped. | open | Batch 2 |
| 19 | `array_mean_by` is not used by `frames`, and `group.gu` has an unused local. | open | Batch 2 |
| 20 | Filtering a 1M-row frame after the read takes about 10 ms, 2 to 4 times pandas (measured in the speed report; the pandas part is within the noise of the method). | open | Batch 2 |

Batch 1 covers items 1, 2, 4, 5, 6 and 13. Batch 2 covers items 7 to 12 and 14 to 20. Batch 3 covers item 3.
