import numpy as np
n = 10_000_000
a = np.linspace(0.0, 1.0, n)
b = np.linspace(1.0, 2.0, n)
c = np.linspace(2.0, 3.0, n)
for _ in range(5):
    r = a * b + c
print(repr(float(r.sum())))
