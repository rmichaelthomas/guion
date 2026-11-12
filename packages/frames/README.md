# frames

Data frames for Guion: named columns of equal length, read from CSV or JSONL, filtered, sorted, grouped, joined and written back out. The heavy work runs in Guion's built-in array kernels.

Use it from a project with `guion add frames --path PATH/TO/frames` and `import "pkg:frames/src/lib.gu"`. Check the library with `guion check tests.gu` and test it with `guion test tests.gu`.

```
import "pkg:frames/src/lib.gu"

fn main() -> Unit:
    let sales = unwrap(frame_read_csv("sales.csv"))
    let totals = unwrap(frame_group(sales, ["region"], ["amount:sum", "amount:count"]))
    print(frame_show(unwrap(frame_sort(totals, ["amount_sum"], [false])), 20))
```

## Columns

A column has a name and one of four kinds: `i64`, `f64`, `bool` or `text`. Text columns are stored as integer codes into a dictionary of distinct values, so grouping, joining and sorting compare integers. Any value can be missing.

- Build: `column_i64(name, values)`, `column_f64`, `column_bool`, `column_text`; `column_i64_opt(name, List[Option[Int]])` and the `_opt` forms for the other kinds; `column_from_i64(name, Array[i64], valid: Array[bool])` and the other `column_from_` forms (an empty `valid` means nothing is missing).
- Read: `column_name`, `column_kind`, `column_len`, `column_missing`, `column_count` (present values), `column_valid` (true where present), `column_get_i64/f64/bool/text(column, row) -> Option[...]`, `column_cell_text` (`NA` for missing), and the storage arrays `column_ints`, `column_floats`, `column_flags`, `column_codes`, `column_dictionary`.
- Compute: `column_compare(column, op, value)` (`<`, `<=`, `>`, `>=`, `==`, `!=`; missing rows are false), `column_text_equals(column, text)`, `column_sum`, `column_mean`; reductions skip missing values.

## Frames

Every operation that names a column or can meet bad input returns `Result[..., StdError]`, so `?` keeps a pipeline short. Error codes: `E_FRAME_COLUMN` (unknown or duplicate column), `E_FRAME_KIND` (wrong kind for the operation), `E_FRAME_SHAPE` (row count or schema mismatch), `E_FRAME_PARSE` (a value in a file, with its line and column). File errors keep their own codes.

- Shape: `frame_new(columns)`, `frame_rows`, `frame_width`, `frame_names`, `frame_columns`, `frame_column(frame, name)`, `frame_has`.
- Columns: `frame_select`, `frame_drop`, `frame_rename`, `frame_with_column` (replaces or appends).
- Rows: `frame_filter(frame, mask)`, `frame_take(frame, indices)`, `frame_head`, `frame_tail`, `frame_slice`, `frame_concat(frames)` (same names and kinds; text matched by value).
- Missing values: `frame_drop_missing(frame, names)` (all columns when `names` is empty), `frame_fill_i64`, `frame_fill_f64`, `frame_fill_bool`, `frame_fill_text`.
- `frame_sort(frame, keys, ascending)`: stable, first key most significant, text ordered by value, missing values last in either direction.
- `frame_group(frame, keys, ["column:function", ...])` with `sum`, `mean`, `min`, `max` or `count`; result columns are named `column_function`, groups appear in first-seen order, and a missing key value forms its own group. `count` counts present values; the `sum` of no values is 0; `mean`, `min` and `max` of no values are missing.
- `frame_join(left, right, keys, how)` with `how` `"inner"` or `"left"`: rows keep the left order, a left row matching several right rows repeats in right order, and a right column whose name is taken gets `_right`. A missing key never matches, not even another missing key; pandas matches missing keys by default, SQL does not, and this follows SQL.

## Files

- `frame_read_csv(path)` reads a file with a header row. Each column's kind is the first of `i64`, `f64`, `bool` (`true`/`false`) and `text` that holds every non-empty value; integers beyond the `i64` range make the column `f64`, and an empty field is missing. Blank lines are skipped, and a leading UTF-8 byte-order mark is not part of the first column name. `frame_read_csv_with(path, {"name": "kind"})` fixes kinds; a value that does not fit is `E_FRAME_PARSE`.
- `frame_read_jsonl(path)` reads one JSON object per line; any other JSON value is `E_FRAME_PARSE`. The first object's keys are the columns, in the order its JSON text lists them (sorted by key). A missing key or `null` is missing; kinds follow the JSON types, and integers mixed with other numbers become `f64`. Nested values become their JSON text, and keys that first appear in later objects are not read.
- `frame_write_csv(frame, path)` writes a header and the rows; missing values are empty fields and floats always carry a decimal point, so the file reads back with the same kinds. An empty text value is also written as an empty field, so it reads back as missing.
- `frame_show(frame, max_rows)` returns an aligned table with a `[rows x columns]` footer, showing the first and last rows with a `…` row when there are more than `max_rows`.

Reading a file is limited by the project's `[runtime] max_file_bytes` (16 MiB by default) and memory by `allocation_limit_bytes` (256 MiB by default); raise both in `guion.toml` for large files.

Not yet provided: dates and times, pivot and melt, rolling windows, string methods on columns, and Parquet or Arrow files.
