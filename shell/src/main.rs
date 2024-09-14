use std::{io::Write, time::Duration};
use chrono::Local;
use std::thread;

fn main() {
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .write(true)
        .create(true)
        .open("/tmp/chop.txt")
        .unwrap();
    let now = Local::now();
    let formatted_time = now.format("%m-%d %H:%M:%S").to_string();

    write!(file, "Hello, world! {}\n", formatted_time).unwrap();
    thread::sleep(Duration::new(10, 0));
}
