n = 256
m = n * n
step = 1.0 / (m - 1)
a = [[(i * n + j) * step for j in range(n)] for i in range(n)]
b = [[1.0 + (i * n + j) * step for j in range(n)] for i in range(n)]
bt = list(zip(*b))
total = 0.0
for row in a:
    for col in bt:
        total += sum(x * y for x, y in zip(row, col))
print(repr(total))
