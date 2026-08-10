//! Small colored-output helpers.

use nu_ansi_term::Color;

pub fn ok(msg: &str) {
    println!("{}", Color::Green.paint(msg));
}

pub fn info(msg: &str) {
    println!("{}", Color::Cyan.paint(msg));
}

pub fn warn(msg: &str) {
    println!("{}", Color::Yellow.paint(msg));
}

pub fn error(msg: &str) {
    eprintln!("{}", Color::Red.bold().paint(msg));
}

pub fn bold(msg: &str) -> String {
    Color::White.bold().paint(msg).to_string()
}

pub fn dim(msg: &str) -> String {
    Color::DarkGray.paint(msg).to_string()
}
