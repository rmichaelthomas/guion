# The same statement as append.gu: s = s + a + b copies s on every pass in CPython.
s = ""
for i in range(200000):
    s = s + str(i) + ","
print(len(s))
