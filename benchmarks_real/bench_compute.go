package main

import (
	"fmt"
	"time"
)

func fib(n int64) int64 {
	if n <= 1 {
		return n
	}
	return fib(n-1) + fib(n-2)
}

func main() {
	start := time.Now()
	res := fib(38)
	elapsed := time.Since(start)
	fmt.Printf("fib(38)=%d time_ms=%d\n", res, elapsed.Milliseconds())
}
