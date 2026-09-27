package main

import "fmt"

func main() {
	limit := 20000000
	sieve := make([]bool, 0)
	for i := 0; i < limit; i++ {
		sieve = append(sieve, true)
	}
	count := 0
	for i := 2; i < limit; i++ {
		if sieve[i] {
			count++
			for j := i * i; j < limit; j += i {
				sieve[j] = false
			}
		}
	}
	fmt.Println(count)
}
