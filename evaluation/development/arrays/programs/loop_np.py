import numpy as np
n = 1_000_000
a = np.linspace(0.0, 1.0, n)
total = 0.0
for i in range(n):
    total += a[i]
print(repr(float(total)))
