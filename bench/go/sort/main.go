package main

import (
	"fmt"
	"slices"
)

func main() {
	x := int64(42)
	items := []int64{}
	for i := 0; i < 1000000; i++ {
		x = (x*1103515245 + 12345) % 2147483648
		items = append(items, x)
	}
	s := slices.Clone(items)
	slices.Sort(s)
	fmt.Println(s[0], s[500000], s[999999])
}
