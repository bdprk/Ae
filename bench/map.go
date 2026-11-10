// Histogram counting: a fixed-seed pseudo-random stream (LCG, all arithmetic
// in int64 without overflow) is drawn a million times, each draw increments
// one of 10000 string keys, then a count-weighted checksum touches every
// bucket and len() reports how many keys were reached. The checksum cannot be
// computed from the draw count or the key count alone, so every draw and
// every lookup is exercised. All four implementations must print identical
// text (run.py compares them).

package main

import "fmt"
import "strconv"

const key_count = 10000
const draws = 1000000

func key(i int) string {
	return "k" + strconv.Itoa(i)
}

func main() {
	counts := map[string]int64{}
	var x int64 = 1234567
	for j := 0; j < draws; j++ {
		x = (x * 48271) % 2147483647
		k := "k" + strconv.FormatInt(x%key_count, 10)
		counts[k] = counts[k] + 1
	}
	var checksum int64 = 0
	for i := 0; i < key_count; i++ {
		v, ok := counts[key(i)]
		if !ok {
			v = 0
		}
		checksum = checksum + v*int64(i+1)
	}
	fmt.Println("draws=1000000")
	fmt.Println("keys=10000")
	fmt.Println("distinct=" + strconv.Itoa(len(counts)))
	fmt.Println("total=1000000")
	fmt.Println("checksum=" + strconv.FormatInt(checksum, 10))
}
