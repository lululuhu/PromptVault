use anyhow::{bail, Context, Result};
use nu_ansi_term::Color;

use crate::core::objects::{self, TreeEntry};
use crate::core::refs;
use crate::core::repository::Repo;
use crate::diff::{diff_lines, split_lines, DiffKind};
use crate::ui::printer;

/// `pv diff`           — working tree vs HEAD
/// `pv diff <ref>`     — working tree vs <ref>
/// `pv diff <a> <b>`   — tree of <a> vs tree of <b>
/// `pv diff --stat`    — summary only (which files changed, +/- counts)
///
/// `<ref>` may be a commit hash (or prefix), tag, branch, or `HEAD`.
pub fn run(args: Vec<String>, stat: bool) -> Result<()> {
    let repo = Repo::find()?;

    match args.len() {
        0 => diff_worktree_vs(&repo, "HEAD", None, stat),
        1 => diff_worktree_vs(&repo, &args[0], None, stat),
        2 => diff_refs(&repo, &args[0], &args[1], stat),
        _ => bail!("too many arguments: `pv diff [a] [b]` takes at most 2"),
    }
}

/// Diff the working tree against the tree of `ref_spec`, optionally restricted to `filter`.
fn diff_worktree_vs(repo: &Repo, ref_spec: &str, filter: Option<&str>, stat: bool) -> Result<()> {
    let base_entries = resolve_tree_entries(repo, ref_spec)?.unwrap_or_default();
    let mut stats: Vec<FileStat> = Vec::new();
    let mut printed = false;
    for e in &base_entries {
        if let Some(f) = filter {
            if e.path != f {
                continue;
            }
        }
        if stat {
            if let Some(s) = stat_diff_blob_vs_worktree(repo, &e.path, Some(&e.hash))? {
                stats.push(s);
                printed = true;
            }
        } else if print_diff_blob_vs_worktree(repo, &e.path, Some(&e.hash))? {
            printed = true;
        }
    }
    // Allow diffing a single untracked file (everything shows as added).
    if let Some(f) = filter {
        let tracked = base_entries.iter().any(|e| e.path == f);
        if !tracked {
            if stat {
                if let Some(s) = stat_diff_blob_vs_worktree(repo, f, None)? {
                    stats.push(s);
                    printed = true;
                }
            } else if print_diff_blob_vs_worktree(repo, f, None)? {
                printed = true;
            }
        }
    }
    if stat {
        print_stat_summary(&stats);
    }
    if !printed {
        printer::info("no changes");
    }
    Ok(())
}

/// Diff the tree of `a_spec` against the tree of `b_spec`.
fn diff_refs(repo: &Repo, a_spec: &str, b_spec: &str, stat: bool) -> Result<()> {
    let a = resolve_tree_entries(repo, a_spec)?
        .ok_or_else(|| anyhow::anyhow!("could not resolve '{a_spec}' to a commit"))?;
    let b = resolve_tree_entries(repo, b_spec)?
        .ok_or_else(|| anyhow::anyhow!("could not resolve '{b_spec}' to a commit"))?;

    let a_map: std::collections::HashMap<String, String> =
        a.iter().cloned().map(|e| (e.path, e.hash)).collect();
    let b_map: std::collections::HashMap<String, String> =
        b.iter().cloned().map(|e| (e.path, e.hash)).collect();

    let mut paths: Vec<&String> = a_map.keys().chain(b_map.keys()).collect();
    paths.sort();
    paths.dedup();

    let mut stats: Vec<FileStat> = Vec::new();
    let mut printed = false;
    for p in paths {
        let ah = a_map.get(p);
        let bh = b_map.get(p);
        if ah == bh {
            continue;
        }
        let old_lines = blob_lines(repo, ah)?;
        let new_lines = blob_lines(repo, bh)?;
        if stat {
            let s = stat_for(p, &old_lines, &new_lines);
            stats.push(s);
            printed = true;
        } else {
            println!(
                "{}",
                printer::bold(&format!("diff -- {a_spec}:{p} {b_spec}:{p}"))
            );
            print_diff_lines(&old_lines, &new_lines);
            println!();
            printed = true;
        }
    }
    if stat {
        print_stat_summary(&stats);
    }
    if !printed {
        printer::info(&format!("no differences between {a_spec} and {b_spec}"));
    }
    Ok(())
}

fn blob_lines(repo: &Repo, hash: Option<&String>) -> Result<Vec<String>> {
    match hash {
        Some(h) => {
            let obj = objects::read_object(&repo.pv_dir, h)
                .with_context(|| format!("failed to read blob {h} for diff"))?;
            Ok(split_lines(&String::from_utf8_lossy(&obj.data)))
        }
        None => Ok(Vec::new()),
    }
}

fn print_diff_blob_vs_worktree(repo: &Repo, rel: &str, old_hash: Option<&str>) -> Result<bool> {
    let abs = repo.root.join(rel);
    // A missing file (deleted from working tree) is a legitimate diff state —
    // treat it as empty. Other read errors (permission denied, etc.) are real
    // and must be surfaced, not silently swallowed.
    let new_data = match std::fs::read_to_string(&abs) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).context(format!("failed to read {rel} from working tree")),
    };
    let new_lines = split_lines(&new_data);

    let old_lines = if let Some(h) = old_hash {
        let obj = objects::read_object(&repo.pv_dir, h)?;
        split_lines(&String::from_utf8_lossy(&obj.data))
    } else {
        Vec::new()
    };

    if old_lines == new_lines {
        return Ok(false);
    }

    println!("{}", printer::bold(&format!("diff -- {rel}")));
    print_diff_lines(&old_lines, &new_lines);
    println!();
    Ok(true)
}

/// Like `print_diff_blob_vs_worktree` but only computes +/- counts, no output.
fn stat_diff_blob_vs_worktree(repo: &Repo, rel: &str, old_hash: Option<&str>) -> Result<Option<FileStat>> {
    let abs = repo.root.join(rel);
    let new_data = match std::fs::read_to_string(&abs) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).context(format!("failed to read {rel} from working tree")),
    };
    let new_lines = split_lines(&new_data);

    let old_lines = if let Some(h) = old_hash {
        let obj = objects::read_object(&repo.pv_dir, h)?;
        split_lines(&String::from_utf8_lossy(&obj.data))
    } else {
        Vec::new()
    };

    if old_lines == new_lines {
        return Ok(None);
    }
    let (added, removed) = count_changes(&old_lines, &new_lines);
    Ok(Some(FileStat {
        path: rel.to_string(),
        added,
        removed,
    }))
}

fn print_diff_lines(old: &[String], new: &[String]) {
    let d = diff_lines(old, new);
    for dl in d {
        let (prefix, color) = match dl.kind {
            DiffKind::Equal => (" ", Color::DarkGray),
            DiffKind::Added => ("+", Color::Green),
            DiffKind::Removed => ("-", Color::Red),
        };
        println!("{}{}", color.paint(prefix), color.paint(&dl.content));
    }
}

#[derive(Debug, Clone)]
struct FileStat {
    path: String,
    added: usize,
    removed: usize,
}

fn stat_for(path: &str, old: &[String], new: &[String]) -> FileStat {
    let (added, removed) = count_changes(old, new);
    FileStat {
        path: path.to_string(),
        added,
        removed,
    }
}

fn count_changes(old: &[String], new: &[String]) -> (usize, usize) {
    let d = diff_lines(old, new);
    let mut added = 0;
    let mut removed = 0;
    for dl in d {
        match dl.kind {
            DiffKind::Added => added += 1,
            DiffKind::Removed => removed += 1,
            DiffKind::Equal => {}
        }
    }
    (added, removed)
}

fn print_stat_summary(stats: &[FileStat]) {
    if stats.is_empty() {
        return;
    }
    let max_path = stats.iter().map(|s| s.path.len()).max().unwrap_or(0);
    let mut total_added = 0;
    let mut total_removed = 0;
    for s in stats {
        let bar = format_bar(s.added, s.removed);
        println!(
            " {:<width$} | {:>4} {}",
            s.path,
            format!("{}{}", format!("+{}", s.added), format!("-{}", s.removed)),
            bar,
            width = max_path
        );
        total_added += s.added;
        total_removed += s.removed;
    }
    println!();
    println!(
        " {} file{} changed, {} insertion{}(+), {} deletion{}(-)",
        stats.len(),
        if stats.len() == 1 { "" } else { "s" },
        total_added,
        if total_added == 1 { "" } else { "s" },
        total_removed,
        if total_removed == 1 { "" } else { "s" },
    );
}

fn format_bar(added: usize, removed: usize) -> String {
    let plus = "+".repeat(added.min(30));
    let minus = "-".repeat(removed.min(30));
    format!("{}{}", Color::Green.paint(&plus), Color::Red.paint(&minus))
}

/// Resolve a ref spec (hash/prefix, tag, branch, HEAD) to its tree entries.
fn resolve_tree_entries(repo: &Repo, spec: &str) -> Result<Option<Vec<TreeEntry>>> {
    let Some(commit_hash) = resolve_commit(repo, spec)? else {
        return Ok(None);
    };
    let commit = objects::read_commit(&repo.pv_dir, &commit_hash)?;
    Ok(Some(objects::read_tree(&repo.pv_dir, &commit.tree)?))
}

fn resolve_commit(repo: &Repo, spec: &str) -> Result<Option<String>> {
    if spec == "HEAD" {
        return repo.head_commit();
    }
    if let Some(h) = refs::resolve_tag(&repo.pv_dir, spec)? {
        return Ok(Some(h));
    }
    if let Some(h) = refs::resolve_branch(&repo.pv_dir, spec)? {
        return Ok(Some(h));
    }
    crate::core::safe::resolve_hash_prefix(&repo.pv_dir.join("objects"), spec)
}
