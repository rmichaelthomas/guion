import pandas as pd

frame = pd.read_csv("data/rows.csv")
groups = frame.groupby("group", sort=False, dropna=False)["x"].agg(["mean", "count"])
print(len(groups))
print(repr(float(groups["mean"].sum())))
print(repr(float(groups["count"].sum())))
print(repr(float(groups["mean"].iloc[0])))
