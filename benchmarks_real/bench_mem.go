package main

import (
	"fmt"
	"strconv"
	"time"
)

func main() {
	start := time.Now()
	var v []string
	for i := 0; i < 500000; i++ {
		v = append(v, "item_"+strconv.Itoa(i))
	}
	var total int64
	for _, s := range v {
		total += int64(len(s))
	}
	elapsed := time.Since(start)
	fmt.Printf("total_len=%d time_ms=%d\n", total, elapsed.Milliseconds())
}
