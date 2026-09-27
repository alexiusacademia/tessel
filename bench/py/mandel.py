size = 1200
inside = 0
for py in range(size):
    for px in range(size):
        cx = px / size * 3.0 - 2.0
        cy = py / size * 3.0 - 1.5
        x = y = 0.0
        n = 0
        while n < 100 and x * x + y * y <= 4.0:
            t = x * x - y * y + cx
            y = 2.0 * x * y + cy
            x = t
            n += 1
        if n == 100:
            inside += 1
print(inside)
