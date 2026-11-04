import math
total=0.0
for i in range(2000000):
    x=i*0.001
    total+=math.sin(x)*math.cos(x)+math.atan(x)
print(total)
