import re
text = "".join(f"user{i}@example.com, order {i*7} shipped; " for i in range(1500))
total = 0
for _ in range(20):
    emails = re.findall(r"[a-z0-9]+@[a-z]+\.com", text)
    numbers = re.findall(r"[0-9]+", text)
    swapped = re.sub(r"order ([0-9]+)", r"#\1", text)
    parts = re.split(r";\s*", text)
    total += len(emails) + len(numbers) + len(swapped) + len(parts)
print(total)
