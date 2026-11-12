import pandas as pd

frame = pd.read_csv("data/rows.csv")
kept = frame[frame["x"] > 0.5]
print(len(kept))
print(repr(float(kept["y"].sum())))
