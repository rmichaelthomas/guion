import pandas as pd

for _ in range(6):
    frame = pd.read_csv("data/rows.csv")
    rows = len(frame)
    total = float(frame["x"].sum())
print(rows)
print(repr(total))
