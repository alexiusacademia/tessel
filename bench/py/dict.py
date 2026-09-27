m = {}
for i in range(1000000):
    m[i * 7] = i
total = 0
for k in range(2000000):
    v = m.get(k)
    if v is not None:
        total += v
print(len(m), total)
