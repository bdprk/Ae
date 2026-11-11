// Rust twin of hit.ae: 1M hits on a pre-filled map.
use std::collections::HashMap;
fn main() {
    let mut counts: HashMap<String, i64> = HashMap::new();
    for i in 0..10000i64 {
        counts.insert(format!("k{}", i), i + 1);
    }
    let mut x: i64 = 1234567;
    let mut acc: i64 = 0;
    for _ in 0..1000000 {
        x = (x * 48271) % 2147483647;
        let key = format!("k{}", x % 10000);
        acc += counts.get(&key).copied().unwrap_or(0);
    }
    println!("{}", acc);
}
