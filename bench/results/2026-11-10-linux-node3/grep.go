package main

import ("bufio"; "bytes"; "fmt"; "os"; "strings")

func main() {
	data, err := os.ReadFile("bench-input.txt")
	if err != nil {
		panic(err)
	}
	lines, matches := 0, 0
	sc := bufio.NewScanner(bytes.NewReader(data))
	for sc.Scan() {
		lines++
		if strings.Contains(sc.Text(), "ae") {
			matches++
		}
	}
	fmt.Println(lines)
	fmt.Println(matches)
}
