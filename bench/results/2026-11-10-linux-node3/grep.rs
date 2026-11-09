use std::fs;

fn main() {
    let text = fs::read_to_string("bench-input.txt").unwrap();
    let mut lines = 0;
    let mut matches = 0;
    for line in text.lines() {
        lines += 1;
        if line.contains("ae") {
            matches += 1;
        }
    }
    println!("{}\n{}", lines, matches);
}
