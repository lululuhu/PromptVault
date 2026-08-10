use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use nu_ansi_term::Color;

use crate::core::objects::{self, hash_blob};
use crate::core::repository::Repo;
use crate::render::render;
use crate::ui::printer;

pub fn run(
    prompt: &str,
    dataset: PathBuf,
    strict: bool,
    show: bool,
) -> Result<()> {
    let repo = Repo::find()?;
    let template = load_prompt(&repo, prompt)?;
    let cases = load_dataset(&dataset)?;

    if cases.is_empty() {
        bail!("dataset is empty: {}", dataset.display());
    }

    println!(
        "{} {}  ({} cases)",
        printer::bold("Eval:"),
        prompt,
        cases.len()
    );
    println!();

    let mut passed = 0usize;
    let mut has_assertions = false;

    for (idx, case) in cases.iter().enumerate() {
        let n = idx + 1;
        let total = cases.len();

        // Render the prompt with this case's variables.
        let rendered = match render(&template, case, strict) {
            Ok(r) => r,
            Err(e) => {
                println!(
                    "[{n}/{total}] {}  render error: {e}",
                    Color::Red.paint("ERROR")
                );
                continue;
            }
        };

        // If the case has an `expected` key, assert the rendered prompt contains it.
        if let Some(expected) = case.get("expected") {
            has_assertions = true;
            if rendered.contains(expected.as_str()) {
                println!("[{n}/{total}] {}  contains \"{expected}\"", Color::Green.paint("PASS"));
                passed += 1;
            } else {
                println!(
                    "[{n}/{total}] {}  rendered prompt does not contain \"{expected}\"",
                    Color::Red.paint("FAIL")
                );
            }
        } else {
            // No assertion: just report that it rendered.
            println!("[{n}/{total}] {}  rendered (no assertion)", Color::Cyan.paint("OK"));
            passed += 1;
        }

        if show {
            println!("{}", Color::DarkGray.paint("--- rendered prompt ---"));
            println!("{}", rendered);
            println!("{}", Color::DarkGray.paint("--- end ---"));
        }
    }

    println!();
    if has_assertions {
        let pct = (passed as f64 / cases.len() as f64) * 100.0;
        let summary = format!(
            "Summary: {}/{} passed ({:.0}%)",
            passed,
            cases.len(),
            pct
        );
        if passed == cases.len() {
            printer::ok(&summary);
        } else {
            printer::warn(&summary);
        }
    } else {
        printer::info(&format!("Rendered {} cases (no assertions in dataset)", cases.len()));
    }
    Ok(())
}

/// Resolve the prompt template text from a path, `HEAD:path`, or hash.
fn load_prompt(repo: &Repo, spec: &str) -> Result<String> {
    if let Some(path) = spec.strip_prefix("HEAD:") {
        let path = path.replace('\\', "/");
        let Some(h) = repo.head_commit()? else {
            bail!("no commits yet: cannot resolve HEAD:{path}");
        };
        let commit = objects::read_commit(&repo.pv_dir, &h)?;
        let entries = objects::read_tree(&repo.pv_dir, &commit.tree)?;
        let entry = entries
            .iter()
            .find(|e| e.path == path)
            .with_context(|| format!("path not tracked at HEAD: {path}"))?;
        let blob = objects::read_object(&repo.pv_dir, &entry.hash)?;
        return Ok(String::from_utf8_lossy(&blob.data).to_string());
    }

    // If it's a 64-char or 7+-char hex and exists as an object, treat as blob hash.
    if spec.len() >= 4 && spec.chars().all(|c| c.is_ascii_hexdigit()) {
        if let Some(obj) = try_read_object(&repo, spec)? {
            return Ok(String::from_utf8_lossy(&obj.data).to_string());
        }
    }

    // Otherwise treat as a file path relative to the repo root.
    let abs = repo.root.join(spec);
    let data = std::fs::read_to_string(&abs)
        .with_context(|| format!("cannot read prompt: {spec}"))?;
    Ok(data)
}

fn try_read_object(repo: &Repo, spec: &str) -> Result<Option<objects::Object>> {
    let (dir, file_prefix) = spec.split_at(2);
    let dir_path = repo.pv_dir.join("objects").join(dir);
    if !dir_path.exists() {
        return Ok(None);
    }
    for entry in std::fs::read_dir(&dir_path)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(file_prefix) {
            let hash = format!("{dir}{name}");
            let obj = objects::read_object(&repo.pv_dir, &hash)?;
            return Ok(Some(obj));
        }
    }
    Ok(None)
}

/// Load a JSON Lines dataset. Each line is a JSON object mapping var names to values.
/// A special key `expected` (if present) is used as the assertion.
fn load_dataset(path: &PathBuf) -> Result<Vec<HashMap<String, String>>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read dataset: {}", path.display()))?;
    let mut cases = Vec::new();
    for (i, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(line)
            .with_context(|| format!("line {}: invalid JSON", i + 1))?;
        let obj = v.as_object().context("each case must be a JSON object")?;
        let mut case: HashMap<String, String> = HashMap::new();
        for (k, val) in obj {
            let s = match val {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            case.insert(k.clone(), s);
        }
        cases.push(case);
    }
    Ok(cases)
}

// Used by `status`/`list` to detect working-tree changes — re-exported for consistency.
#[allow(dead_code)]
pub fn _blob_hash_of(data: &[u8]) -> String {
    hash_blob(data)
}
