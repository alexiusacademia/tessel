import Foundation

struct Body { var x, y, z, vx, vy, vz, mass: Double }

func energy(_ b: [Body]) -> Double {
    var e = 0.0
    for i in 0..<b.count {
        e += 0.5 * b[i].mass * (b[i].vx * b[i].vx + b[i].vy * b[i].vy + b[i].vz * b[i].vz)
        for j in (i + 1)..<b.count {
            let dx = b[i].x - b[j].x, dy = b[i].y - b[j].y, dz = b[i].z - b[j].z
            e -= b[i].mass * b[j].mass / (dx * dx + dy * dy + dz * dz).squareRoot()
        }
    }
    return e
}

let solar = 4 * Double.pi * Double.pi, days = 365.24
var b = [
    Body(x: 0, y: 0, z: 0, vx: 0, vy: 0, vz: 0, mass: solar),
    Body(x: 4.84143144246472090e+00, y: -1.16032004402742839e+00, z: -1.03622044471123109e-01, vx: 1.66007664274403694e-03 * days, vy: 7.69901118419740425e-03 * days, vz: -6.90460016972063023e-05 * days, mass: 9.54791938424326609e-04 * solar),
    Body(x: 8.34336671824457987e+00, y: 4.12479856412430479e+00, z: -4.03523417114321381e-01, vx: -2.76742510726862411e-03 * days, vy: 4.99852801234917238e-03 * days, vz: 2.30417297573763929e-05 * days, mass: 2.85885980666130812e-04 * solar),
    Body(x: 1.28943695621391310e+01, y: -1.51111514016986312e+01, z: -2.23307578892655734e-01, vx: 2.96460137564761618e-03 * days, vy: 2.37847173959480950e-03 * days, vz: -2.96589568540237556e-05 * days, mass: 4.36624404335156298e-05 * solar),
    Body(x: 1.53796971148509165e+01, y: -2.59193146099879641e+01, z: 1.79258772950371181e-01, vx: 2.68067772490389322e-03 * days, vy: 1.62824170038242295e-03 * days, vz: -9.51592254519715870e-05 * days, mass: 5.15138902046611451e-05 * solar),
]
let n = b.count
let dt = 0.01
print(String(format: "%.9f", energy(b)))
for _ in 0..<1_000_000 {
    for i in 0..<n {
        for j in (i + 1)..<n {
            let dx = b[i].x - b[j].x, dy = b[i].y - b[j].y, dz = b[i].z - b[j].z
            let d2 = dx * dx + dy * dy + dz * dz
            let mag = dt / (d2 * d2.squareRoot())
            let mi = b[i].mass * mag, mj = b[j].mass * mag
            b[i].vx -= dx * mj; b[i].vy -= dy * mj; b[i].vz -= dz * mj
            b[j].vx += dx * mi; b[j].vy += dy * mi; b[j].vz += dz * mi
        }
    }
    for i in 0..<n {
        b[i].x += dt * b[i].vx; b[i].y += dt * b[i].vy; b[i].z += dt * b[i].vz
    }
}
print(String(format: "%.9f", energy(b)))
