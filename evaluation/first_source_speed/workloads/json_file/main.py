import json
import sys
from pathlib import Path

print(len(json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))))
