#!/usr/bin/env python3
"""Field Kit Log: a local, configured inspection history."""
import argparse
from datetime import date
from pathlib import Path
import sqlite3
import sys
import tomllib

SCHEMA = '''
CREATE TABLE IF NOT EXISTS equipment (
    item_id TEXT PRIMARY KEY, name TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS inspections (
    inspection_id INTEGER PRIMARY KEY AUTOINCREMENT,
    item_id TEXT NOT NULL REFERENCES equipment(item_id),
    inspected_on TEXT NOT NULL,
    result TEXT NOT NULL CHECK (result IN ('pass', 'fail')),
    note TEXT NOT NULL
);
'''


def text(value):
    value = value.strip()
    if not value or len(value) > 280 or any(c in value for c in '\n\r\t'):
        raise argparse.ArgumentTypeError('use 1–280 characters without tabs or newlines')
    return value


def database_path(config):
    config = config.resolve()
    with config.open('rb') as stream:
        settings = tomllib.load(stream)
    value = settings.get('storage', {}).get('database')
    if not isinstance(value, str) or not value.strip():
        raise ValueError('TOML must define [storage] database as a nonempty path')
    path = Path(value).expanduser()
    if not path.is_absolute():
        path = config.parent / path
    path = path.resolve()
    if not path.parent.is_dir():
        raise ValueError('database parent directory must already exist')
    runtime = Path(sys.argv[0]).resolve().parent
    if path.is_relative_to(runtime):
        raise ValueError('database must be outside the installed runtime directory')
    return path


def run(args):
    path = database_path(args.config)
    if not path.exists() and args.command != 'add':
        if args.command == 'list':
            print('inspection_id\titem_id\tdate\tresult\tnote')
            return
        raise ValueError(f'unknown item: {args.item_id}')
    with sqlite3.connect(path) as db:
        db.execute('PRAGMA foreign_keys = ON')
        if args.command == 'add':
            db.executescript(SCHEMA)
            try:
                db.execute('INSERT INTO equipment VALUES (?, ?)', (args.item_id, args.name))
            except sqlite3.IntegrityError as error:
                raise ValueError(f'duplicate item ID: {args.item_id}') from error
            print(f'Added {args.item_id}: {args.name}')
        elif args.command == 'inspect':
            if not db.execute('SELECT 1 FROM equipment WHERE item_id = ?', (args.item_id,)).fetchone():
                raise ValueError(f'unknown item: {args.item_id}')
            stamp = date.today().isoformat()
            db.execute('INSERT INTO inspections (item_id, inspected_on, result, note) VALUES (?, ?, ?, ?)',
                       (args.item_id, stamp, args.result, args.note))
            print(f'Recorded {args.result} for {args.item_id} on {stamp}')
        else:
            print('inspection_id\titem_id\tdate\tresult\tnote')
            query = 'SELECT inspection_id, item_id, inspected_on, result, note FROM inspections'
            if args.failed_only:
                query += " WHERE result = 'fail'"
            query += ' ORDER BY inspection_id'
            for row in db.execute(query):
                print('\t'.join(map(str, row)))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config', type=Path, default=Path('config.toml'), help='TOML configuration (default: config.toml)')
    commands = parser.add_subparsers(dest='command', required=True)
    add = commands.add_parser('add', help='add equipment with a unique ID')
    add.add_argument('item_id', type=text)
    add.add_argument('name', type=text)
    inspect = commands.add_parser('inspect', help='record an inspection with today’s local date')
    inspect.add_argument('item_id', type=text)
    inspect.add_argument('result', choices=('pass', 'fail'))
    inspect.add_argument('note', type=text)
    listing = commands.add_parser('list', help='list history in recording order')
    listing.add_argument('--failed-only', action='store_true', help='show only failed inspections')
    args = parser.parse_args()
    try:
        run(args)
    except (OSError, ValueError, sqlite3.Error, tomllib.TOMLDecodeError) as error:
        print(f'Error: {error}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
