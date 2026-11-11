use std::collections::HashMap;

fn main() {
    let mut counts: HashMap<&str, i64> = HashMap::new();
    counts.insert("present", 1);
    let mut seen: i64 = 0;
    for _ in 0..1_000_000 {
        match counts.get("absent") {
            Some(n) => seen += *n,
            None => seen += "key not found".len() as i64,
        }
    }
    println!("{}", seen);
}
