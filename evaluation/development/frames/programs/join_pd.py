import pandas as pd

frame = pd.read_csv("data/rows.csv")
lookup = pd.read_csv("data/lookup.csv")
joined = frame.merge(lookup, on="group", how="inner")
print(len(joined))
print(repr(float(joined["weight"].sum())))
