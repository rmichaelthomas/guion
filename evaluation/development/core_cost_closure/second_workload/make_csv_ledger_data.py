import random
import sys
from pathlib import Path

rows = int(sys.argv[1])
out = Path(sys.argv[2])
rng = random.Random(20261031)
services = [f"svc-{index:02d}" for index in range(40)]
with out.open("w", encoding="utf-8", newline="") as handle:
    handle.write("service,status,latency_ms,bytes\n")
    for index in range(rows):
        service = rng.choice(services)
        status = rng.choice([200, 200, 200, 201, 204, 301, 404, 500, 502])
        latency = rng.randint(1, 4000)
        if index % 97 == 0:
            latency_text = "n/a"
        else:
            latency_text = str(latency)
        handle.write(f"{service},{status},{latency_text},{rng.randint(10, 90000)}\n")
