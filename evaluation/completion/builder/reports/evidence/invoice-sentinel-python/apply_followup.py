from pathlib import Path
from datetime import datetime, timezone
import json
import shutil
root = Path(__file__).parent
source = root/'invoice_sentinel.py'
shutil.copy2(source, root/'invoice_sentinel_initial.py')
(root/'followup-start.json').write_text(json.dumps({'utc': datetime.now(timezone.utc).isoformat()})+'\n')
text = source.read_text()
text = text.replace('def audit(input_path, as_of):\n    vendors = {}', 'def audit(input_path, as_of, vendor_filter=None):\n    vendors = {}\n    excluded = 0')
text = text.replace('            totals = vendors.setdefault(vendor, {', '            if vendor_filter is not None and vendor != vendor_filter:\n                excluded += 1\n                continue\n            totals = vendors.setdefault(vendor, {')
text = text.replace('    return {\n        \'as_of\': as_of.isoformat(),', '    result = {\n        \'as_of\': as_of.isoformat(),')
text = text.replace("        'rejected_row_count': len(rejected), 'rejected_rows': rejected,\n    }\n", "        'rejected_row_count': len(rejected), 'rejected_rows': rejected,\n    }\n    if vendor_filter is not None:\n        result['excluded_row_count'] = excluded\n    return result\n")
text = text.replace("    args = parser.parse_args(argv)", "    parser.add_argument('--vendor', help='exact, case-sensitive vendor filter')\n    args = parser.parse_args(argv)")
text = text.replace('result = audit(args.input, as_of)', 'result = audit(args.input, as_of, args.vendor)')
source.write_text(text)
