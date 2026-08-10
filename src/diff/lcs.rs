//! Longest-common-subsequence line diff.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    Equal,
    Added,
    Removed,
}

#[derive(Debug, Clone)]
pub struct DiffLine {
    pub kind: DiffKind,
    pub content: String,
}

/// Produce a line-level diff from `old` to `new`.
pub fn diff_lines(old: &[String], new: &[String]) -> Vec<DiffLine> {
    let m = old.len();
    let n = new.len();

    // dp[i][j] = length of LCS of old[i..] and new[j..]
    let mut dp = vec![vec![0u32; n + 1]; m + 1];
    for i in (0..m).rev() {
        for j in (0..n).rev() {
            if old[i] == new[j] {
                dp[i][j] = dp[i + 1][j + 1] + 1;
            } else {
                dp[i][j] = dp[i + 1][j].max(dp[i][j + 1]);
            }
        }
    }

    let mut result = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < m && j < n {
        if old[i] == new[j] {
            result.push(DiffLine {
                kind: DiffKind::Equal,
                content: old[i].clone(),
            });
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            result.push(DiffLine {
                kind: DiffKind::Removed,
                content: old[i].clone(),
            });
            i += 1;
        } else {
            result.push(DiffLine {
                kind: DiffKind::Added,
                content: new[j].clone(),
            });
            j += 1;
        }
    }
    while i < m {
        result.push(DiffLine {
            kind: DiffKind::Removed,
            content: old[i].clone(),
        });
        i += 1;
    }
    while j < n {
        result.push(DiffLine {
            kind: DiffKind::Added,
            content: new[j].clone(),
        });
        j += 1;
    }
    result
}
