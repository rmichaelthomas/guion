import re,time
t=time.time()
total=0
for i in range(100000):
    f=re.search(r"[a-z]+[0-9]{2,4}","the quick brown fox12345 jumps") is not None
    s=re.sub(r"[aeiou]","_","the quick brown fox jumps")
    if f: total+=len(s)
print(total, round(time.time()-t,3))
