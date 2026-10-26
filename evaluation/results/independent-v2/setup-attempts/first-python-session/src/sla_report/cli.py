"""Strict input validation, deterministic reporting, and atomic output."""
import argparse
import csv
from datetime import datetime, timedelta, timezone
import json
import os
from pathlib import Path
import re
import sys
import tempfile

HEADER = ['id', 'team', 'priority', 'status', 'created_at']
TIMESTAMP = re.compile(
    r'[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}'
    r'(?:\.[0-9]{1,6})?(?:Z|[+-][0-9]{2}:[0-9]{2})'
)


class ReportError(Exception):
    """An actionable input error with a stable diagnostic code."""

    def __init__(self, code, message):
        self.code = code
        super().__init__(message)


def timestamp(value, field):
    if not TIMESTAMP.fullmatch(value):
        raise ReportError('invalid_timestamp', f'{field}: expected YYYY-MM-DDTHH:MM:SS[.fraction](Z|±HH:MM), with 1–6 fractional digits')
    try:
        if value[-1] != 'Z' and (int(value[-5:-3]) > 23 or int(value[-2:]) > 59):
            raise ValueError('timezone offset out of range')
        return datetime.fromisoformat(value).astimezone(timezone.utc)
    except (ValueError, OverflowError) as exc:
        raise ReportError('invalid_timestamp', f'{field}: {value!r}: {exc}') from exc


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f'duplicate JSON key {key!r}')
        result[key] = value
    return result


def invalid_constant(value):
    raise ValueError(f'invalid JSON constant {value}')


def read_policy(path):
    try:
        with open(path, encoding='utf-8') as stream:
            policy = json.load(stream, object_pairs_hook=unique_object, parse_constant=invalid_constant)
    except (ValueError, UnicodeError, RecursionError) as exc:
        raise ReportError('invalid_policy', f'{path}: {exc}') from exc
    if not isinstance(policy, dict) or not isinstance(policy.get('response_minutes'), dict):
        raise ReportError('invalid_policy', f'{path}: expected an object containing a response_minutes object')
    limits = policy['response_minutes']
    for priority, limit in limits.items():
        if priority not in ('high', 'low'):
            raise ReportError('invalid_policy', f'{path}: unknown response_minutes key {priority!r}; use high or low')
        if type(limit) is not int or limit < 0:
            raise ReportError('invalid_policy', f'{path}: response_minutes.{priority} must be a nonnegative JSON integer')
    return limits


def report(tickets_path, limits, as_of):
    tickets = []
    seen = set()
    try:
        with open(tickets_path, encoding='utf-8', newline='') as stream:
            rows = csv.reader(stream, strict=True)
            if next(rows, None) != HEADER:
                raise ReportError('malformed_csv', f'{tickets_path}: header must be exactly {",".join(HEADER)}')
            for row in rows:
                context = f'{tickets_path}: line {rows.line_num}'
                if len(row) != 5 or any(not value.strip() for value in row):
                    raise ReportError('malformed_csv', f'{context}: expected exactly five non-empty fields; blank rows are invalid')
                ticket_id, team, priority, status, created_text = row
                if ticket_id in seen:
                    raise ReportError('duplicate_id', f'{context}: duplicate ID {ticket_id!r}')
                seen.add(ticket_id)
                if priority not in ('high', 'low'):
                    raise ReportError('unknown_priority', f'{context}: {priority!r}; use high or low')
                if priority not in limits:
                    raise ReportError('missing_policy_entry', f'{context}: add response_minutes.{priority} to the policy')
                if status not in ('open', 'closed'):
                    raise ReportError('unknown_status', f'{context}: {status!r}; use open or closed')
                created = timestamp(created_text, f'{context}: created_at')
                if created > as_of:
                    raise ReportError('future_created_at', f'{context}: created_at is later than --as-of')
                age = (as_of - created) // timedelta(minutes=1)
                limit = limits[priority]
                tickets.append(dict(id=ticket_id, team=team, priority=priority, status=status,
                                    age_minutes=age, limit_minutes=limit,
                                    breached=status == 'open' and age > limit))
    except (csv.Error, UnicodeError) as exc:
        raise ReportError('malformed_csv', f'{tickets_path}: {exc}') from exc
    tickets.sort(key=lambda ticket: (ticket['team'], ticket['id']))
    return {'as_of': as_of.isoformat().replace('+00:00', 'Z'), 'tickets': tickets,
            'summary': {'total': len(tickets), 'breached': sum(t['breached'] for t in tickets)}}


def write_atomic(path, result):
    # Serialize and validate everything before creating a temporary output file.
    content = json.dumps(result, ensure_ascii=False, indent=2, allow_nan=False) + '\n'
    destination = Path(path)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode='w', encoding='utf-8', newline='\n',
                                         dir=destination.parent, prefix='.sla-report-',
                                         suffix='.tmp', delete=False) as stream:
            temporary = stream.name
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, destination)
        temporary = None
    finally:
        if temporary is not None:
            os.unlink(temporary)


def main(argv=None):
    parser = argparse.ArgumentParser(prog='sla-report', description='Produce a validated support ticket SLA report.')
    parser.add_argument('--tickets', required=True, type=Path, help='UTF-8 tickets CSV')
    parser.add_argument('--policy', required=True, type=Path, help='JSON response_minutes policy')
    parser.add_argument('--as-of', required=True, help='RFC3339 report time including timezone')
    parser.add_argument('--output', required=True, type=Path, help='JSON output; replaced only on success')
    args = parser.parse_args(argv)
    try:
        as_of = timestamp(args.as_of, '--as-of')
        limits = read_policy(args.policy)
        result = report(args.tickets, limits, as_of)
        write_atomic(args.output, result)
    except ReportError as exc:
        print(f'sla-report: {exc.code}: {exc}', file=sys.stderr)
        return 2
    except OSError as exc:
        print(f'sla-report: io_error: {exc}', file=sys.stderr)
        return 2
    return 0
