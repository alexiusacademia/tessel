import math

class Body:
    __slots__ = ("x", "y", "z", "vx", "vy", "vz", "mass")
    def __init__(self, x, y, z, vx, vy, vz, mass):
        self.x, self.y, self.z, self.vx, self.vy, self.vz, self.mass = x, y, z, vx, vy, vz, mass

def energy(bodies):
    e = 0.0
    for i, b in enumerate(bodies):
        e += 0.5 * b.mass * (b.vx * b.vx + b.vy * b.vy + b.vz * b.vz)
        for c in bodies[i + 1:]:
            dx, dy, dz = b.x - c.x, b.y - c.y, b.z - c.z
            e -= b.mass * c.mass / math.sqrt(dx * dx + dy * dy + dz * dz)
    return e

solar = 4 * math.pi * math.pi
days = 365.24
bodies = [
    Body(0, 0, 0, 0, 0, 0, solar),
    Body(4.84143144246472090e+00, -1.16032004402742839e+00, -1.03622044471123109e-01, 1.66007664274403694e-03 * days, 7.69901118419740425e-03 * days, -6.90460016972063023e-05 * days, 9.54791938424326609e-04 * solar),
    Body(8.34336671824457987e+00, 4.12479856412430479e+00, -4.03523417114321381e-01, -2.76742510726862411e-03 * days, 4.99852801234917238e-03 * days, 2.30417297573763929e-05 * days, 2.85885980666130812e-04 * solar),
    Body(1.28943695621391310e+01, -1.51111514016986312e+01, -2.23307578892655734e-01, 2.96460137564761618e-03 * days, 2.37847173959480950e-03 * days, -2.96589568540237556e-05 * days, 4.36624404335156298e-05 * solar),
    Body(1.53796971148509165e+01, -2.59193146099879641e+01, 1.79258772950371181e-01, 2.68067772490389322e-03 * days, 1.62824170038242295e-03 * days, -9.51592254519715870e-05 * days, 5.15138902046611451e-05 * solar),
]
n = len(bodies)
dt = 0.01
print(f"{energy(bodies):.9f}")
for _ in range(1000000):
    for i in range(n):
        bi = bodies[i]
        for j in range(i + 1, n):
            bj = bodies[j]
            dx, dy, dz = bi.x - bj.x, bi.y - bj.y, bi.z - bj.z
            d2 = dx * dx + dy * dy + dz * dz
            mag = dt / (d2 * math.sqrt(d2))
            mi, mj = bi.mass * mag, bj.mass * mag
            bi.vx -= dx * mj
            bi.vy -= dy * mj
            bi.vz -= dz * mj
            bj.vx += dx * mi
            bj.vy += dy * mi
            bj.vz += dz * mi
    for b in bodies:
        b.x += dt * b.vx
        b.y += dt * b.vy
        b.z += dt * b.vz
print(f"{energy(bodies):.9f}")
