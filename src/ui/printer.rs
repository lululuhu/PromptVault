//! Small colored-output helpers.
//!
//! Colors are auto-disabled when stdout/stderr is not a TTY (e.g. when piping
//! to `grep` or redirecting to a file), so ANSI escape codes never leak into
//! pipelines.

use std::io::IsTerminal;
use std::sync::OnceLock;

use nu_ansi_term::Color;

static USE_COLOR: OnceLock<bool> = OnceLock::new();

fn color_enabled() -> bool {
    *USE_COLOR.get_or_init(|| std::io::stdout().is_terminal())
}

pub fn ok(msg: &str) {
    if color_enabled() {
        println!("{}", Color::Green.paint(msg));
    } else {
        println!("{msg}");
    }
}

pub fn info(msg: &str) {
    if color_enabled() {
        println!("{}", Color::Cyan.paint(msg));
    } else {
        println!("{msg}");
    }
}

pub fn warn(msg: &str) {
    if color_enabled() {
        println!("{}", Color::Yellow.paint(msg));
    } else {
        println!("{msg}");
    }
}

pub fn error(msg: &str) {
    if std::io::stderr().is_terminal() {
        eprintln!("{}", Color::Red.bold().paint(msg));
    } else {
        eprintln!("{msg}");
    }
}

pub fn bold(msg: &str) -> String {
    if color_enabled() {
        Color::White.bold().paint(msg).to_string()
    } else {
        msg.to_string()
    }
}

pub fn dim(msg: &str) -> String {
    if color_enabled() {
        Color::DarkGray.paint(msg).to_string()
    } else {
        msg.to_string()
    }
}
