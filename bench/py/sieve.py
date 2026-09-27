limit = 20000000
sieve = []
for _ in range(limit):
    sieve.append(True)
count = 0
i = 2
while i < limit:
    if sieve[i]:
        count += 1
        j = i * i
        while j < limit:
            sieve[j] = False
            j += i
    i += 1
print(count)
