"""Write the PGO training inputs into the directory given as the first argument."""
import json, random, sys

out = sys.argv[1]
r = random.Random(99)
kinds = ["click", "view", "purchase", "refund", "signup"]
with open(f"{out}/clicks.jsonl", "w") as f:
    for i in range(150000):
        d = {"user": f"u{r.randint(1,500)}", "kind": r.choice(kinds), "ms": r.randint(1, 9000), "ts": f"2026-{r.randint(1,12):02d}-{r.randint(1,28):02d}T{r.randint(0,23):02d}:{r.randint(0,59):02d}:{r.randint(0,59):02d}Z", "tags": ["a", "b"], "ok": r.random() > 0.1}
        f.write((json.dumps(d) if i % 211 else "{broken") + "\n")
with open(f"{out}/items.csv", "w") as f:
    f.write("item,qty,price,region\n")
    for i in range(150000):
        f.write(f"sku-{r.randint(1,300)},{r.randint(1,40) if i%131 else 'x'},{r.randint(100,99999)},r{r.randint(1,8)}\n")
