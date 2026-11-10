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
