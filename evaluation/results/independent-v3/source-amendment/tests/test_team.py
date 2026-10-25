import json,pathlib,subprocess,tempfile,unittest
import test_cli as base

class TeamFilter(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(dir=base.ROOT/'work')
        self.addCleanup(self.temp.cleanup)
        self.root=pathlib.Path(self.temp.name)
        self.tickets=self.root/'tickets.csv'; self.policy=self.root/'policy.json'; self.output=self.root/'report.json'
        self.tickets.write_text(base.HEADER+'b,Support North,high,open,2026-01-02T10:59:00Z\nz,Other,high,open,2026-01-02T10:00:00Z\na,Support North,low,closed,2026-01-01T10:00:00Z\n',encoding='utf-8')
        self.policy.write_text('{"response_minutes":{"high":60,"low":120}}')
        self.args=['--tickets',str(self.tickets),'--policy',str(self.policy),'--as-of','2026-01-02T12:00:00Z','--output',str(self.output)]

    def run_report(self,extra):
        self.output.write_text('SENTINEL')
        p=subprocess.run(base.COMMAND+self.args+extra,capture_output=True,text=True)
        return p,self.output.read_text()

    def test_match_filters_array_and_summary_preserving_default(self):
        p,full=self.run_report([]);self.assertEqual(p.returncode,0,p.stderr)
        p,filtered=self.run_report(['--team','Support North']);self.assertEqual(p.returncode,0,p.stderr)
        expected=json.loads(full)
        expected['tickets']=[t for t in expected['tickets'] if t['team']=='Support North']
        expected['summary']={'total':2,'breached':1}
        self.assertEqual(json.loads(filtered),expected)
        self.assertEqual([t['id'] for t in expected['tickets']],['a','b'])
        p,again=self.run_report([]);self.assertEqual(p.returncode,0,p.stderr)
        self.assertEqual(again,full)

    def test_miss_and_case_sensitive(self):
        for team in ['missing','support north']:
            p,s=self.run_report(['--team',team]);self.assertEqual(p.returncode,0,p.stderr)
            data=json.loads(s)
            self.assertEqual(data['tickets'],[])
            self.assertEqual(data['summary'],{'total':0,'breached':0})

    def test_invalid_team_arguments_preserve_output(self):
        for extra in [['--team'],['--team',''],['--team','Other','--team','Support North']]:
            p,s=self.run_report(extra)
            self.assertNotEqual(p.returncode,0)
            self.assertIn('usage_error',p.stderr)
            self.assertEqual(s,'SENTINEL')

    def test_unselected_malformed_ticket_still_rejected(self):
        with self.tickets.open('a') as f:f.write('bad,Other,urgent,open,2026-01-02T10:00:00Z\n')
        p,s=self.run_report(['--team','Support North'])
        self.assertNotEqual(p.returncode,0)
        self.assertIn('unknown_priority',p.stderr)
        self.assertEqual(s,'SENTINEL')

if __name__=='__main__':unittest.main()
