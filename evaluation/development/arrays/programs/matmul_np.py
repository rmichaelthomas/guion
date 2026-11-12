import numpy as np
n = 256
a = np.linspace(0.0, 1.0, n * n).reshape(n, n)
b = np.linspace(1.0, 2.0, n * n).reshape(n, n)
print(repr(float((a @ b).sum())))
