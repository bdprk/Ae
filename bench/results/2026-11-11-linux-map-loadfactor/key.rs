// Rust twin of key.ae: LCG plus key formatting, no map.
fn main() {
    let mut x: i64 = 1234567;
    let mut acc: i64 = 0;
    for _ in 0..1000000 {
        x = (x * 48271) % 2147483647;
        let key = format!("k{}", x % 10000);
        acc += key.len() as i64;
    }
    println!("{}", acc);
}
