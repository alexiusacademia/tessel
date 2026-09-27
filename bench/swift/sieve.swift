let limit = 20_000_000
var sieve: [Bool] = []
for _ in 0..<limit {
    sieve.append(true)
}
var count = 0
var i = 2
while i < limit {
    if sieve[i] {
        count += 1
        var j = i * i
        while j < limit {
            sieve[j] = false
            j += i
        }
    }
    i += 1
}
print(count)
