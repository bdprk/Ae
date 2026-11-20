use std::time::Instant;

fn main() {
    let start = Instant::now();
    let mut v = Vec::with_capacity(500000);
    for i in 0..500000 {
        v.push(format!("item_{i}"));
    }
    let mut total: i64 = 0;
    for s in &v {
        total += s.len() as i64;
    }
    let elapsed = start.elapsed();
    println!("total_len={} time_ms={}", total, elapsed.as_millis());
}
