var parts: [String] = []
for i in 0..<1_000_000 {
    parts.append("item\(i)")
}
let text = parts.joined(separator: ",")
var count = 0
for p in text.split(separator: ",") {
    if p.hasSuffix("7") { count += 1 }
}
print(text.utf8.count, count)
