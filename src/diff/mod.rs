pub mod lcs;
#[allow(unused_imports)]
pub use lcs::{diff_lines, DiffKind, DiffLine};

/// Split text into lines for diffing (trailing newline is ignored).
pub fn split_lines(text: &str) -> Vec<String> {
    text.lines().map(|s| s.to_string()).collect()
}
