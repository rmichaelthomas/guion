"""Declared managed-CPython adapter for parsing and atomic filesystem IO.
The Veritas entry point owns age/limit breach decisions and report construction.
"""
import csv
import datetime as dt
import json
import os
from pathlib import Path
import re
import tempfile

HELP = 'usage: sla-report --tickets PATH --policy PATH --as-of RFC3339 --output PATH [--team NAME]'
STAMP = re.compile(r'[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]{1,6})?(?:Z|[+-][0-9]{2}:[0-9]{2})\Z')

class InputError(ValueError):
    pass

def timestamp(text, context):
    if not STAMP.fullmatch(text):
        raise InputError(f'invalid_timestamp: {context}: expected YYYY-MM-DDTHH:MM:SS[.ffffff](Z|+HH:MM|-HH:MM)')
    if text[-1] != 'Z' and (int(text[-5:-3]) > 23 or int(text[-2:]) > 59):
        raise InputError(f'invalid_timestamp: {context}: invalid timezone offset')
    try:
        return dt.datetime.fromisoformat(text).astimezone(dt.timezone.utc)
    except (ValueError, OverflowError) as exc:
        raise InputError(f'invalid_timestamp: {context}: {exc}') from exc

def pairs(items):
    result = {}
    for k,v in items:
        if k in result:
            raise InputError(f'invalid_policy: duplicate JSON key {k!r}')
        result[k] = v
    return result

def load(arguments):
    try:
        if arguments == ['--help'] or arguments == ['-h']:
            return {'ok': True, 'help': True, 'message': HELP}
        flags = {'--tickets','--policy','--as-of','--output'}
        options = {}
        if len(arguments) not in {8, 10}:
            raise InputError('usage_error: ' + HELP)
        for i in range(0,len(arguments),2):
            key,value = arguments[i:i+2]
            if key not in flags | {'--team'} or key in options or not value:
                raise InputError('usage_error: unknown, duplicate, or empty argument: ' + key)
            options[key] = value
        if set(options) - {'--team'} != flags:
            raise InputError('usage_error: ' + HELP)
        as_of = timestamp(options['--as-of'], '--as-of')
        try:
            policy = json.loads(Path(options['--policy']).read_text(encoding='utf-8'), object_pairs_hook=pairs,
                                parse_constant=lambda value: (_ for _ in ()).throw(InputError('invalid_policy: non-finite JSON number')))
        except (ValueError, UnicodeError) as exc:
            raise InputError(f'invalid_policy: {exc}') from exc
        if not isinstance(policy,dict) or not isinstance(policy.get('response_minutes'),dict):
            raise InputError('invalid_policy: expected object containing response_minutes object')
        limits = policy['response_minutes']
        for key,value in limits.items():
            if key not in {'high','low'} or type(value) is not int or value < 0:
                raise InputError(f'invalid_policy: response_minutes.{key} must be a nonnegative JSON integer for high or low')
        tickets = []
        seen = set()
        with open(options['--tickets'],encoding='utf-8',newline='') as stream:
            reader = csv.reader(stream,strict=True)
            if next(reader,None) != ['id','team','priority','status','created_at']:
                raise InputError('malformed_header: expected id,team,priority,status,created_at')
            for row in reader:
                line = reader.line_num
                if len(row) != 5 or any(field == '' for field in row):
                    raise InputError(f'malformed_row: line {line}: expected five non-empty fields')
                ident,team,priority,status,created = row
                if ident in seen:
                    raise InputError(f'duplicate_id: line {line}: {ident!r}')
                seen.add(ident)
                if priority not in {'high','low'}:
                    raise InputError(f'unknown_priority: line {line}: {priority!r}')
                if priority not in limits:
                    raise InputError(f'missing_policy_entry: line {line}: response_minutes.{priority}')
                if status not in {'open','closed'}:
                    raise InputError(f'unknown_status: line {line}: {status!r}')
                created_at = timestamp(created, f'line {line} created_at')
                elapsed = as_of-created_at
                microseconds = (elapsed.days*86400 + elapsed.seconds)*1000000 + elapsed.microseconds
                if microseconds < 0:
                    raise InputError(f'future_created_at: line {line}: created_at is later than --as-of')
                tickets.append({'id':ident,'team':team,'priority':priority,'status':status,
                                'elapsed_microseconds':microseconds,'limit_minutes':limits[priority]})
        if '--team' in options:
            tickets = [row for row in tickets if row['team'] == options['--team']]
        tickets.sort(key=lambda row:(row['team'],row['id']))
        return {'ok':True,'help':False,'as_of':as_of.isoformat().replace('+00:00','Z'),
                'tickets':tickets,'output':options['--output']}
    except (InputError,OSError,UnicodeError,csv.Error) as exc:
        return {'ok':False,'error':str(exc)}

def save(path, content):
    temporary = None
    try:
        target = Path(path)
        with tempfile.NamedTemporaryFile(mode='w',encoding='utf-8',newline='\n',dir=target.parent,
                                         prefix='.'+target.name+'.',suffix='.tmp',delete=False) as stream:
            temporary = stream.name
            stream.write(content+'\n')
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary,target)
        return ''
    except (OSError,UnicodeError) as exc:
        return f'output_error: {path}: {exc}'
    finally:
        if temporary and os.path.exists(temporary):
            os.unlink(temporary)
