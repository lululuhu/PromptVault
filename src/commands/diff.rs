use anyhow::{bail, Result};
use nu_ansi_term::Color;

use crate::core::objects::{self, TreeEntry};
use crate::core::refs;
use crate::core::repository::Repo;
use crate::diff::{diff_lines, split_lines, DiffKind};
use crate::ui::printer;

/// `pv diff`           — working tree vs HEAD
/// `pv diff <ref>`     — working tree vs <ref>
/// `pv diff <a> <b>`   — tree of <a> vs tree of <b>
///
/// `<ref>` may be a commit hash (or prefix), tag, branch, or `HEAD`.
pub fn run(args: Vec<String>) -> Result<()> {
    let repo = Repo::find()?;

    match args.len() {
        0 => diff_worktree_vs(&repo, "HEAD", None),
        1 => diff_worktree_vs(&repo, &args[0], None),
        2 => diff_refs(&repo, &args[0], &args[1]),
        _ => bail!("too many arguments: `pv diff [a] [b]` takes at most 2"),
    }
}

/// Diff the working tree against the tree of `ref_spec`, optionally restricted to `filter`.
fn diff_worktree_vs(repo: &Repo, ref_spec: &str, filter: Option<&str>) -> Result<()> {
    let base_entries = resolve_tree_entries(repo, ref_spec)?.unwrap_or_default();
    let mut printed = false;
    for e in &base_entries {
        if let Some(f) = filter {
            if e.path != f {
                continue;
            }
        }
        if print_diff_blob_vs_worktree(repo, &e.path, Some(&e.hash))? {
            printed = true;
        }
    }
    // Allow diffing a single untracked file (everything shows as added).
    if let Some(f) = filter {
        let tracked = base_entries.iter().any(|e| e.path == f);
        if !tracked {
            if print_diff_blob_vs_worktree(repo, f, None)? {
                printed = true;
            }
        }
    }
    if !printed {
        printer::info("no changes");
    }
    Ok(())
}

/// Diff the tree of `a_spec` against the tree of `b_spec`.
fn diff_refs(repo: &Repo, a_spec: &str, b_spec: &str) -> Result<()> {
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

    let mut printed = false;
    for p in paths {
        let ah = a_map.get(p);
        let bh = b_map.get(p);
        if ah == bh {
            continue;
        }
        let old_lines = blob_lines(repo, ah);
        let new_lines = blob_lines(repo, bh);
        println!(
            "{}",
            printer::bold(&format!("diff -- {a_spec}:{p} {b_spec}:{p}"))
        );
        print_diff_lines(&old_lines, &new_lines);
        println!();
        printed = true;
    }
    if !printed {
        printer::info(&format!("no differences between {a_spec} and {b_spec}"));
    }
    Ok(())
}

fn blob_lines(repo: &Repo, hash: Option<&String>) -> Vec<String> {
    match hash {
        Some(h) => {
            let obj = match objects::read_object(&repo.pv_dir, h) {
                Ok(o) => o,
                Err(_) => return Vec::new(),
            };
            split_lines(&String::from_utf8_lossy(&obj.data))
        }
        None => Vec::new(),
    }
}

fn print_diff_blob_vs_worktree(repo: &Repo, rel: &str, old_hash: Option<&str>) -> Result<bool> {
    let abs = repo.root.join(rel);
    let new_data = std::fs::read_to_string(&abs).unwrap_or_default();
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
    if spec.len() >= 4 && spec.chars().all(|c| c.is_ascii_hexdigit()) {
        let (dir, file_prefix) = spec.split_at(2);
        let dir_path = repo.pv_dir.join("objects").join(dir);
        if dir_path.exists() {
            for entry in std::fs::read_dir(&dir_path)? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with(file_prefix) {
                    return Ok(Some(format!("{dir}{name}")));
                }
            }
        }
    }
    Ok(None)
}
