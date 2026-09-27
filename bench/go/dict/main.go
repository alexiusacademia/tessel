package main

import "fmt"

func main() {
	m := map[int]int{}
	for i := 0; i < 1000000; i++ {
		m[i*7] = i
	}
	sum := 0
	for k := 0; k < 2000000; k++ {
		if v, ok := m[k]; ok {
			sum += v
		}
	}
	fmt.Println(len(m), sum)
}
