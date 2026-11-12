n = 4_000_000
step = 1.0 / (n - 1)
a = [i * step for i in range(n)]
total = 0.0
for _ in range(10):
    total += sum(a)
print(repr(total))
