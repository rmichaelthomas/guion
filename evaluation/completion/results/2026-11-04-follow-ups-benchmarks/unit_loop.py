acc = 0
for i in range(20000000):
    acc = (acc + i * 3) % 1000003
print(acc)
