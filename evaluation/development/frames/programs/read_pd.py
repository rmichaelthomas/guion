import pandas as pd

frame = pd.read_csv("data/rows.csv")
print(len(frame))
print(repr(float(frame["x"].sum())))
print(int(frame["x"].count()))
