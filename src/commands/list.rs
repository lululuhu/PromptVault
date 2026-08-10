use anyhow::Result;

use crate::core::objects::{self, hash_blob, TreeEntry};
use crate::core::repository::Repo;
use crate::ui::printer;

pub fn run() -> Result<()> {
    let repo = Repo::find()?;
    let entries = head_tree_entries(&repo)?;

    if entries.is_empty() {
        printer::warn("no tracked prompts yet");
        return Ok(());
    }

    let mut longest = "path".len();
    for e in &entries {
        if e.path.len() > longest {
            longest = e.path.len();
        }
    }

    println!(
        "{}  {}  {}",
        printer::bold(&pad("HASH", 7)),
        printer::bold(&pad("PATH", longest)),
        printer::bold(&"STATUS".to_string())
    );

    for e in entries {
        let status = working_status(&repo, &e.path, &e.hash);
        println!("{}  {}  {}", &e.hash[..7], pad(&e.path, longest), status);
    }
    Ok(())
}

fn head_tree_entries(repo: &Repo) -> Result<Vec<TreeEntry>> {
    let Some(h) = repo.head_commit()? else {
        return Ok(Vec::new());
    };
    let commit = objects::read_commit(&repo.pv_dir, &h)?;
    Ok(objects::read_tree(&repo.pv_dir, &commit.tree)?)
}

fn pad(s: &str, width: usize) -> String {
    if s.len() >= width {
        s.to_string()
    } else {
        format!("{s:width$}")
    }
}

fn working_status(repo: &Repo, rel: &str, head_hash: &str) -> String {
    let abs = repo.root.join(rel);
    match std::fs::read(&abs) {
        Ok(data) => {
            let cur = hash_blob(&data);
            if cur == head_hash {
                "clean".to_string()
            } else {
                "modified".to_string()
            }
        }
        Err(_) => "deleted".to_string(),
    }
}
