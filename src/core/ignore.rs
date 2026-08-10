//! `.pvignore` support — simple glob patterns (one per line).
//!
//! Supported syntax:
//!   - blank lines and `#` comments are ignored
//!   - `pattern` matches a path if the pattern matches the whole path **or** any
//!     path component (so `drafts` ignores a `drafts/` directory anywhere)
//!   - `*` matches anything except `/`
//!   - leading `/` anchors to the repo root
//!
//! This is intentionally simpler than gitignore — enough for prompt workflows.

use std::path::Path;

#[derive(Debug, Clone)]
pub struct IgnoreSet {
    patterns: Vec<Pattern>,
}

#[derive(Debug, Clone)]
struct Pattern {
    anchored: bool,
    segs: Vec<String>, // path segments, each may contain *
}

impl IgnoreSet {
    /// Load patterns from `<root>/.pvignore` if it exists.
    pub fn load(root: &Path) -> IgnoreSet {
        let path = root.join(".pvignore");
        let Ok(content) = std::fs::read_to_string(&path) else {
            return IgnoreSet { patterns: Vec::new() };
        };
        IgnoreSet::parse(&content)
    }

    pub fn parse(text: &str) -> IgnoreSet {
        let mut patterns = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (anchored, body) = if let Some(rest) = line.strip_prefix('/') {
                (true, rest)
            } else {
                (false, line)
            };
            let segs: Vec<String> = body.split('/').map(|s| s.to_string()).collect();
            patterns.push(Pattern {
                anchored,
                segs,
            });
        }
        IgnoreSet { patterns }
    }

    /// True if `rel_path` (forward-slash, repo-relative) should be ignored.
    pub fn is_ignored(&self, rel_path: &str) -> bool {
        let path_segs: Vec<&str> = rel_path.split('/').collect();
        for p in &self.patterns {
            if matches_pattern(p, &path_segs) {
                return true;
            }
        }
        false
    }
}

fn matches_pattern(p: &Pattern, path_segs: &[&str]) -> bool {
    if p.segs.len() == 1 && !p.anchored {
        // Single-segment, non-anchored: match if any path component matches.
        return path_segs.iter().any(|s| glob_match(&p.segs[0], s));
    }
    if p.anchored {
        // Must match from the start of the path.
        if p.segs.len() > path_segs.len() {
            return false;
        }
        p.segs
            .iter()
            .zip(path_segs.iter())
            .all(|(pat, seg)| glob_match(pat, seg))
    } else {
        // Non-anchored multi-segment: match at any boundary.
        if p.segs.len() > path_segs.len() {
            return false;
        }
        for start in 0..=(path_segs.len() - p.segs.len()) {
            let window = &path_segs[start..start + p.segs.len()];
            if p.segs.iter().zip(window.iter()).all(|(pat, seg)| glob_match(pat, seg)) {
                return true;
            }
        }
        false
    }
}

/// Glob match with `*` (matches any run of non-`/` chars). No `?` or `**`.
///
/// Implemented as an iterative DP over (pattern_pos, text_pos) so that pathological
/// patterns like `*a*a*a*a*` over long strings stay O(P*T) instead of going
/// exponential.
fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let pn = p.len();
    let tn = t.len();

    // dp[i][j] = does p[i..] match t[j..]?
    // Computed bottom-up so each cell is filled exactly once.
    let mut dp = vec![vec![false; tn + 1]; pn + 1];
    dp[pn][tn] = true; // empty pattern matches empty text

    // dp[pn][j>0] = false (empty pattern can't match non-empty text)

    for i in (0..pn).rev() {
        // dp[i][tn] = true only if the rest of the pattern is all '*'
        dp[i][tn] = p[i] == '*' && dp[i + 1][tn];
        for j in (0..tn).rev() {
            if p[i] == '*' {
                // '*' matches zero chars (dp[i+1][j]) or one+ chars (dp[i][j+1]).
                dp[i][j] = dp[i + 1][j] || dp[i][j + 1];
            } else if p[i] == t[j] {
                dp[i][j] = dp[i + 1][j + 1];
            } else {
                dp[i][j] = false;
            }
        }
    }
    dp[0][0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_segment_matches_anywhere() {
        let ig = IgnoreSet::parse("drafts\n");
        assert!(ig.is_ignored("drafts/x.md"));
        assert!(ig.is_ignored("a/b/drafts/y.md"));
        assert!(!ig.is_ignored("prompts/x.md"));
    }

    #[test]
    fn anchored_matches_from_root() {
        let ig = IgnoreSet::parse("/secret\n");
        assert!(ig.is_ignored("secret/x.md"));
        assert!(!ig.is_ignored("a/secret/x.md"));
    }

    #[test]
    fn star_glob() {
        let ig = IgnoreSet::parse("*.bak\n");
        assert!(ig.is_ignored("prompts/x.bak"));
        assert!(ig.is_ignored("y.bak"));
        assert!(!ig.is_ignored("prompts/x.md"));
    }

    #[test]
    fn multi_segment_unanchored() {
        let ig = IgnoreSet::parse("tmp/out\n");
        assert!(ig.is_ignored("tmp/out/x.md"));
        assert!(ig.is_ignored("a/tmp/out/y.md"));
        assert!(!ig.is_ignored("tmp/x.md"));
    }

    #[test]
    fn comments_and_blanks_ignored() {
        let ig = IgnoreSet::parse("# comment\n\n  *.tmp  \n");
        assert!(ig.is_ignored("a.tmp"));
        assert!(!ig.is_ignored("a.md"));
    }
}
