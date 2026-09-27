let size = 1200
var inside = 0
for py in 0..<size {
    for px in 0..<size {
        let cx = Double(px) / Double(size) * 3.0 - 2.0
        let cy = Double(py) / Double(size) * 3.0 - 1.5
        var x = 0.0, y = 0.0
        var n = 0
        while n < 100 && x * x + y * y <= 4.0 {
            let t = x * x - y * y + cx
            y = 2.0 * x * y + cy
            x = t
            n += 1
        }
        if n == 100 { inside += 1 }
    }
}
print(inside)
