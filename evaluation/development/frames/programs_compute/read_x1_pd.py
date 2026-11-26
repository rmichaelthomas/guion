import pandas as pd

for _ in range(1):
    frame = pd.read_csv("data/rows.csv")
    rows = len(frame)
    total = float(frame["x"].sum())
print(rows)
print(repr(total))
