package main

import (
	"fmt"
	"math"
)

type Body struct{ x, y, z, vx, vy, vz, mass float64 }

func energy(b []Body) float64 {
	e := 0.0
	for i := range b {
		e += 0.5 * b[i].mass * (b[i].vx*b[i].vx + b[i].vy*b[i].vy + b[i].vz*b[i].vz)
		for j := i + 1; j < len(b); j++ {
			dx, dy, dz := b[i].x-b[j].x, b[i].y-b[j].y, b[i].z-b[j].z
			e -= b[i].mass * b[j].mass / math.Sqrt(dx*dx+dy*dy+dz*dz)
		}
	}
	return e
}

func main() {
	solar := 4 * math.Pi * math.Pi
	days := 365.24
	b := []Body{
		{0, 0, 0, 0, 0, 0, solar},
		{4.84143144246472090e+00, -1.16032004402742839e+00, -1.03622044471123109e-01, 1.66007664274403694e-03 * days, 7.69901118419740425e-03 * days, -6.90460016972063023e-05 * days, 9.54791938424326609e-04 * solar},
		{8.34336671824457987e+00, 4.12479856412430479e+00, -4.03523417114321381e-01, -2.76742510726862411e-03 * days, 4.99852801234917238e-03 * days, 2.30417297573763929e-05 * days, 2.85885980666130812e-04 * solar},
		{1.28943695621391310e+01, -1.51111514016986312e+01, -2.23307578892655734e-01, 2.96460137564761618e-03 * days, 2.37847173959480950e-03 * days, -2.96589568540237556e-05 * days, 4.36624404335156298e-05 * solar},
		{1.53796971148509165e+01, -2.59193146099879641e+01, 1.79258772950371181e-01, 2.68067772490389322e-03 * days, 1.62824170038242295e-03 * days, -9.51592254519715870e-05 * days, 5.15138902046611451e-05 * solar},
	}
	n := len(b)
	dt := 0.01
	fmt.Printf("%.9f\n", energy(b))
	for s := 0; s < 1000000; s++ {
		for i := 0; i < n; i++ {
			for j := i + 1; j < n; j++ {
				dx, dy, dz := b[i].x-b[j].x, b[i].y-b[j].y, b[i].z-b[j].z
				d2 := dx*dx + dy*dy + dz*dz
				mag := dt / (d2 * math.Sqrt(d2))
				mi, mj := b[i].mass*mag, b[j].mass*mag
				b[i].vx -= dx * mj
				b[i].vy -= dy * mj
				b[i].vz -= dz * mj
				b[j].vx += dx * mi
				b[j].vy += dy * mi
				b[j].vz += dz * mi
			}
		}
		for i := range b {
			b[i].x += dt * b[i].vx
			b[i].y += dt * b[i].vy
			b[i].z += dt * b[i].vz
		}
	}
	fmt.Printf("%.9f\n", energy(b))
}
