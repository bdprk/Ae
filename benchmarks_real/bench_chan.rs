use std::sync::mpsc::sync_channel;
use std::thread;
use std::time::Instant;

fn main() {
    let start = Instant::now();
    let (tx, rx) = sync_channel(256);
    thread::spawn(move || {
        for i in 1..=200000 {
            tx.send(i).unwrap();
        }
    });
    let mut sum: i64 = 0;
    for _ in 0..200000 {
        sum += rx.recv().unwrap();
    }
    let elapsed = start.elapsed();
    println!("sum={} time_ms={}", sum, elapsed.as_millis());
}
