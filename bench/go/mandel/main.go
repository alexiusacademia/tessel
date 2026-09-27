package main

import "fmt"

func main() {
	size := 1200
	inside := 0
	for py := 0; py < size; py++ {
		for px := 0; px < size; px++ {
			cx := float64(px)/float64(size)*3.0 - 2.0
			cy := float64(py)/float64(size)*3.0 - 1.5
			x, y := 0.0, 0.0
			n := 0
			for n < 100 && x*x+y*y <= 4.0 {
				t := x*x - y*y + cx
				y = 2.0*x*y + cy
				x = t
				n++
			}
			if n == 100 {
				inside++
			}
		}
	}
	fmt.Println(inside)
}
