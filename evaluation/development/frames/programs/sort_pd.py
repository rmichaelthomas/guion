import pandas as pd

frame = pd.read_csv("data/rows.csv")
ordered = frame.sort_values(["group", "x"], ascending=[True, False], na_position="last", kind="stable")
ids = ordered["id"].tolist()
print(ids[0])
print(ids[1])
print(ids[2])
print(ids[-1])
