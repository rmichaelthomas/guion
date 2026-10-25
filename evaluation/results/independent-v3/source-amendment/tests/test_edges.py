import json, pathlib, subprocess, tempfile, unittest
import test_cli as base
COMMAND, ROOT, HEADER = base.COMMAND, base.ROOT, base.HEADER

class Edges(unittest.TestCase):
    invoke = base.CLI.invoke
    def test_policy_shapes_values(self):
        row='x,A,high,open,2026-01-02T11:00:00Z\n'
        for policy in [[],{}, {'response_minutes':[]}, {'response_minutes':{'high':-1}}, {'response_minutes':{'high':1.0}}, {'response_minutes':{'high':'60'}}, {'response_minutes':{'high':None}}]:
            with self.subTest(policy=policy):
                p,s=self.invoke(row,policy)
                self.assertNotEqual(p.returncode,0)
                self.assertIn('invalid_policy',p.stderr)
                self.assertEqual(s,'SENTINEL')

    def test_rows_future_and_empty(self):
        for row in ['x,A,high,open\n','x,A,high,open,2026-01-02T11:00:00Z,extra\n',',A,high,open,2026-01-02T11:00:00Z\n','x,A,high,open,2026-01-02T12:00:00.000001Z\n','"unterminated,A,high,open,2026-01-02T11:00:00Z\n']:
            with self.subTest(row=row):
                p,s=self.invoke(row)
                self.assertNotEqual(p.returncode,0)
                self.assertEqual(s,'SENTINEL')
        p,s=self.invoke('')
        self.assertEqual(p.returncode,0,p.stderr)
        self.assertEqual(json.loads(s)['summary'],{'total':0,'breached':0})

    def test_csv_unicode_and_quoted_fields(self):
        p,s=self.invoke('"a,1",Équipe,high,open,2026-01-02T10:59:00Z\n')
        self.assertEqual(p.returncode,0,p.stderr)
        self.assertEqual(json.loads(s)['tickets'][0]['id'],'a,1')
        self.assertEqual(json.loads(s)['tickets'][0]['team'],'Équipe')

    def test_floor_exact_and_zero_limit(self):
        p,s=self.invoke('a,A,high,open,2026-01-02T10:59:00.000001Z\n')
        self.assertEqual(p.returncode,0,p.stderr)
        self.assertEqual(json.loads(s)['tickets'][0]['age_minutes'],60)
        self.assertFalse(json.loads(s)['tickets'][0]['breached'])
        p,s=self.invoke('a,A,high,open,2026-01-02T11:59:00Z\n',{'response_minutes':{'high':0}})
        self.assertEqual(p.returncode,0,p.stderr)
        self.assertTrue(json.loads(s)['tickets'][0]['breached'])

    def test_raw_malformed_files_and_help(self):
        with tempfile.TemporaryDirectory(dir=ROOT/'work') as tmp:
            d=pathlib.Path(tmp); out=d/'out'; out.write_bytes(b'unchanged\x00')
            tickets=d/'tickets'; policy=d/'policy'
            tickets.write_text(HEADER)
            for raw in ['{','{"response_minutes":{"high":1,"high":2}}','{"response_minutes":{"high":NaN}}']:
                policy.write_text(raw)
                p=subprocess.run(COMMAND+['--tickets',str(tickets),'--policy',str(policy),'--as-of','2026-01-02T12:00:00Z','--output',str(out)],capture_output=True,text=True)
                self.assertNotEqual(p.returncode,0)
                self.assertIn('invalid_policy',p.stderr)
                self.assertEqual(out.read_bytes(),b'unchanged\x00')
            policy.write_text('{"response_minutes":{}}')
            tickets.write_text('team,id,priority,status,created_at\n')
            p=subprocess.run(COMMAND+['--tickets',str(tickets),'--policy',str(policy),'--as-of','2026-01-02T12:00:00Z','--output',str(out)],capture_output=True,text=True)
            self.assertNotEqual(p.returncode,0)
            self.assertIn('malformed_header',p.stderr)
            self.assertEqual(out.read_bytes(),b'unchanged\x00')
        p=subprocess.run(COMMAND+['--help'],capture_output=True,text=True)
        self.assertEqual(p.returncode,0,p.stderr)
        self.assertIn('sla-report',p.stdout)

if __name__=='__main__': unittest.main()
