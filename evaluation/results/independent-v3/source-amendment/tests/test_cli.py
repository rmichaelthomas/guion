import json, os, pathlib, subprocess, tempfile, unittest

ROOT = pathlib.Path(os.environ.get('SLA_TEST_ROOT', pathlib.Path(__file__).resolve().parents[1]))
COMMAND = json.loads(os.environ.get('SLA_COMMAND', '["/opt/veritas/veritas", "run", "/candidate"]'))
HEADER = 'id,team,priority,status,created_at\n'

class CLI(unittest.TestCase):
    def invoke(self, rows, policy=None, as_of='2026-01-02T12:00:00Z'):
        self.temp = tempfile.TemporaryDirectory(dir=ROOT / 'work')
        d = pathlib.Path(self.temp.name)
        (d/'tickets.csv').write_text(HEADER + rows, encoding='utf-8')
        (d/'policy.json').write_text(json.dumps(policy if policy is not None else {'response_minutes':{'high':60,'low':120}}), encoding='utf-8')
        out = d/'report.json'
        out.write_text('SENTINEL', encoding='utf-8')
        p = subprocess.run(COMMAND + ['--tickets',str(d/'tickets.csv'),'--policy',str(d/'policy.json'),'--as-of',as_of,'--output',str(out)], capture_output=True,text=True)
        content = out.read_text(encoding='utf-8')
        self.temp.cleanup()
        return p, content

    def test_threshold_closed_sort_and_shape(self):
        p, s = self.invoke('b,Z,high,open,2026-01-02T10:59:00Z\na,A,high,open,2026-01-02T11:00:00Z\nc,A,low,closed,2026-01-01T12:00:00Z\n')
        self.assertEqual(p.returncode,0,p.stderr)
        self.assertEqual(json.loads(s), {'as_of':'2026-01-02T12:00:00Z','tickets':[
            {'id':'a','team':'A','priority':'high','status':'open','age_minutes':60,'limit_minutes':60,'breached':False},
            {'id':'c','team':'A','priority':'low','status':'closed','age_minutes':1440,'limit_minutes':120,'breached':False},
            {'id':'b','team':'Z','priority':'high','status':'open','age_minutes':61,'limit_minutes':60,'breached':True}], 'summary':{'total':3,'breached':1}})

    def test_rejections_preserve_output(self):
        good = 'x,A,high,open,2026-01-02T11:00:00Z\n'
        cases = [(good+good,None,'duplicate_id'), ('x,A,urgent,open,2026-01-02T11:00:00Z\n',None,'unknown_priority'),
                 (good,{'response_minutes':{}},'missing_policy_entry'), (good,{'response_minutes':{'high':True}},'invalid_policy'),
                 (good,{'response_minutes':{'other':1}},'invalid_policy'), ('\n',None,'malformed_row'),
                 ('x,A,high,pending,2026-01-02T11:00:00Z\n',None,'unknown_status'),
                 ('x,A,high,open,2026-02-30T00:00:00Z\n',None,'invalid_timestamp')]
        for rows, policy, code in cases:
            with self.subTest(code=code):
                p,s = self.invoke(rows,policy)
                self.assertNotEqual(p.returncode,0)
                self.assertIn(code,p.stderr)
                self.assertEqual(s,'SENTINEL')

    def test_offsets_and_fraction(self):
        p,s = self.invoke('x,A,high,open,2026-01-02T12:59:59.999999+02:00\n',as_of='2026-01-02T12:00:00.000000+01:00')
        self.assertEqual(p.returncode,0,p.stderr)
        self.assertEqual(json.loads(s)['tickets'][0]['age_minutes'],0)

    def test_invalid_timestamp_syntax(self):
        for ts in ['2026-01-02 12:00:00Z','2026-01-02T12:00:00','2026-01-02T12:00:60Z','2026-01-02T12:00:00.1234567Z','2026-01-02T12:00:00+24:00','2026-01-02T12:00:00z']:
            with self.subTest(ts=ts):
                p,s = self.invoke('',as_of=ts)
                self.assertNotEqual(p.returncode,0)
                self.assertIn('invalid_timestamp',p.stderr)
                self.assertEqual(s,'SENTINEL')

if __name__ == '__main__': unittest.main()
