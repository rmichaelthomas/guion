import numpy as np
a = np.linspace(0.0, 1.0, 4_000_000)
total = 0.0
for _ in range(10):
    total += float(a.sum())
print(repr(total))
