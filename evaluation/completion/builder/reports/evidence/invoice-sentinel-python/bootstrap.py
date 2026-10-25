from pathlib import Path
from datetime import datetime, timezone
import json
root = Path(__file__).parent
(root/'started.json').write_text(json.dumps({'start_utc': datetime.now(timezone.utc).isoformat(), 'python': __import__('sys').version, 'setup_seconds': 30}, indent=2)+'\n')
(root/'session-contract.limn').write_text('remember a string called source-state with "scanned"\nremember a string called claim-basis with "none"\nremember a list called tracked-decisions with "none"\nremember a list called open-questions with "none"\nremember a list called session-corrections with "none"\nadd "Independent Python standard-library implementation from the supplied brief" to tracked-decisions\n')
(root/'fixtures').mkdir(exist_ok=True)
(root/'fixtures'/'ledger.tsv').write_text('invoice_id\tvendor\tamount_cents\tdue_date\tstate\na1\tAcme\t1200\t2026-01-01\topen\nb1\tBeta\t500\t2026-03-01\topen\na2\tAcme\t300\t2026-02-01\topen\nb2\tBeta\t700\t2025-01-01\tpaid\nc1\tCafé Supplies\t900\t2026-04-01\topen\nbad1\tAcme\t12.50\t2026-01-01\topen\nbad2\tBeta\t100\t2026-02-30\topen\nbad3\tAcme\t100\t2026-01-01\tunknown\nwrong\twidth\n', encoding='utf-8')
(root/'fixtures'/'empty.tsv').write_text('invoice_id\tvendor\tamount_cents\tdue_date\tstate\n')
(root/'fixtures'/'bad-header.tsv').write_text('vendor\tamount\nAcme\t42\n')
