//! Line diff using Myers' algorithm.
//!
//! O((N+M)D) time and O(N+M) memory in the common case, where D is the edit
//! distance. Far better than the O(N*M) memory of a full LCS DP table for
//! large inputs — a 100k-line diff no longer allocates ~40GB.

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

/// Produce a line-level diff from `old` to `new` using Myers' diff.
pub fn diff_lines(old: &[String], new: &[String]) -> Vec<DiffLine> {
    // Trim common prefix and suffix to shrink the problem (Myers' standard optimization).
    let mut prefix = 0;
    while prefix < old.len() && prefix < new.len() && old[prefix] == new[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix + prefix < old.len()
        && suffix + prefix < new.len()
        && old[old.len() - 1 - suffix] == new[new.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let a = &old[prefix..old.len() - suffix];
    let b = &new[prefix..new.len() - suffix];

    let mut result = Vec::with_capacity(old.len() + new.len());

    // Common prefix.
    for i in 0..prefix {
        result.push(DiffLine {
            kind: DiffKind::Equal,
            content: old[i].clone(),
        });
    }

    // Middle edit (Myers).
    let edits = myers_diff(a, b);
    for e in edits {
        match e {
            Edit::Equal(i) => result.push(DiffLine {
                kind: DiffKind::Equal,
                content: a[i].clone(),
            }),
            Edit::Remove(i) => result.push(DiffLine {
                kind: DiffKind::Removed,
                content: a[i].clone(),
            }),
            Edit::Add(j) => result.push(DiffLine {
                kind: DiffKind::Added,
                content: b[j].clone(),
            }),
        }
    }

    // Common suffix.
    for i in 0..suffix {
        result.push(DiffLine {
            kind: DiffKind::Equal,
            content: old[old.len() - suffix + i].clone(),
        });
    }

    result
}

enum Edit {
    Equal(usize),  // index into a
    Remove(usize), // index into a
    Add(usize),    // index into b
}

/// Myers' O((N+M)D) diff. Returns the edit script for the trimmed slices a, b.
fn myers_diff(a: &[String], b: &[String]) -> Vec<Edit> {
    let n = a.len();
    let m = b.len();
    if n == 0 && m == 0 {
        return Vec::new();
    }
    if n == 0 {
        return (0..m).map(Edit::Add).collect();
    }
    if m == 0 {
        return (0..n).map(Edit::Remove).collect();
    }

    let max = n + m;
    // V[k] holds the furthest x reached on diagonal k. Offset by `max` for negative indices.
    let mut v = vec![0i64; 2 * max + 1];
    let mut trace: Vec<Vec<i64>> = Vec::new();

    let mut d = 0;
    'outer: while d <= max as i64 {
        trace.push(v.clone());
        for k in (-d..=d).step_by(2) {
            let idx = (k + max as i64) as usize;
            // Choose whether to go down (insert) or right (delete).
            let mut x = if k == -d
                || (k != d
                    && v[(k - 1 + max as i64) as usize] < v[(k + 1 + max as i64) as usize])
            {
                v[(k + 1 + max as i64) as usize] // down: insert
            } else {
                v[(k - 1 + max as i64) as usize] + 1 // right: delete
            };
            let mut y = x - k;
            // Slide down any matching diagonal.
            while (x as usize) < n && (y as usize) < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[idx] = x;
            if (x as usize) >= n && (y as usize) >= m {
                break 'outer;
            }
        }
        d += 1;
    }

    // Backtrack to build the edit script.
    let mut script = Vec::new();
    let mut x = n as i64;
    let mut y = m as i64;
    for (d, vs) in trace.iter().enumerate().rev() {
        let k = x - y;
        let prev_k = if k == -(d as i64)
            || (k != d as i64
                && vs[(k - 1 + max as i64) as usize] < vs[(k + 1 + max as i64) as usize])
        {
            k + 1
        } else {
            k - 1
        };
        let prev_x = vs[(prev_k + max as i64) as usize];
        let prev_y = prev_x - prev_k;

        // Snake (matches) between (prev_x_or_after_corner, prev_y..) and (x, y).
        while x > prev_x && y > prev_y {
            script.push(Edit::Equal((x - 1) as usize));
            x -= 1;
            y -= 1;
        }
        if d > 0 {
            // The corner step that connects prev to the snake.
            if x == prev_x {
                // Came from above: an insertion in b.
                script.push(Edit::Add((y - 1) as usize));
                y -= 1;
            } else {
                // Came from the left: a deletion from a.
                script.push(Edit::Remove((x - 1) as usize));
                x -= 1;
            }
        }
    }
    script.reverse();
    script
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|s| s.to_string()).collect()
    }

    fn kinds(diff: &[DiffLine]) -> Vec<&str> {
        diff.iter()
            .map(|d| match d.kind {
                DiffKind::Equal => " ",
                DiffKind::Added => "+",
                DiffKind::Removed => "-",
            })
            .collect()
    }

    #[test]
    fn identical() {
        let a = s(&["a", "b", "c"]);
        let d = diff_lines(&a, &a);
        assert_eq!(kinds(&d), vec![" ", " ", " "]);
    }

    #[test]
    fn all_added() {
        let d = diff_lines(&[], &s(&["a", "b"]));
        assert_eq!(kinds(&d), vec!["+", "+"]);
    }

    #[test]
    fn all_removed() {
        let d = diff_lines(&s(&["a", "b"]), &[]);
        assert_eq!(kinds(&d), vec!["-", "-"]);
    }

    #[test]
    fn simple_edit() {
        let a = s(&["a", "b", "c"]);
        let b = s(&["a", "B", "c"]);
        let d = diff_lines(&a, &b);
        // Common prefix "a" + common suffix "c" are trimmed; middle is -b / +B.
        assert_eq!(kinds(&d), vec![" ", "-", "+", " "]);
        assert_eq!(d[1].content, "b");
        assert_eq!(d[2].content, "B");
    }

    #[test]
    fn preserves_content() {
        let a = s(&["line1", "line2", "line3"]);
        let b = s(&["line1", "modified", "line3", "line4"]);
        let d = diff_lines(&a, &b);
        // Reconstruct a from removed+equal, b from added+equal.
        let recon_a: Vec<&str> = d
            .iter()
            .filter(|d| d.kind != DiffKind::Added)
            .map(|d| d.content.as_str())
            .collect();
        let recon_b: Vec<&str> = d
            .iter()
            .filter(|d| d.kind != DiffKind::Removed)
            .map(|d| d.content.as_str())
            .collect();
        assert_eq!(recon_a, vec!["line1", "line2", "line3"]);
        assert_eq!(recon_b, vec!["line1", "modified", "line3", "line4"]);
    }

    #[test]
    fn empty_both() {
        let d = diff_lines(&[], &[]);
        assert!(d.is_empty());
    }

    #[test]
    fn large_input_does_not_oom() {
        // 50k lines — would be ~10GB with the old O(N*M) DP table.
        let a: Vec<String> = (0..50_000).map(|i| format!("line {i}")).collect();
        let mut b = a.clone();
        b[25_000] = "CHANGED".to_string();
        let d = diff_lines(&a, &b);
        // Should still produce a sane diff: prefix + suffix equal, middle -/+ .
        assert!(d.iter().any(|d| d.kind == DiffKind::Removed));
        assert!(d.iter().any(|d| d.kind == DiffKind::Added));
        assert!(d.iter().any(|d| d.kind == DiffKind::Equal));
    }
}
