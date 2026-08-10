//! `pv ab` — render two prompt versions against the same dataset and diff them.
//!
//! Pure local: no model calls, no network. For each case in the dataset, both
//! templates are rendered and compared line-by-line so you can see exactly how
//! two prompt variants differ when filled with the same variables.
//!
//! Usage:
//!   pv ab <prompt-a> <prompt-b> --dataset <file> [--strict] [--show]
//!
//! Each prompt is a `ref:path` spec (e.g. `main:summarize.md`, `experiment:summarize.md`,
//! `HEAD:summarize.md`, or a plain path).

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use nu_ansi_term::Color;

use crate::core::prompt_ref;
use crate::core::repository::Repo;
use crate::diff::{diff_lines, split_lines, DiffKind};
use crate::render::render;
use crate::ui::printer;

pub fn run(a: &str, b: &str, dataset: PathBuf, strict: bool, show: bool) -> Result<()> {
    let repo = Repo::find()?;
    let template_a = prompt_ref::load_prompt(&repo, a)?;
    let template_b = prompt_ref::load_prompt(&repo, b)?;
    let cases = load_dataset(&dataset)?;

    if cases.is_empty() {
        bail!("dataset is empty: {}", dataset.display());
    }

    println!(
        "{}  A={}  B={}  ({} cases)",
        printer::bold("A/B:"),
        a,
        b,
        cases.len()
    );
    println!();

    let mut identical = 0usize;
    let mut differing = 0usize;

    for (idx, case) in cases.iter().enumerate() {
        let n = idx + 1;
        let total = cases.len();

        let rendered_a = match render(&template_a, case, strict) {
            Ok(r) => r,
            Err(e) => {
                println!("[{n}/{total}] {}  A render error: {e}", Color::Red.paint("ERROR"));
                continue;
            }
        };
        let rendered_b = match render(&template_b, case, strict) {
            Ok(r) => r,
            Err(e) => {
                println!("[{n}/{total}] {}  B render error: {e}", Color::Red.paint("ERROR"));
                continue;
            }
        };

        if rendered_a == rendered_b {
            identical += 1;
            println!("[{n}/{total}] {}  identical", Color::DarkGray.paint("SAME"));
        } else {
            differing += 1;
            println!("[{n}/{total}] {}  differs", Color::Yellow.paint("DIFF"));
            if show {
                let lines_a = split_lines(&rendered_a);
                let lines_b = split_lines(&rendered_b);
                let d = diff_lines(&lines_a, &lines_b);
                println!("{}", Color::DarkGray.paint("--- A vs B ---"));
                for dl in d {
                    let (prefix, color) = match dl.kind {
                        DiffKind::Equal => (" ", Color::DarkGray),
                        DiffKind::Added => ("+B", Color::Green),
                        DiffKind::Removed => ("-A", Color::Red),
                    };
                    println!("{} {}", color.paint(prefix), color.paint(&dl.content));
                }
                println!("{}", Color::DarkGray.paint("--- end ---"));
            }
        }
    }

    println!();
    let summary = format!(
        "Summary: {identical} identical, {differing} differing (of {})",
        cases.len()
    );
    if differing == 0 {
        printer::warn(&summary);
        printer::info("both versions render identically — nothing to A/B test");
    } else {
        printer::ok(&summary);
    }
    Ok(())
}

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
