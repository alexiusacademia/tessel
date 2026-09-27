x = 42
items = []
for _ in range(1000000):
    x = (x * 1103515245 + 12345) % 2147483648
    items.append(x)
s = sorted(items)
print(s[0], s[500000], s[999999])
