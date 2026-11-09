package main
import ("fmt"; "math")
func add(a, b int64) int64 {
    if (b > 0 && a > math.MaxInt64-b) || (b < 0 && a < math.MinInt64-b) { panic("integer overflow") }
    return a + b
}
func main() {
    var i, total int64
    for i < 1000000 { total = add(total, i); i = add(i, 1) }
    fmt.Println(total)
}
