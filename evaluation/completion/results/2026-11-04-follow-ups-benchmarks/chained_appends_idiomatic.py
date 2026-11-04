# The form Python programmers use for this job.
parts = []
for i in range(200000):
    parts.append(str(i))
    parts.append(",")
print(len("".join(parts)))
