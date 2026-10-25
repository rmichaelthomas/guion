# Field Kit Log

Requires CPython 3.14.7, with no external packages. `python3 --version` must report that version.

## Install and choose storage

1. Put `field-kit-log.pyz` in a directory dedicated to the installed app, for example `installed app/`.
2. Create a separate writable data directory, for example `operator data/`. The app deliberately requires this directory to exist and rejects databases inside its installed app directory.
3. Copy `config.sample.toml` to `config.toml` beside `operator data/`. Set `[storage] database` to the desired SQLite filename. Relative paths are resolved beside the TOML file; absolute paths are also accepted. Quote paths with spaces or Unicode in TOML and quote shell path arguments.

The SQLite file is created on the first successful `add`. If no database exists, `list` prints just its header. An inspection of an unknown item is rejected.

## Operate

Run these commands from the directory containing `config.toml`, `installed app/`, and `operator data/`:

```sh
python3 'installed app/field-kit-log.pyz' --config config.toml add radio-1 'Field radio'
python3 'installed app/field-kit-log.pyz' --config config.toml inspect radio-1 pass 'Battery charged'
python3 'installed app/field-kit-log.pyz' --config config.toml inspect radio-1 fail 'Antenna damaged'
python3 'installed app/field-kit-log.pyz' --config config.toml list
python3 'installed app/field-kit-log.pyz' --config config.toml list --failed-only
```

Inspections automatically use the computer's current local calendar date, in `YYYY-MM-DD` form. There is no operator-entered date argument. History is tab-separated, ordered by its permanent inspection number, and includes ID, equipment ID, date, result, and note. `--failed-only` filters results while preserving their original order and numbers. The normal `list` continues to include both results.

IDs must be unique. IDs, equipment names, and notes require 1–280 characters, with no tabs or line breaks. `pass` and `fail` are the only result values. Errors are printed to standard error and return a nonzero exit code; rejected duplicate IDs, unknown items, and invalid results preserve previous records. Fix the input and rerun the command.

## Move, back up, or rebuild

Stop running commands before copying the database for a backup. The database contains all equipment and inspections. Keep a copy of its TOML configuration too.

The installed `.pyz` can be moved to a path with spaces or Unicode. Point `--config` at the existing configuration; an absolute database path keeps using the original data directory. If moving the data itself, first create the new writable directory, copy the database while the app is stopped, then update the TOML path. Keep data outside the installed runtime.

The delivered `field_kit_log.py` is the complete maintainable source. To rebuild with the pinned interpreter, create a temporary empty packaging directory, copy the source into it as `__main__.py`, and run `python3 -m zipapp PACKAGING_DIRECTORY -o field-kit-log.pyz -p '/usr/bin/env python3'`. No compilation or dependency installation is needed. Run `--help` for command help.
