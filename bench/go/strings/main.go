package main

import (
	"fmt"
	"strings"
)

func main() {
	parts := []string{}
	for i := 0; i < 1000000; i++ {
		parts = append(parts, fmt.Sprintf("item%d", i))
	}
	text := strings.Join(parts, ",")
	count := 0
	for _, p := range strings.Split(text, ",") {
		if strings.HasSuffix(p, "7") {
			count++
		}
	}
	fmt.Println(len(text), count)
}
