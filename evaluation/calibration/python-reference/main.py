#!/usr/bin/env python3
"""Author-run Python calibration candidate for the independent task matrix.

This is a known-good baseline for checking task fixtures and expected values. It
is not an independent evaluator or a result from a fresh assistant session.
"""
from __future__ import annotations
import argparse
import csv
import datetime as dt
import hashlib
import hmac
import json
import os
from pathlib import Path
import re
import sqlite3
import sys
import string
import tempfile
import threading
import time
import tomllib
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor

sys.path.insert(0, str(Path(__file__).resolve().parent / '.deps'))
from email.utils import parsedate_to_datetime


def write_atomic(path: str, data: str) -> None:
    p = Path(path)
    p.parent.mkdir(parents=True, exist_ok=True)
    fd, temp = tempfile.mkstemp(prefix=f'.{p.name}.', dir=str(p.parent))
    try:
        with os.fdopen(fd, 'w', encoding='utf-8', newline='') as f:
            f.write(data)
            f.flush()
            os.fsync(f.fileno())
        os.replace(temp, p)
    except BaseException:
        try: os.unlink(temp)
        except OSError: pass
        raise


def json_write(path: str, value) -> None:
    write_atomic(path, json.dumps(value, ensure_ascii=False, separators=(',', ':')) + '\n')


def format_utc(value: dt.datetime) -> str:
    value=value.astimezone(dt.timezone.utc)
    base=value.strftime('%Y-%m-%dT%H:%M:%S')
    fraction=f'{value.microsecond:06d}'.rstrip('0')
    return base + ('.'+fraction if fraction else '') + 'Z'


def parse_date(text: str) -> dt.datetime:
    if not isinstance(text, str) or not re.fullmatch(r'\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,6})?(?:Z|[+-]\d{2}:\d{2})', text):
        raise ValueError('timestamp must use RFC3339 date-time syntax')
    value = text[:-1] + '+00:00' if text.endswith('Z') else text
    parsed = dt.datetime.fromisoformat(value)
    if parsed.tzinfo is None: raise ValueError('timestamp must include a timezone')
    return parsed.astimezone(dt.timezone.utc)


def read_response_with_deadline(response, deadline: float) -> bytes:
    """Read a urllib response against one total wall-clock deadline."""
    sock = response.fp.raw._sock
    chunks = []
    reader = getattr(response, 'read1', response.read)
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError('response exceeded total deadline')
        if not getattr(sock, '_closed', False):
            try:
                sock.settimeout(remaining)
            except OSError:
                if not getattr(sock, '_closed', False):
                    raise
        chunk = reader(8192)
        if not chunk:
            return b''.join(chunks)
        chunks.append(chunk)


def fail(message: str, code: int = 1) -> int:
    print(message, file=sys.stderr)
    return code


def sla(args) -> int:
    try:
        policy_doc=json.loads(Path(args.policy).read_text(encoding='utf-8'))
        if not isinstance(policy_doc,dict) or not isinstance(policy_doc.get('response_minutes'),dict): raise ValueError('policy response_minutes must be an object')
        policy=policy_doc['response_minutes']
        for name,value in policy.items():
            if name not in ('high','low') or isinstance(value,bool) or not isinstance(value,int) or value<0: raise ValueError('policy limits must be nonnegative integers for high or low')
        as_of=parse_date(args.as_of)
        with Path(args.tickets).open(encoding='utf-8',newline='') as stream:
            reader=csv.reader(stream)
            if next(reader,None)!=['id','team','priority','status','created_at']: raise ValueError('invalid CSV header')
            raw_rows=list(reader)
        if any(len(row)!=5 or any(not field for field in row) for row in raw_rows): raise ValueError('malformed CSV row')
        rows=[dict(zip(('id','team','priority','status','created_at'),row)) for row in raw_rows]
        ids=set(); out=[]
        for row in rows:
            if not row or not all(row.get(k) is not None for k in ('id','team','priority','status','created_at')): raise ValueError('malformed CSV row')
            if row['id'] in ids: raise ValueError(f'duplicate ticket ID: {row["id"]}')
            ids.add(row['id'])
            if row['priority'] not in ('high','low'): raise ValueError(f'unknown_priority: {row["priority"]}')
            if row['priority'] not in policy: raise ValueError(f'missing_policy_entry: {row["priority"]}')
            if row['status'] not in ('open','closed'): raise ValueError(f'invalid status: {row["status"]}')
            created=parse_date(row['created_at'])
            limit=policy[row['priority']]
            age=int((as_of-created).total_seconds()//60)
            if age < 0: raise ValueError('ticket timestamp is after --as-of')
            out.append({'id':row['id'],'team':row['team'],'priority':row['priority'],'status':row['status'],'age_minutes':age,'limit_minutes':limit,'breached':row['status']!='closed' and age>limit})
        if args.team is not None: out=[x for x in out if x['team']==args.team]
        out.sort(key=lambda x:(x['team'],x['id']))
        json_write(args.output,{'as_of':as_of.isoformat().replace('+00:00','Z'),'tickets':out,'summary':{'total':len(out),'breached':sum(x['breached'] for x in out)}})
        return 0
    except Exception as e: return fail(str(e))


def jsonl(args) -> int:
    try:
        allowed=None if args.services is None else set(Path(args.services).read_text(encoding='utf-8').splitlines())
        groups={}; invalid=[]; ignored=0
        for line_no,line in enumerate(Path(args.events).read_text(encoding='utf-8').splitlines(),1):
            if not line.strip(): continue
            try:
                item=json.loads(line)
                service=item['service']; when=parse_date(item['at'])
                if not isinstance(service,str) or not service: raise ValueError('service must be non-empty text')
            except Exception as e:
                invalid.append({'line':line_no})
                if args.strict: return fail(f'invalid event at line {line_no}: {e}')
                continue
            if allowed is not None and service not in allowed:
                ignored+=1; continue
            key=(service,when.date().isoformat())
            groups.setdefault(key,[]).append(when)
        rows=[]
        for (service,date),values in sorted(groups.items()):
            rows.append({'service':service,'date':date,'count':len(values),'oldest':format_utc(min(values)),'newest':format_utc(max(values))})
        result={'groups':rows,'invalid':invalid,'ignored_count':ignored}
        json_write(args.output,result)
        return 0
    except Exception as e: return fail(str(e))


def semver(text: str):
    match=re.fullmatch(r'(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z.-]+))?(?:\+([0-9A-Za-z.-]+))?',text)
    if not match: raise ValueError(f'invalid semantic version: {text}')
    major,minor,patch,pre,build=match.groups()
    if pre is None: prerelease=(1,)
    else:
        parts=[]
        for part in pre.split('.'):
            if not part or (part.isdigit() and len(part)>1 and part[0]=='0'): raise ValueError(f'invalid semantic version: {text}')
            if part.isdigit(): parts.append((0,int(part)))
            else: parts.append((1,part))
        prerelease=(0,tuple(parts))
    if build is not None and any(not part for part in build.split('.')): raise ValueError(f'invalid semantic version: {text}')
    return int(major),int(minor),int(patch),prerelease


def escape_md(text):
    return ''.join('\\' + char if char in string.punctuation else char for char in str(text))


def release_notes(args) -> int:
    try:
        config=json.loads(Path(args.config).read_text(encoding='utf-8'))
        if not isinstance(config,dict) or set(config)!={'date','category_order'}: raise ValueError('config must contain exactly date and category_order')
        if not isinstance(config['date'],str) or not re.fullmatch(r'\d{4}-\d{2}-\d{2}',config['date']): raise ValueError('date must use YYYY-MM-DD')
        date=dt.date.fromisoformat(config['date']).isoformat()
        cat_order=config['category_order']
        if not isinstance(cat_order,list) or not cat_order or any(not isinstance(x,str) or not x or '\r' in x or '\n' in x for x in cat_order) or len(set(cat_order))!=len(cat_order): raise ValueError('category_order must contain unique non-empty single-line strings')
        since=None
        if args.since is not None:
            try: since=semver(args.since)
            except ValueError as error: raise ValueError(f'invalid --since version: {args.since}') from error
        versions={};seen_ids=set()
        for path in sorted(Path(args.changes).glob('*.json')):
            version=path.stem
            order=semver(version)
            data=json.loads(path.read_text(encoding='utf-8'))
            if not isinstance(data,dict): raise ValueError(f'{path}: invalid record field: record must be an object')
            missing=sorted({'id','category','text'}-set(data))
            extra=sorted(set(data)-{'id','category','text'})
            if missing: raise ValueError(f'{path}: missing required field: {missing[0]}')
            if extra: raise ValueError(f'{path}: unexpected field: {extra[0]}')
            for key in ('id','category','text'):
                if not isinstance(data[key],str) or not data[key]: raise ValueError(f'{path}: invalid field type or empty value: {key}')
            for key in ('category','text'):
                if any(char in data[key] for char in ('\r','\n')): raise ValueError(f'{path}: field {key} must be a single-line string')
            if data['id'] in seen_ids: raise ValueError(f'{path}: duplicate field id: {data["id"]}')
            seen_ids.add(data['id'])
            if data['category'] not in cat_order: raise ValueError(f'{path}: unconfigured category field: category ({data["category"]})')
            if since is not None and order<=since: continue
            entries=versions.setdefault((order,version),[])
            entries.append(data)
        ordered=sorted(versions.items(),key=lambda pair:pair[0][1])
        ordered.sort(key=lambda pair:pair[0][0],reverse=True)
        lines=[f'# Release notes ({date})']
        summary=[]
        for (order,version),entries in ordered:
            lines.extend(['',f'## {version}'])
            count=0
            for category in cat_order:
                selected=sorted((x for x in entries if x['category']==category),key=lambda x:x['id'])
                if selected:
                    lines.extend(['',f'### {category}'])
                    for item in selected:
                        lines.append('- '+escape_md(item['text']));count+=1
            summary.append({'version':version,'entry_count':count})
        markdown='\n'.join(lines)+'\n'
        # Validate both rendered products before publishing either file.
        json_data=json.dumps({'releases':summary},ensure_ascii=False,separators=(',',':'))+'\n'
        write_atomic(args.markdown,markdown)
        write_atomic(args.summary,json_data)
        return 0
    except Exception as e: return fail(str(e))


def calendar_report(args) -> int:
    try:
        from icalendar import Calendar
        sources=json.loads(Path(args.sources).read_text(encoding='utf-8'))['calendars']
        start=parse_date(args.window_start); end=parse_date(args.window_end)
        if start>=end: raise ValueError('window-start must be earlier than window-end')
        if isinstance(args.concurrency,bool) or args.concurrency<=0: raise ValueError('concurrency must be positive')
        if isinstance(args.timeout_ms,bool) or args.timeout_ms<=0: raise ValueError('timeout-ms must be positive')
        if not isinstance(sources,list): raise ValueError('calendars must be an array')
        names=[source.get('name') for source in sources if isinstance(source,dict)]
        if len(names)!=len(sources) or any(not isinstance(name,str) or not name for name in names) or len(set(names))!=len(names): raise ValueError('calendar names must be unique non-empty strings')
        if any(not isinstance(source.get('url'),str) or not source['url'] for source in sources): raise ValueError('calendar URLs must be non-empty strings')
        semaphore=threading.BoundedSemaphore(args.concurrency)
        def fetch(source):
            with semaphore:
                try:
                    req=urllib.request.Request(source['url'])
                    deadline=time.monotonic()+args.timeout_ms/1000
                    with urllib.request.urlopen(req,timeout=args.timeout_ms/1000) as response: body=read_response_with_deadline(response,deadline)
                    cal=Calendar.from_ical(body)
                    events=[]
                    for comp in cal.walk('VEVENT'):
                        status=str(comp.get('STATUS','CONFIRMED')).upper()
                        if status=='CANCELLED': continue
                        tentative=status=='TENTATIVE'
                        if tentative and not args.include_tentative: continue
                        st=comp.decoded('DTSTART'); en=comp.decoded('DTEND')
                        if isinstance(st,dt.date) and not isinstance(st,dt.datetime): continue
                        if isinstance(en,dt.date) and not isinstance(en,dt.datetime): continue
                        if not isinstance(st,dt.datetime) or not isinstance(en,dt.datetime) or en<=st: raise ValueError('invalid timed event interval')
                        if st.tzinfo is None or en.tzinfo is None: raise ValueError('floating local event times are invalid')
                        st=st.astimezone(dt.timezone.utc);en=en.astimezone(dt.timezone.utc)
                        uid=comp.decoded('UID')
                        if isinstance(uid,bytes): uid=uid.decode('utf-8')
                        if not isinstance(uid,str) or not uid.strip(): raise ValueError('event UID must be non-empty')
                        if st<end and en>start: events.append({'id':str(uid),'calendar':source['name'],'start':st,'end':en})
                    return source['name'],events,None
                except TimeoutError: return source['name'],[],'timeout'
                except urllib.error.URLError as e:
                    return source['name'],[],('timeout' if isinstance(getattr(e,'reason',None),TimeoutError) else 'http')
                except Exception: return source['name'],[],'invalid_calendar'
        with ThreadPoolExecutor(max_workers=args.concurrency) as pool: results=list(pool.map(fetch,sources))
        events=[event for _,rows,error in results for event in rows]
        errors=sorted(({'calendar':name,'category':error} for name,rows,error in results if error),key=lambda x:x['calendar'])
        conflicts=[]
        for i,a in enumerate(events):
            for b in events[i+1:]:
                if a['calendar']==b['calendar']: continue
                st=max(a['start'],b['start'],start); en=min(a['end'],b['end'],end)
                if st>=en: continue
                conflicts.append({'start':format_utc(st),'end':format_utc(en),'event_ids':sorted([a['id'],b['id']]),'calendars':sorted([a['calendar'],b['calendar']])})
        conflicts.sort(key=lambda x:(x['start'],x['end'],x['event_ids']))
        result={'conflicts':conflicts,'source_errors':errors}
        json_write(args.output,result)
        return 0
    except Exception as e: return fail(str(e))


def webhook(args) -> int:
    try:
        if args.command=='init':
            try: secret=Path(args.secret_file).read_bytes().decode('utf-8').strip(' \t\r\n\v\f')
            except UnicodeDecodeError:
                return fail('invalid_secret')
            if not secret: return fail('invalid_secret')
            Path(args.db).parent.mkdir(parents=True,exist_ok=True)
            with sqlite3.connect(args.db) as db:
                db.execute('CREATE TABLE IF NOT EXISTS events(id TEXT PRIMARY KEY, body BLOB NOT NULL, state TEXT NOT NULL, attempts INTEGER NOT NULL DEFAULT 0, category TEXT)')
            return 0
        if args.command=='ingest':
            body=Path(args.body).read_bytes()
            try:
                secret_text=Path(args.secret_file).read_bytes().decode('utf-8')
            except UnicodeDecodeError:
                json_write(args.result,{'accepted':False,'reason':'invalid_secret'});return 1
            secret=secret_text.strip(' \t\r\n\v\f').encode('utf-8')
            if not secret:
                json_write(args.result,{'accepted':False,'reason':'invalid_secret'});return 1
            if not re.fullmatch(r'[0-9A-Fa-f]{64}',args.signature or ''):
                json_write(args.result,{'accepted':False,'reason':'invalid_signature'});return 1
            signature=hmac.new(secret,body,hashlib.sha256).digest()
            if not hmac.compare_digest(signature,bytes.fromhex(args.signature)):
                json_write(args.result,{'accepted':False,'reason':'invalid_signature'});return 1
            try:
                event=json.loads(body)
                if not isinstance(event,dict) or not isinstance(event.get('id'),str) or not event['id'] or 'payload' not in event: raise ValueError()
            except Exception:
                json_write(args.result,{'accepted':False,'reason':'invalid_body'});return 1
            with sqlite3.connect(args.db) as db:
                try: db.execute('INSERT INTO events(id,body,state) VALUES(?,?,?)',(event['id'],body,'pending'))
                except sqlite3.IntegrityError:
                    json_write(args.result,{'accepted':False,'reason':'duplicate_id'});return 1
            json_write(args.result,{'accepted':True,'id':event['id']});return 0
        if args.command=='status':
            with sqlite3.connect(args.db) as db:
                pending=[x[0] for x in db.execute("SELECT id FROM events WHERE state='pending' ORDER BY id")]
                delivered=[x[0] for x in db.execute("SELECT id FROM events WHERE state='delivered' ORDER BY id")]
            json_write(args.output,{'pending':pending,'delivered':delivered});return 0
        if args.command=='deliver':
            if args.max_attempts is not None and args.max_attempts<=0: return fail('--max-attempts must be positive')
            with sqlite3.connect(args.db) as db:
                db.row_factory=sqlite3.Row
                rows=db.execute("SELECT * FROM events WHERE state='pending' ORDER BY id").fetchall()
                failed=[];dead=[];delivered=[]
                for row in rows:
                    if args.max_attempts is not None and row['attempts']>=args.max_attempts:
                        db.execute("UPDATE events SET state='dead',category='http_failure' WHERE id=?",(row['id'],))
                        failed.append(row['id']);dead.append(row['id']);continue
                    try:
                        req=urllib.request.Request(args.target,data=row['body'],headers={'Content-Type':'application/json'},method='POST')
                        with urllib.request.urlopen(req,timeout=3) as response: code=response.status
                        ok=200<=code<300
                    except urllib.error.HTTPError as e: ok=False
                    except Exception: ok=False
                    attempts=row['attempts']+1
                    if ok: db.execute("UPDATE events SET state='delivered',attempts=? WHERE id=?",(attempts,row['id']));delivered.append(row['id'])
                    elif args.max_attempts is not None and attempts>=args.max_attempts:
                        db.execute("UPDATE events SET state='dead',attempts=?,category='http_failure' WHERE id=?",(attempts,row['id']))
                        failed.append(row['id']);dead.append(row['id'])
                    else: db.execute("UPDATE events SET attempts=? WHERE id=?",(attempts,row['id']));failed.append(row['id'])
            result={'pending':[x for x in failed if x not in dead],'delivered':delivered}
            if args.max_attempts is not None: result['dead_lettered']=dead
            json_write(args.result,result);return 1 if failed else 0
        if args.command=='dead-letters':
            with sqlite3.connect(args.db) as db:
                rows=db.execute("SELECT id,category,attempts FROM events WHERE state='dead' ORDER BY id").fetchall()
            json_write(args.output,{'dead_letters':[{'id':r[0],'category':r[1],'attempts':r[2]} for r in rows]});return 0
        return 2
    except Exception as e: return fail(str(e))


def health(args) -> int:
    try:
        config=json.loads(Path(args.config).read_text(encoding='utf-8'))
        if not isinstance(config,dict) or not isinstance(config.get('services'),list): raise ValueError('services must be an array')
        services=config['services']; now=parse_date(args.now) if args.now else None
        names=[]
        for service in services:
            if not isinstance(service,dict) or not isinstance(service.get('name'),str) or not service['name'] or not isinstance(service.get('url'),str) or not service['url']: raise ValueError('each service needs a non-empty name and URL')
            names.append(service['name'])
            if 'maintenance' in service:
                maintenance=service['maintenance']
                if not isinstance(maintenance,dict) or set(maintenance)!={'start','end'}: raise ValueError('maintenance must contain start and end')
                a=parse_date(maintenance['start']);b=parse_date(maintenance['end'])
                if a>b: raise ValueError('maintenance start must not be after end')
        if len(set(names))!=len(names): raise ValueError('service names must be unique')
        if any('maintenance' in service for service in services) and now is None: raise ValueError('--now is required when maintenance is configured')
        def get(service):
            raw=None
            try:
                deadline=time.monotonic()+args.timeout_ms/1000
                with urllib.request.urlopen(service['url'],timeout=args.timeout_ms/1000) as response:
                    body=read_response_with_deadline(response,deadline)
                    if response.status<200 or response.status>=300: status='unhealthy'; raw=None
                    else:
                        try:
                            data=json.loads(body)
                            if not isinstance(data,dict) or not isinstance(data.get('status'),str): status='invalid_response'; raw=None
                            else:
                                raw=data['status']; status='healthy' if raw=='ok' else 'unhealthy'
                        except Exception: status='invalid_response'; raw=None
            except (TimeoutError,TimeoutError): status='timeout'
            except urllib.error.URLError as e:
                status='timeout' if isinstance(getattr(e,'reason',None),TimeoutError) else 'unhealthy'
            except Exception: status='unhealthy'; raw=None
            if now is not None and 'maintenance' in service:
                maint=service['maintenance'];a=parse_date(maint['start']);b=parse_date(maint['end'])
                if a<=now<=b and status not in ('healthy','timeout'): status='maintenance'
            return service['name'],status,raw
        with ThreadPoolExecutor(max_workers=args.concurrency) as pool: rows=list(pool.map(get,services))
        rows.sort(key=lambda x:x[0])
        result={'results':[{'service':name,'status':status,'raw_status':raw} for name,status,raw in rows]}
        json_write(args.json,result)
        write_atomic(args.text,''.join(f'{name} {status}\n' for name,status,raw in rows))
        return 0
    except Exception as e: return fail(str(e))


def config_migrate(args) -> int:
    try:
        path=Path(args.file); text=path.read_text(encoding='utf-8'); data=tomllib.loads(text);version=data.get('version')
        if isinstance(version,bool) or not isinstance(version,int): return fail('version must be an integer')
        if version<1 or version>3: return fail(f'unsupported schema version {version}')
        if not isinstance(data.get('name'),str) or not data['name']: return fail('name must be non-empty text')
        sync=data.get('sync',{})
        if not isinstance(sync,dict): return fail('sync must be a table')
        if 'timeout_seconds' in sync and (isinstance(sync['timeout_seconds'],bool) or not isinstance(sync['timeout_seconds'],int) or sync['timeout_seconds']<=0): return fail('invalid sync timeout_seconds')
        if 'parallelism' in sync and (isinstance(sync['parallelism'],bool) or not isinstance(sync['parallelism'],int) or sync['parallelism']<=0): return fail('invalid sync parallelism')
        if version>=2 and 'timeout_seconds' not in sync: return fail('schema version 2 or later requires sync.timeout_seconds')
        if version>=3 and 'parallelism' not in sync: return fail('schema version 3 requires sync.parallelism')
        if args.command=='check' or version==3: return 0
        lines=text.splitlines();
        for i,line in enumerate(lines):
            match=re.match(r'^(\s*version\s*=\s*)[-+]?\d+(\s*(?:#.*)?)$',line)
            if match: lines[i]=f'{match.group(1)}{version+1}{match.group(2)}';break
        def add_sync_default(key,value):
            nonlocal lines
            section=next((i for i,line in enumerate(lines) if line.strip()=='[sync]'),None)
            if section is None:
                lines += ['[sync]',f'{key} = {value}'];return
            end=next((i for i in range(section+1,len(lines)) if re.match(r'^\s*\[',lines[i])),len(lines))
            if not any(re.match(rf'^\s*{re.escape(key)}\s*=',line) for line in lines[section+1:end]):
                lines.insert(end,f'{key} = {value}')
        if version==1: add_sync_default('timeout_seconds',30)
        else: add_sync_default('parallelism',4)
        write_atomic(args.file,'\n'.join(lines)+'\n');return 0
    except Exception as e:return fail(str(e))


def norm_title(title): return ' '.join(title.casefold().split())

def path_sort_key(path): return norm_title(path), path.encode('utf-8')

def yaml_flow_strings(text):
    """Parse one JSON array of strings from the task's single-line aliases field."""
    values=json.loads(text)
    if not isinstance(values,list) or any(not isinstance(value,str) for value in values):
        raise ValueError('aliases must be a JSON array of strings')
    return values

def front_matter_parts(text):
    lines=text.splitlines()
    if not lines or lines[0] != '---':
        return text, []
    try:
        end=next(i for i in range(1,len(lines)) if lines[i]=='---')
    except StopIteration:
        return text, []
    fields=[line for line in lines[1:end] if re.match(r'^\s*aliases\s*:',line)]
    if len(fields)>1: raise ValueError('duplicate aliases field')
    if not fields: return '\n'.join(lines[end+1:]), []
    match=re.fullmatch(r'\s*aliases\s*:\s*(\[[^\n]*\])\s*',fields[0])
    if not match: raise ValueError('aliases must be a single-line JSON array of strings')
    try:
        aliases=yaml_flow_strings(match.group(1))
        return '\n'.join(lines[end+1:]), aliases
    except (ValueError, json.JSONDecodeError) as error:
        raise ValueError('aliases must be a JSON array of strings') from error

def front_matter_aliases(text):
    return front_matter_parts(text)[1]

def doc_index(args) -> int:
    try:
        if args.command=='build':
            root=Path(args.root).resolve()
            docs=[]
            for base,dirs,files in os.walk(root,followlinks=False):
                dirs[:]=[d for d in dirs if not (Path(base)/d).is_symlink()]
                for filename in files:
                    path=Path(base)/filename
                    if path.suffix.lower()!='.md' or path.is_symlink(): continue
                    text=path.read_text(encoding='utf-8')
                    rel=path.relative_to(root).as_posix()
                    body,aliases=front_matter_parts(text)
                    title=next((line[2:].strip() for line in body.splitlines() if line.startswith('# ')),path.stem)
                    links=re.findall(r'\[\[([^\]]+)\]\]',body)
                    docs.append((rel,title,aliases,links))
            docs.sort(key=lambda doc:path_sort_key(doc[0]))
            db=sqlite3.connect(args.database)
            db.executescript('CREATE TABLE IF NOT EXISTS documents(path TEXT PRIMARY KEY,title TEXT NOT NULL,aliases TEXT NOT NULL); CREATE TABLE IF NOT EXISTS links(source TEXT NOT NULL,target TEXT NOT NULL);')
            with db:
                db.execute('DELETE FROM documents');db.execute('DELETE FROM links')
                for rel,title,aliases,links in docs:
                    db.execute('INSERT INTO documents VALUES(?,?,?)',(rel,title,json.dumps(aliases,ensure_ascii=False)))
                    for link in links: db.execute('INSERT INTO links VALUES(?,?)',(rel,link))
            db.close();return 0
        db=sqlite3.connect(args.database);db.row_factory=sqlite3.Row
        docs=db.execute('SELECT * FROM documents').fetchall()
        choices={}
        for d in docs:
            for title in [d['title'],*json.loads(d['aliases'])]:choices.setdefault(norm_title(title),set()).add(d['path'])
        if args.command=='check':
            missing=[];amb=[]
            issues=list(db.execute('SELECT source,target FROM links'))
            issues.sort(key=lambda l:(path_sort_key(l['source']),norm_title(l['target']),l['target'].encode('utf-8')))
            for l in issues:
                matches=choices.get(norm_title(l['target']),[])
                if not matches:missing.append({'source':l['source'],'target':l['target']})
                elif len(matches)>1:amb.append({'source':l['source'],'target':l['target']})
            json_write(args.json,{'missing':missing,'ambiguous':amb});db.close();return 0
        target=norm_title(args.title);matches=choices.get(target,[])
        result=sorted({l['source'] for l in db.execute('SELECT source,target FROM links') if norm_title(l['target'])==target},key=path_sort_key)
        json_write(args.json,{'backlinks':result});db.close();return 0
    except Exception as e:return fail(str(e))


def lockfile(args) -> int:
    try:
        lock=json.loads(Path(args.lockfile).read_text(encoding='utf-8'));policy=json.loads(Path(args.policy).read_text(encoding='utf-8'))
        if not isinstance(lock,dict) or not isinstance(lock.get('packages'),list): raise ValueError('lockfile must contain a packages array')
        for pkg in lock['packages']:
            if not isinstance(pkg,dict) or not isinstance(pkg.get('name'),str) or not pkg['name'] or not isinstance(pkg.get('version'),str): raise ValueError('each lock entry requires non-empty name and version')
            semver(pkg['version'])
            for key in ('source','checksum'):
                if pkg.get(key) is not None and not isinstance(pkg[key],str): raise ValueError(f'{key} must be a string or null')
        if not isinstance(policy,dict): raise ValueError('policy must be an object')
        if not isinstance(policy.get('deny',[]),list) or not all(isinstance(x,str) for x in policy.get('deny',[])): raise ValueError('deny must be a string array')
        for key in ('minimum_versions','maximum_versions'):
            mapping=policy.get(key,{})
            if not isinstance(mapping,dict) or not all(isinstance(k,str) and isinstance(v,str) for k,v in mapping.items()): raise ValueError(f'{key} must map names to versions')
            for value in mapping.values(): semver(value)
        for key in ('require_source','require_checksum'):
            if key in policy and not isinstance(policy[key],bool): raise ValueError(f'{key} must be Boolean')
        if args.as_of is not None:
            if not re.fullmatch(r'\d{4}-\d{2}-\d{2}', args.as_of): raise ValueError('invalid --as-of date')
            try: asof=dt.date.fromisoformat(args.as_of)
            except ValueError as error: raise ValueError('invalid --as-of date') from error
        else: asof=None
        exceptions=policy.get('exceptions',[])
        if not isinstance(exceptions,list): raise ValueError('exceptions must be an array')
        if exceptions and not args.as_of: raise ValueError('--as-of is required when exceptions are configured')
        exception_map={}
        for exc in exceptions:
            if not isinstance(exc,dict) or set(exc)!={'name','expires'} or not isinstance(exc.get('name'),str) or not exc['name'] or not isinstance(exc.get('expires'),str) or not re.fullmatch(r'\d{4}-\d{2}-\d{2}',exc['expires']): raise ValueError('invalid exception entry')
            if exc['name'] in exception_map: raise ValueError('duplicate exception name')
            try: exception_map[exc['name']]=dt.date.fromisoformat(exc['expires'])
            except ValueError as error: raise ValueError('invalid exception date') from error
        violations=[];warnings=[]
        for pkg in lock['packages']:
            name=pkg['name'];version=pkg['version'];source=pkg.get('source');checksum=pkg.get('checksum')
            exc_expiry=exception_map.get(name) if asof else None
            expired=bool(name in policy.get('deny',[]) and exc_expiry and asof>=exc_expiry)
            if name in policy.get('deny',[]):
                if exc_expiry and not expired: pass
                else: violations.append({'name':name,'rule':'denied'})
            if name in policy.get('minimum_versions',{}) and semver(version)<semver(policy['minimum_versions'][name]):violations.append({'name':name,'rule':'below_minimum'})
            if name in policy.get('maximum_versions',{}) and semver(version)>semver(policy['maximum_versions'][name]):violations.append({'name':name,'rule':'above_maximum'})
            if policy.get('require_source') and not source:violations.append({'name':name,'rule':'missing_source'})
            if policy.get('require_checksum') and not checksum:violations.append({'name':name,'rule':'missing_checksum'})
            if expired:warnings.append({'name':name,'rule':'expired_exception'})
        violations.sort(key=lambda x:(x['name'],x['rule']));warnings.sort(key=lambda x:(x['name'],x['rule']))
        json_write(args.output,{'violations':violations,'warnings':warnings});return 0
    except Exception as e:return fail(str(e))


def build_parser():
    p=argparse.ArgumentParser();p.add_argument('--tickets');p.add_argument('--policy');p.add_argument('--as-of');p.add_argument('--output');p.add_argument('--team')
    p.add_argument('--events');p.add_argument('--strict',action='store_true');p.add_argument('--services')
    p.add_argument('--changes');p.add_argument('--config');p.add_argument('--markdown');p.add_argument('--summary');p.add_argument('--since')
    p.add_argument('--sources');p.add_argument('--window-start');p.add_argument('--window-end');p.add_argument('--concurrency',type=int,default=4);p.add_argument('--timeout-ms',type=int,default=1000);p.add_argument('--include-tentative',action='store_true')
    p.add_argument('--json');p.add_argument('--text');p.add_argument('--now')
    p.add_argument('--lockfile')
    sub=p.add_subparsers(dest='command')
    for name in ('init','ingest','deliver','status','dead-letters','build','check','backlinks','migrate'):
        q=sub.add_parser(name)
        for arg in ('db','secret-file','body','signature','result','target','max-attempts','output','file','root','database','json'):
            flag='--'+arg
            if arg=='max-attempts': q.add_argument(flag,type=int)
            else: q.add_argument(flag)
        if name=='backlinks': q.add_argument('title')
    return p


def main(argv=None):
    args=build_parser().parse_args(argv)
    if args.tickets:return sla(args)
    if args.events:return jsonl(args)
    if args.changes:return release_notes(args)
    if args.sources:return calendar_report(args)
    if args.lockfile:return lockfile(args)
    if args.command in ('init','ingest','deliver','status','dead-letters'):return webhook(args)
    if args.command in ('build','check','backlinks') and (args.root is not None or args.database is not None):return doc_index(args)
    if args.command in ('check','migrate') and args.file is not None:return config_migrate(args)
    if args.config:return health(args)
    return fail('unrecognized command')

if __name__=='__main__':raise SystemExit(main())
