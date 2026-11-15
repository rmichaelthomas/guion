# The basics list

**Version 1, as of Guion 0.5.0.**

This is the list of tasks a small program usually needs, with the function that covers each. The claim it supports is exactly this one: **Guion covers the basics as of this list.** It is not a claim that Guion covers everything a program might need, and the list is versioned so the claim can be checked: a test (`crates/cli/tests/basics_list.rs`) fails if any function named in the "Covered by" column is missing from the shipped standard library or the built-in functions.

Each release that changes the list raises its version and says so in the changelog. Adding a row is an addition; removing a row or changing what covers it is a change to this list and is stated the way any other break is.

Functions in the `std:` modules need `import "std:MODULE"`; the built-in functions (`print`, `read_text`, `len` and so on) need no import. `guion doc --all` lists every function and its module.

## Covered

| Basic | Covered by | Notes |
|---|---|---|
| Print and write to standard error | `print`, `eprint` | |
| Read and write a whole file | `read_text`, `read_bytes`, `write_text`, `write_text_atomic`, `write_bytes` | Needs a file grant; `write_text_atomic` replaces a file in one step. |
| Read a file line by line | `file_open_lines`, `file_next_line`, `file_close` | Streams, so memory stays bounded. |
| Files and directories | `fs_exists`, `fs_size`, `fs_list_dir`, `fs_walk`, `fs_make_dir`, `fs_copy`, `fs_rename`, `fs_remove`, `fs_remove_all`, `fs_temp_dir` | |
| File paths | `path_join`, `path_basename`, `path_dirname`, `path_extension`, `path_normalize` | |
| Text | `len`, `text_slice`, `text_find`, `text_split`, `text_join`, `text_trim`, `text_replace`, `text_chars`, `text_casefold`, `text_normalize` | |
| Parse and format numbers | `text_parse_integer`, `text_parse_float`, `text_parse_decimal`, `int_to_text`, `float_to_text`, `int_to_float`, `float_to_int` | |
| Maths | `math_abs`, `math_pow`, `math_floor`, `math_round`, `math_log`, `math_sin`, `sqrt` | |
| Lists and maps | `list_push`, `list_get`, `list_set`, `map_get`, `map_set`, `map_has` | |
| Sorting | `sort`, `sort_by`, `sort_by_int_key` | |
| Pass a function to a function | `map_list`, `filter_list`, `fold_list`, `any_list`, `all_list` | Pass a named function or write one in place, `fn(x: Int) -> Int: x + offset`, which keeps the values it uses. `sort_by` and `parallel_map` take a named function. |
| Handle a value that may be missing or fail | `Ok`, `Err`, `Some`, `is_ok`, `is_some`, `unwrap` | |
| Regular expressions | `regex_is_match`, `regex_find`, `regex_find_all`, `regex_replace`, `regex_split`, `regex_escape` | Linear-time syntax; no backreferences or lookaround. |
| JSON | `json_parse`, `json_stringify`, `json_get`, `json_index`, `json_object`, `json_array` | |
| CSV and JSON Lines | `csv_open`, `csv_next`, `csv_encode`, `jsonl_open`, `jsonl_next`, `jsonl_close` | |
| TOML and YAML | `toml_parse`, `toml_get_text`, `toml_render`, `yaml_parse` | YAML is read only. |
| Time and dates | `unix_time_ms`, `format_utc`, `parse_date`, `parse_timestamp`, `today_local`, `resolve_local` | Needs a clock grant. |
| Command-line arguments and environment | `args`, `env`, `parse_cli` | |
| Run another program | `process_run` | Needs a process grant naming the command. |
| Call a web service | `http_get`, `http_request`, `http_response_json` | |
| Serve HTTP | `http_server_listen`, `http_server_accept`, `http_server_respond` | The `web` package adds routing, sessions and templates on top. |
| URLs | `url_parse`, `url_encode_component`, `url_query_build`, `url_query_parse` | |
| Sockets and TLS | `tcp_connect`, `tcp_listen`, `udp_bind`, `tls_connect` | |
| Hashing and encoding | `crypto_sha256`, `crypto_hmac_sha256`, `hex_encode`, `base64_encode`, `base64url_encode` | |
| Random numbers | `random_new`, `random_next_range`, `crypto_random_bytes`, `crypto_random_hex` | The first two are seeded and repeatable; the last two come from the operating system. |
| Compress and archive | `gzip_compress`, `gzip_decompress`, `zlib_compress`, `zlib_decompress`, `tar_write`, `tar_read`, `tar_list` | |
| A database | `sqlite_open`, `sqlite_begin`, `sqlite_execute`, `sqlite_query`, `sqlite_commit` | |
| Logging | `log_info`, `log_warn`, `log_error`, `log_open` | |
| Version numbers | `parse_version`, `compare_versions` | |
| Run work on several threads | `parallel_map` | |
| Test a program | `assert` | Run with `guion test`. |

## Deliberately not covered

These are not in the standard library as of this list. A program that needs one uses a package or the host program.

| Not covered | Why it is on this list |
|---|---|
| zip, bzip2, xz and brotli archives | Compression covers gzip and zlib, and archives cover tar: the formats files usually arrive in. |
| zstd | Not built yet. |
| XML and HTML parsing | Not in the standard library. |
| Sending email | Not in the standard library. |
| Databases other than SQLite | Not in the standard library. |
| Image handling | Not in the standard library; the `evaluation/frameworks/publish-page` example runs an external tool for it. |
| Graphical user interfaces | Not in the standard library. |
