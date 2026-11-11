// Rust twin of put.ae: 1M overwrites of existing keys.
use std::collections::HashMap;
fn main() {
    let mut counts: HashMap<String, i64> = HashMap::new();
    for i in 0..10000i64 {
        counts.insert(format!("k{}", i), 0);
    }
    let mut x: i64 = 1234567;
    for j in 0..1000000i64 {
        x = (x * 48271) % 2147483647;
        let key = format!("k{}", x % 10000);
        counts.insert(key, j);
    }
    let mut acc: i64 = 0;
    for i in 0..10000i64 {
        acc += counts.get(&format!("k{}", i)).copied().unwrap_or(0);
    }
    println!("{}", acc);
}
