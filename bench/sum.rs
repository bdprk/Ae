fn main() {
    let (mut i, mut total): (i64, i64) = (0, 0);
    while i < 1_000_000 {
        total = total.checked_add(i).expect("integer overflow");
        i = i.checked_add(1).expect("integer overflow");
    }
    println!("{total}");
}
