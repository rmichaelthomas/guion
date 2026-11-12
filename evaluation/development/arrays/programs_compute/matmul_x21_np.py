import numpy as np
n = 512
a = np.linspace(0.0, 1.0, n * n).reshape(n, n)
b = np.linspace(1.0, 2.0, n * n).reshape(n, n)
t = 0.0
for _ in range(21):
    t += float((a @ b).sum())
print(repr(t))
