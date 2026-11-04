import hashlib,time
from urllib.parse import quote,unquote
t=time.time();total=0
data=b"The quick brown fox jumps over the lazy dog 0123456789";key=b"key!"
for i in range(100000):
    d=hashlib.sha1(data).digest()
    m=bytes(b^key[j%4] for j,b in enumerate(data))
    p=unquote(quote("a b&c=d/é?",safe=""))
    ints=list(d)
    total+=len(m)+len(p)+len(ints)
print(total,round(time.time()-t,3))
