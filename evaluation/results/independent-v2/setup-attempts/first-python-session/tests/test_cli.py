import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
AS_OF = '2026-01-01T12:00:00Z'
HEADER = 'id,team,priority,status,created_at\n'


class CLITest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.tickets = self.root / 'tickets.csv'
        self.policy = self.root / 'policy.json'
        self.output = self.root / 'result.json'

    def run_cli(self, csv_text=HEADER, policy=None, as_of=AS_OF):
        self.tickets.write_text(csv_text, encoding='utf-8')
        self.policy.write_text(policy if policy is not None else
                               '{"response_minutes":{"high":60,"low":120}}', encoding='utf-8')
        args = [sys.executable]
        artifact = os.environ.get('SLA_REPORT_ARTIFACT')
        args += [artifact] if artifact else ['-m', 'sla_report']
        args += ['--tickets', str(self.tickets), '--policy', str(self.policy),
                 '--as-of', as_of, '--output', str(self.output)]
        env = os.environ.copy()
        env.pop('PYTHONPATH', None)
        if not artifact:
            env['PYTHONPATH'] = str(ROOT / 'src')
        return subprocess.run(args, cwd=self.root, env=env, capture_output=True, text=True)

    def rejected(self, csv_text=HEADER, policy=None, as_of=AS_OF, code=None):
        self.output.write_bytes(b'original output\x00')
        p = self.run_cli(csv_text, policy, as_of)
        self.assertNotEqual(p.returncode, 0, p.stdout)
        self.assertEqual(self.output.read_bytes(), b'original output\x00')
        if code:
            self.assertIn(code, p.stderr)
        self.assertNotIn('Traceback', p.stderr)

    def test_exact_shape_order_threshold_and_closed(self):
        p = self.run_cli(HEADER +
            'z,Zeta,low,closed,2025-12-31T12:00:00Z\n'
            'b,Alpha,high,open,2026-01-01T10:59:00Z\n'
            'a,Alpha,high,open,2026-01-01T11:00:00Z\n')
        self.assertEqual(p.returncode, 0, p.stderr)
        self.assertEqual(json.loads(self.output.read_text()), {
            'as_of': AS_OF,
            'tickets': [
                {'id':'a','team':'Alpha','priority':'high','status':'open',
                 'age_minutes':60,'limit_minutes':60,'breached':False},
                {'id':'b','team':'Alpha','priority':'high','status':'open',
                 'age_minutes':61,'limit_minutes':60,'breached':True},
                {'id':'z','team':'Zeta','priority':'low','status':'closed',
                 'age_minutes':1440,'limit_minutes':120,'breached':False}],
            'summary': {'total':3, 'breached':1}})

    def test_empty_input(self):
        p = self.run_cli()
        self.assertEqual(p.returncode, 0, p.stderr)
        self.assertEqual(json.loads(self.output.read_text()),
                         {'as_of':AS_OF, 'tickets':[], 'summary':{'total':0,'breached':0}})

    def test_offset_fraction_floor_and_unicode(self):
        p = self.run_cli(HEADER+'1,Équipe,high,open,2026-01-01T12:59:00.000002+01:00\n',
                         as_of='2026-01-01T07:00:00.000001-05:00')
        self.assertEqual(p.returncode, 0, p.stderr)
        result = json.loads(self.output.read_text(encoding='utf-8'))
        self.assertEqual(result['as_of'], '2026-01-01T12:00:00.000001Z')
        self.assertEqual(result['tickets'][0]['age_minutes'], 0)
        self.assertEqual(result['tickets'][0]['team'], 'Équipe')

    def test_bad_timestamps_in_both_positions(self):
        values = ['2026-02-29T00:00:00Z','2026-01-01T12:00:60Z',
                  '2026-01-01 12:00:00Z','2026-01-01T12:00:00',
                  '2026-01-01T12:00:00.1234567Z','2026-01-01T12:00:00.Z',
                  '2026-01-01T12:00:00+24:00','2026-01-01T12:00:00+00:60',
                  '2026-01-01T12:00:00z','2026-01-01T12:00:00Z\n',
                  '0000-01-01T00:00:00Z']
        for value in values:
            with self.subTest(value=value, field='as_of'):
                self.rejected(as_of=value, code='invalid_timestamp')
            with self.subTest(value=value, field='created_at'):
                # CSV quoting allows a newline inside a timestamp, still invalid.
                self.rejected(HEADER+'1,A,high,open,"'+value+'"\n', code='invalid_timestamp')

    def test_future_timestamp(self):
        self.rejected(HEADER+'1,A,high,open,2026-01-01T12:00:00.000001Z\n', code='future_created_at')

    def test_rows_and_header(self):
        for content in ['', HEADER.upper(), HEADER+'\n', HEADER+'1,A,high,open\n',
                        HEADER+'1,A,high,open,2026-01-01T12:00:00Z,extra\n',
                        HEADER+',A,high,open,2026-01-01T12:00:00Z\n',
                        HEADER+'1,A,high,open,"unfinished\n']:
            with self.subTest(content=content):
                self.rejected(content, code='malformed_csv')

    def test_duplicate_and_unknown_values(self):
        row = '1,A,high,open,2026-01-01T12:00:00Z\n'
        self.rejected(HEADER+row+row, code='duplicate_id')
        self.rejected(HEADER+row.replace('high','urgent'), code='unknown_priority')
        self.rejected(HEADER+row.replace('open','pending'), code='unknown_status')
        self.rejected(HEADER+row, policy='{"response_minutes":{"low":120}}', code='missing_policy_entry')

    def test_invalid_policies(self):
        values = ['{}','[]','null','{bad', '{"response_minutes":[]}',
                  '{"response_minutes":{"urgent":2}}',
                  '{"response_minutes":{"high":true}}',
                  '{"response_minutes":{"high":-1}}',
                  '{"response_minutes":{"high":1.0}}',
                  '{"response_minutes":{"high":"60"}}',
                  '{"response_minutes":{"high":null}}',
                  '{"response_minutes":{"high":NaN}}',
                  '{"response_minutes":{"high":1,"high":2}}']
        for value in values:
            with self.subTest(policy=value):
                self.rejected(policy=value, code='invalid_policy')

    def test_zero_and_partial_policy(self):
        p = self.run_cli(HEADER+'1,A,low,open,2026-01-01T12:00:00Z\n',
                         policy='{"response_minutes":{"low":0}}')
        self.assertEqual(p.returncode, 0, p.stderr)
        self.assertFalse(json.loads(self.output.read_text())['tickets'][0]['breached'])


if __name__ == '__main__':
    unittest.main()
