//! Token counting for prompt content.
//!
//! When the `run` feature is enabled, uses `tiktoken-rs` for an exact count
//! with the model's real BPE encoding. Otherwise, falls back to a robust
//! heuristic (~4 chars / token for English, with a CJK/emoji adjustment so
//! Chinese / Japanese / Korean content is not massively undercounted).
//!
//! The heuristic is intentionally conservative (overestimates a touch) so
//! cost estimates never silently surprise the user on the low side.

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum TokenMethod {
    /// Computed with the real BPE encoding via `tiktoken-rs`.
    Exact { encoding: &'static str },
    /// Computed with a character-class heuristic (no `run` feature).
    Estimate,
}

impl TokenMethod {
    pub fn is_exact(&self) -> bool {
        matches!(self, TokenMethod::Exact { .. })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TokenCount {
    pub tokens: usize,
    pub chars: usize,
    pub words: usize,
    pub lines: usize,
    pub method: TokenMethod,
}

/// Count tokens in `text`. Picks the best method available at compile time.
pub fn count(text: &str) -> TokenCount {
    let chars = text.chars().count();
    let words = count_words(text);
    let lines = text.lines().count();

    #[cfg(feature = "run")]
    {
        if let Some((tokens, encoding)) = count_exact(text) {
            return TokenCount { tokens, chars, words, lines, method: TokenMethod::Exact { encoding } };
        }
    }

    let tokens = estimate_tokens(text);
    TokenCount { tokens, chars, words, lines, method: TokenMethod::Estimate }
}

/// Rough per-1-call assumption for "expected output size" when none is given.
/// Conservative default: half of input, capped to [200, 2000].
pub fn assumed_output_tokens(input_tokens: usize) -> usize {
    (input_tokens / 2).clamp(200, 2000)
}

// ---- heuristic ------------------------------------------------------------

/// Estimate token count using a per-character-class rule:
///   - ASCII word char      → ~0.25 token (≈ 4 chars/token, English baseline)
///   - Whitespace            → ~0.10 token
///   - CJK / emoji / etc.    → ~1.0 token each (BPE tends to 1 token per CJK char)
///   - Other punctuation    → ~0.5 token
pub fn estimate_tokens(text: &str) -> usize {
    let mut tokens = 0.0f64;
    for c in text.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '\'' {
            tokens += 0.25;
        } else if c.is_whitespace() {
            tokens += 0.10;
        } else if is_cjk_or_emoji(c) {
            tokens += 1.0;
        } else {
            tokens += 0.5;
        }
    }
    tokens.ceil() as usize
}

fn is_cjk_or_emoji(c: char) -> bool {
    let cp = c as u32;
    // CJK Unified Ideographs + extensions (basic plane).
    (0x4E00..=0x9FFF).contains(&cp)
        || (0x3400..=0x4DBF).contains(&cp)
        || (0x20000..=0x2FFFF).contains(&cp)
        // Hiragana / Katakana / Hangul.
        || (0x3040..=0x30FF).contains(&cp)
        || (0xAC00..=0xD7AF).contains(&cp)
        // Emoji-ish ranges.
        || (0x1F000..=0x1FAFF).contains(&cp)
        || (0x2600..=0x27BF).contains(&cp)
}

fn count_words(text: &str) -> usize {
    text.split_whitespace().count()
}

// ---- exact (tiktoken-rs) -------------------------------------------------

#[cfg(feature = "run")]
fn count_exact(text: &str) -> Option<(usize, &'static str)> {
    // Prefer o200k_base (gpt-4o family); fall back to cl100k_base (gpt-3.5/4).
    use tiktoken_rs::cl100k_base;
    use tiktoken_rs::o200k_base;
    match o200k_base() {
        Ok(bpe) => match bpe.encode_with_special_tokens(text).len() {
            n if n > 0 => Some((n, "o200k_base")),
            _ => None,
        },
        Err(_) => match cl100k_base() {
            Ok(bpe) => Some((bpe.encode_with_special_tokens(text).len(), "cl100k_base")),
            Err(_) => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_simple_text() {
        let t = count("Hello world, this is a prompt.");
        assert_eq!(t.chars, 30);
        assert_eq!(t.words, 6);
        assert!(t.tokens > 0);
    }

    #[test]
    fn counts_unicode() {
        let t = count("你好世界，这是一个提示。");
        // tiktoken (o200k_base) counts ~6 tokens; heuristic gives ~11.
        assert!(t.tokens >= 5, "got {} tokens", t.tokens);
    }

    #[test]
    fn counts_empty() {
        let t = count("");
        assert_eq!(t.tokens, 0);
        assert_eq!(t.chars, 0);
        assert_eq!(t.words, 0);
        assert_eq!(t.lines, 0);
    }

    #[test]
    fn counts_lines() {
        let t = count("line1\nline2\nline3");
        assert_eq!(t.lines, 3);
    }

    #[test]
    fn assumed_output_is_bounded() {
        assert_eq!(assumed_output_tokens(0), 200);
        assert_eq!(assumed_output_tokens(100), 200);
        assert_eq!(assumed_output_tokens(400), 200);
        assert_eq!(assumed_output_tokens(10_000), 2000);
    }

    #[test]
    fn estimate_handles_emoji() {
        let n1 = estimate_tokens("hello world");
        let n2 = estimate_tokens("hello 🚀 world");
        assert!(n2 > n1, "emoji should add tokens: {n1} vs {n2}");
    }
}
