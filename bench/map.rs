// Histogram counting, the Rust twin of map.go: a fixed-seed LCG feeds
// 10000 string keys, a million draws increment the histogram, then a
// count-weighted checksum reads every bucket. Output must match the other
// implementations exactly.

use std::collections::HashMap;

const KEY_COUNT: i64 = 10000;
const DRAWS: i64 = 1000000;

fn key(i: i64) -> String {
	format!("k{}", i)
}

fn main() {
	let mut counts: HashMap<String, i64> = HashMap::new();
	let mut x: i64 = 1234567;
	for _ in 0..DRAWS {
		x = (x * 48271) % 2147483647;
		let k = format!("k{}", x % KEY_COUNT);
		*counts.entry(k).or_insert(0) += 1;
	}
	let mut checksum: i64 = 0;
	for i in 0..KEY_COUNT {
		let v = counts.get(&key(i)).copied().unwrap_or(0);
		checksum = checksum + v * (i + 1);
	}
	println!("draws=1000000");
	println!("keys=10000");
	println!("distinct={}", counts.len());
	println!("total=1000000");
	println!("checksum={}", checksum);
}
