var m: [Int: Int] = [:]
for i in 0..<1_000_000 {
    m[i * 7] = i
}
var sum = 0
for k in 0..<2_000_000 {
    if let v = m[k] { sum += v }
}
print(m.count, sum)
