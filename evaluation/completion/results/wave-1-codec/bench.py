import base64, binascii
total = 0
data = b"The quick brown fox jumps over the lazy dog 0123456789"
for i in range(100000):
    enc = base64.b64encode(data).decode()
    dec = base64.b64decode(enc)
    hx = binascii.hexlify(dec).decode()
    total += len(hx) + len(enc)
print(total)
