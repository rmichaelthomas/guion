import json, pathlib, subprocess, time, sys
root = pathlib.Path(__file__).parent
project = pathlib.Path(sys.argv[1])
mode = sys.argv[2]
work = root / ('checks-' + mode)
work.mkdir(exist_ok=True)
ledger = 'invoice_id\tvendor\tamount_cents\tdue_date\tstate\nA1\tAcme\t100\t2026-01-01\topen\nB1\tBeta\t250\t2026-01-10\topen\nA2\tAcme\t300\t2026-02-01\topen\nB2\tBeta\t999\t2025-12-01\tpaid\nU1\tCafé 雪\t9007199254740993\t2026-01-14\topen\nX1\tAcme\tbad\t2026-01-01\topen\nX2\tBeta\t10\t2026-02-30\topen\nmalformed\nX3\tBeta\t10\t2026-01-01\tunknown\nA3\tAcme\t50\t2026-01-15\topen\n'
(work/'ledger.tsv').write_text(ledger)
logs=[]
def run(extra=(), date='2026-01-15', output='report.json'):
    cmd=[str(project/'run'),'--input','ledger.tsv','--as-of',date,'--output',output,*extra]
    start=time.perf_counter(); p=subprocess.run(cmd,cwd=work,capture_output=True,text=True); elapsed=time.perf_counter()-start
    logs.append(dict(command=cmd,cwd=str(work),returncode=p.returncode,stdout=p.stdout,stderr=p.stderr,elapsed_seconds=elapsed))
    return p
results={}
try:
    assert run().returncode == 0
    data=json.loads((work/'report.json').read_text())
    expected={'Acme':{'open_cents':450,'overdue_open_cents':100,'overdue_open_count':1},'Beta':{'open_cents':250,'overdue_open_cents':250,'overdue_open_count':1},'Café 雪':{'open_cents':9007199254740993,'overdue_open_cents':9007199254740993,'overdue_open_count':1}}
    assert data['vendors']==expected, data
    assert data['rejected_row_count']==4
    assert [r['line'] for r in data['rejected_rows']]==[7,8,9,10]
    assert all(r['reason'] for r in data['rejected_rows'])
    results['multiple_vendors_states_dates_malformed_exact_integer']=True
    original=(work/'report.json').read_bytes()
    assert run().returncode==0 and (work/'report.json').read_bytes()==original
    results['deterministic_repeat']=True
    for bad in ['2026-02-30','2026-13-01','2026-1-01','0000-01-01','bad']:
        assert run(date=bad).returncode!=0
        assert (work/'report.json').read_bytes()==original
    assert run(['--amount','1.5']).returncode!=0
    assert (work/'report.json').read_bytes()==original
    results['invalid_arguments_preserve_output']=True
    if mode != 'initial' and mode != 'relocated-initial':
        assert run(['--vendor','Acme']).returncode==0
        filtered=json.loads((work/'report.json').read_text())
        assert filtered['vendors']=={'Acme':expected['Acme']},filtered
        assert filtered['excluded_valid_row_count']==3,filtered
        assert filtered['rejected_row_count']==4
        results['exact_vendor']=True
        assert run(['--vendor','acme']).returncode==0
        missing=json.loads((work/'report.json').read_text())
        assert missing['vendors']=={} and missing['excluded_valid_row_count']==6
        results['absent_exact_filter']=True
        assert run(['--vendor','Café 雪']).returncode==0
        assert json.loads((work/'report.json').read_text())['vendors']=={'Café 雪':expected['Café 雪']}
        results['unicode_vendor']=True
    results['passed']=True
except Exception as e:
    results['passed']=False;results['error']=repr(e)
finally:
    (root/('commands-'+mode+'.json')).write_text(json.dumps(logs,indent=2,ensure_ascii=False))
    (root/('results-'+mode+'.json')).write_text(json.dumps(results,indent=2))
    print(json.dumps(results))
    if not results['passed']:sys.exit(1)
