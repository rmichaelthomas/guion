#!/usr/bin/env python3
"""Audit a UTF-8 tab-separated invoice ledger, using integer cents."""
import argparse
from datetime import date
import json
import os
from pathlib import Path
import re
import sys
import tempfile

HEADER = ['invoice_id', 'vendor', 'amount_cents', 'due_date', 'state']

def parse_date(value):
    if re.fullmatch(r'[0-9]{4}-[0-9]{2}-[0-9]{2}', value) is None:
        raise ValueError('date must use YYYY-MM-DD')
    try:
        return date.fromisoformat(value)
    except ValueError as exc:
        raise ValueError('date must be a valid calendar date') from exc

def parse_row(fields):
    if len(fields) != 5:
        raise ValueError(f'expected 5 tab-separated fields; got {len(fields)}')
    invoice_id, vendor, amount, due, state = fields
    if not invoice_id:
        raise ValueError('invoice_id must not be empty')
    if not vendor:
        raise ValueError('vendor must not be empty')
    if re.fullmatch(r'[+-]?[0-9]+', amount) is None:
        raise ValueError('amount_cents must be an integer')
    try:
        cents = int(amount)
    except ValueError as exc:
        raise ValueError('amount_cents is too long') from exc
    try:
        due_date = parse_date(due)
    except ValueError as exc:
        raise ValueError(f'due_date: {exc}') from exc
    if state not in ('open', 'paid'):
        raise ValueError('state must be open or paid')
    return vendor, cents, due_date, state

def audit(input_path, as_of):
    vendors = {}
    rejected = []
    with input_path.open('r', encoding='utf-8-sig', newline='') as ledger:
        header = ledger.readline().rstrip('\r\n').split('\t')
        if header != HEADER:
            raise ValueError('input header must be: ' + '\\t'.join(HEADER))
        for line_number, line in enumerate(ledger, 2):
            try:
                vendor, cents, due, state = parse_row(line.rstrip('\r\n').split('\t'))
            except ValueError as exc:
                rejected.append({'line': line_number, 'reason': str(exc)})
                continue
            totals = vendors.setdefault(vendor, {
                'vendor': vendor, 'overdue_open_count': 0,
                'overdue_open_cents': 0, 'total_open_cents': 0,
            })
            if state == 'open':
                totals['total_open_cents'] += cents
                if due < as_of:
                    totals['overdue_open_count'] += 1
                    totals['overdue_open_cents'] += cents
    return {
        'as_of': as_of.isoformat(),
        'vendors': [vendors[name] for name in sorted(vendors)],
        'rejected_row_count': len(rejected), 'rejected_rows': rejected,
    }

def write_atomic(output_path, result):
    # Serialize before touching the destination, then replace in its directory.
    data = json.dumps(result, ensure_ascii=False, indent=2, sort_keys=True) + '\n'
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode='w', encoding='utf-8', newline='\n',
                                         dir=output_path.parent, prefix='.invoice-',
                                         suffix='.tmp', delete=False) as stream:
            temporary = Path(stream.name)
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, output_path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)

def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input', required=True, type=Path, help='UTF-8 TSV ledger')
    parser.add_argument('--as-of', required=True, help='YYYY-MM-DD; due dates before this date are overdue')
    parser.add_argument('--output', required=True, type=Path, help='destination JSON path')
    args = parser.parse_args(argv)
    try:
        as_of = parse_date(args.as_of)
        if args.input.resolve() == args.output.resolve():
            raise ValueError('input and output paths must differ')
        result = audit(args.input, as_of)
        write_atomic(args.output, result)
    except (OSError, UnicodeError, ValueError) as exc:
        print(f'error: {exc}', file=sys.stderr)
        return 2
    for row in result['rejected_rows']:
        print(f"line {row['line']}: {row['reason']}", file=sys.stderr)
    return 0

if __name__ == '__main__':
    raise SystemExit(main())
