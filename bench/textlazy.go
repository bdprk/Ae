// The same statistics as text.go, in the lazy shape: scan the bytes once and
// count tokens and their lengths without building a slice of tokens. This is
// the Go analogue of ae's `tokenize` cursor and of Rust's `split(' ')`
// iterator, so the `textlazy` workload compares the same algorithm in all
// three languages (the `text` workload keeps the eager `split`/`strings.Split`
// shape instead).
package main

import (
	"fmt"
	"math"
	"os"
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
	var tokens int64
	var inside int64
	start := 0
	for i := 0; i <= len(raw); i++ {
		if i == len(raw) || raw[i] == ' ' {
			tokens = add(tokens, 1)
			inside = add(inside, int64(i-start))
			start = i + 1
		}
	}
	fmt.Println(len(raw))
	fmt.Println(tokens)
	fmt.Println(inside)
}
