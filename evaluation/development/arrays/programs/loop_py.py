n = 1_000_000
step = 1.0 / (n - 1)
a = [i * step for i in range(n)]
total = 0.0
for i in range(n):
    total += a[i]
print(repr(total))
