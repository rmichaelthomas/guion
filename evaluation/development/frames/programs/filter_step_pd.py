import time

import pandas as pd

frame = pd.read_csv("data/rows.csv")
batch = 20
samples = 15
total = 0.0
for round_index in range(samples + 1):
    start = time.time()
    for _ in range(batch):
        kept = frame[frame["x"] > 0.5]
        total += float(kept["y"].sum())
    elapsed = time.time() - start
    if round_index > 0:
        print(round(elapsed * 1000))
print(repr(total))
