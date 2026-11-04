import random

rng = random.Random(42)
total = 0
total_float = 0.0
for _ in range(1000000):
    total_float += rng.random()
    total += rng.randrange(0, 6)
print(total)
print(total_float)
