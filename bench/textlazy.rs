// The same statistics as text.rs — Rust's `split(' ')` is already lazy, so the
// `textlazy` workload uses it unchanged. Having both `text` and `textlazy`
// entries makes the two API shapes explicit on the AE and Go sides, where
// the eager form differs (ADR-53).

use std::fs;

fn main() {
    let path = std::env::args().nth(1).expect("usage: textstats <file>");
    let text = fs::read_to_string(&path).expect("cannot read file");
    let mut tokens = 0usize;
    let mut inside = 0usize;
    for token in text.split(' ') {
        tokens = tokens.checked_add(1).expect("token count overflow");
        inside = inside.checked_add(token.len()).expect("length overflow");
    }
    println!("{}", text.len());
    println!("{tokens}");
    println!("{inside}");
}
