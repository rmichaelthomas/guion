n = 4_000_000
step = 1.0 / (n - 1)
a = [i * step for i in range(n)]
b = [1.0 + i * step for i in range(n)]
c = [2.0 + i * step for i in range(n)]
for _ in range(5):
    r = [x * y + z for x, y, z in zip(a, b, c)]
print(repr(sum(r)))
