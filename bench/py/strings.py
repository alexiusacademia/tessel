parts = []
for i in range(1000000):
    parts.append(f"item{i}")
text = ",".join(parts)
count = 0
for p in text.split(","):
    if p.endswith("7"):
        count += 1
print(len(text), count)
