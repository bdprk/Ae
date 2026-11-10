package main

import (
	"fmt"
	"math"
	"os"
	"strings"
)

func add(a, b int64) int64 {
	if (b > 0 && a > math.MaxInt64-b) || (b < 0 && a < math.MinInt64-b) {
		panic("integer overflow")
	}
	return a + b
}

func main() {
	if len(os.Args) < 2 {
		fmt.Println("usage: textstats <file>")
		return
	}
	raw, err := os.ReadFile(os.Args[1])
	if err != nil {
		panic(err)
	}
	tokens := strings.Split(string(raw), " ")
	var inside int64
	for _, token := range tokens {
		inside = add(inside, int64(len(token)))
	}
	fmt.Println(len(raw))
	fmt.Println(len(tokens))
	fmt.Println(inside)
}
