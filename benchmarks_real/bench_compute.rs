use std::time::Instant;

fn fib(n: i64) -> i64 {
    if n <= 1 {
        return n;
    }
    fib(n - 1) + fib(n - 2)
}

fn main() {
    let start = Instant::now();
    let res = fib(38);
    let elapsed = start.elapsed();
    println!("fib(38)={} time_ms={}", res, elapsed.as_millis());
}
