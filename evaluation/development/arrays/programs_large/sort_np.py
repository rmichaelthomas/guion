import numpy as np
n = 1_000_000
x = ((np.arange(n, dtype=np.int64) * 7919) % 1000003).astype(np.float64)
s = np.sort(x, kind="stable")
print(repr(float(s[0])))
print(repr(float(s[500000])))
print(repr(float(s[n - 1])))
