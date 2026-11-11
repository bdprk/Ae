// Rust twin of arith.ae: only the LCG arithmetic.
fn main() {
    let mut x: i64 = 1234567;
    let mut acc: i64 = 0;
    for _ in 0..1000000 {
        x = (x * 48271) % 2147483647;
        acc += x % 10000;
    }
    println!("{}", acc);
}
