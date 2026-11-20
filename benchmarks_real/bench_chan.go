package main

import (
	"fmt"
	"time"
)

func main() {
	start := time.Now()
	ch := make(chan int64, 256)
	go func() {
		for i := int64(1); i <= 200000; i++ {
			ch <- i
		}
	}()
	var sum int64
	for i := 0; i < 200000; i++ {
		sum += <-ch
	}
	elapsed := time.Since(start)
	fmt.Printf("sum=%d time_ms=%d\n", sum, elapsed.Milliseconds())
}
