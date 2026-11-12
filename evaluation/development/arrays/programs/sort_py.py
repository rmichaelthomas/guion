n = 1_000_000
x = [float((i * 7919) % 1000003) for i in range(n)]
s = sorted(x)
print(repr(s[0]))
print(repr(s[500000]))
print(repr(s[n - 1]))
