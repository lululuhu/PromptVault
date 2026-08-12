//! `pv eval` — evaluate a prompt template against a dataset.
//!
//! Three modes (progressively more powerful):
//!
//! 1. **Render-only** (default, no `run` feature needed):
//!    Renders `{{vars}}` and asserts the rendered prompt contains `expected`.
//!
//! 2. **LLM-output** (`--llm <provider>`, requires `run` feature):
//!    Renders the prompt, sends it to the LLM, then asserts the LLM's
//!    *output* contains `expected_output`. This catches prompt regressions
//!    that only surface at the model layer.
//!
//! 3. **LLM-judge** (`--llm <provider> --judge`, requires `run` feature):
//!    Renders the prompt, sends it to the LLM, then asks the same (or a
//!    different) LLM to score the output 0-10 against a `rubric`. Produces
//!    a final mean score — the prompt's quality signal.
//!
//! Dataset JSONL line shapes (all keys optional except vars used by template):
//!
//! ```jsonl
//! {"input":"x","expected":"rendered contains this"}
//! {"input":"x","expected_output":"LLM output contains this"}
//! {"input":"x","rubric":"must be concise, factual, and cite sources"}
//! ```

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use nu_ansi_term::Color;
use serde::Serialize;

use crate::core::objects;
use crate::core::repository::Repo;
use crate::render::render;
use crate::ui::printer;

// ---------------------------------------------------------------------------
// Eval history recording
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct EvalRecord {
    ts: i64,
    mode: &'static str,
    prompt: String,
    total: usize,
    passed: usize,
    mean_score: Option<f64>,
    cases: Vec<CaseRecord>,
}

#[derive(Serialize)]
struct CaseRecord {
    idx: usize,
    status: &'static str,
    score: Option<f64>,
}

/// Append an eval record to `.pv/evals/<slug>.jsonl`. One file per prompt,
/// append-only. Failures here are logged to stderr but never abort the eval.
fn record_eval(repo: &Repo, prompt: &str, record: &EvalRecord) {
    let dir = repo.pv_dir.join("evals");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let slug = slugify(prompt);
    let path = dir.join(format!("{slug}.jsonl"));
    let mut line = match serde_json::to_string(record) {
        Ok(s) => s,
        Err(_) => return,
    };
    line.push('\n');
    if std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()))
        .is_err()
    {
        eprintln!("warn: failed to record eval to {}", path.display());
    }
}

fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Read all eval records for a prompt. Newest last (append order).
pub fn read_history(repo: &Repo, prompt: &str) -> Result<Vec<serde_json::Value>> {
    let slug = slugify(prompt);
    let path = repo.pv_dir.join("evals").join(format!("{slug}.jsonl"));
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&path)?;
    let mut out = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            out.push(v);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Render-only eval (always available). Backward-compatible with v0.2.
pub fn run(prompt: &str, dataset: PathBuf, strict: bool, show: bool, record: bool) -> Result<()> {
    let repo = Repo::find()?;
    let template = load_prompt(&repo, prompt)?;
    let cases = load_dataset(&dataset)?;

    if cases.is_empty() {
        bail!("dataset is empty: {}", dataset.display());
    }

    println!(
        "{} {}  ({} cases, render-only)",
        printer::bold("Eval:"),
        prompt,
        cases.len()
    );
    println!();

    let mut passed = 0usize;
    let mut has_assertions = false;
    let mut case_records: Vec<CaseRecord> = Vec::new();

    for (idx, case) in cases.iter().enumerate() {
        let n = idx + 1;
        let total = cases.len();

        let rendered = match render(&template, case, strict) {
            Ok(r) => r,
            Err(e) => {
                println!(
                    "[{n}/{total}] {}  render error: {e}",
                    Color::Red.paint("ERROR")
                );
                case_records.push(CaseRecord { idx, status: "error", score: None });
                continue;
            }
        };

        if let Some(expected) = case.get("expected") {
            has_assertions = true;
            if rendered.contains(expected.as_str()) {
                println!("[{n}/{total}] {}  contains \"{expected}\"", Color::Green.paint("PASS"));
                passed += 1;
                case_records.push(CaseRecord { idx, status: "pass", score: None });
            } else {
                println!(
                    "[{n}/{total}] {}  rendered prompt does not contain \"{expected}\"",
                    Color::Red.paint("FAIL")
                );
                case_records.push(CaseRecord { idx, status: "fail", score: None });
            }
        } else {
            println!("[{n}/{total}] {}  rendered (no assertion)", Color::Cyan.paint("OK"));
            passed += 1;
            case_records.push(CaseRecord { idx, status: "ok", score: None });
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
        let summary = format!("Summary: {}/{} passed ({:.0}%)", passed, cases.len(), pct);
        if passed == cases.len() {
            printer::ok(&summary);
        } else {
            printer::warn(&summary);
        }
    } else {
        printer::info(&format!("Rendered {} cases (no assertions in dataset)", cases.len()));
    }

    if record {
        let rec = EvalRecord {
            ts: chrono::Utc::now().timestamp(),
            mode: "render",
            prompt: prompt.to_string(),
            total: cases.len(),
            passed,
            mean_score: None,
            cases: case_records,
        };
        record_eval(&repo, prompt, &rec);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// LLM-backed eval (only compiled with the `run` feature)
// ---------------------------------------------------------------------------

#[cfg(feature = "run")]
mod llm {
    use super::*;
    use crate::commands::run::Provider;

    /// LLM eval: render → call LLM → assert output / judge rubric.
    pub fn run(
        prompt: &str,
        dataset: PathBuf,
        strict: bool,
        show: bool,
        provider_str: &str,
        model: Option<&str>,
        judge: bool,
        record: bool,
    ) -> Result<()> {
        let repo = Repo::find()?;
        let provider = Provider::parse(provider_str)?;
        let template = load_prompt(&repo, prompt)?;
        let cases = load_dataset(&dataset)?;

        if cases.is_empty() {
            bail!("dataset is empty: {}", dataset.display());
        }

        let mode = if judge { "LLM + judge" } else { "LLM-output" };
        println!(
            "{} {}  ({} cases, {})",
            printer::bold("Eval:"),
            prompt,
            cases.len(),
            mode
        );
        println!();

        printer::warn(&format!(
            "Will call provider '{}' for {} cases. Each case = 1 model call{}.",
            provider.as_str(),
            cases.len(),
            if judge { " + 1 judge call" } else { "" }
        ));
        println!();

        let mut passed = 0usize;
        let mut judge_scores: Vec<f64> = Vec::new();
        let mut case_records: Vec<CaseRecord> = Vec::new();

        for (idx, case) in cases.iter().enumerate() {
            let n = idx + 1;
            let total = cases.len();

            let rendered = match render(&template, case, strict) {
                Ok(r) => r,
                Err(e) => {
                    println!(
                        "[{n}/{total}] {}  render error: {e}",
                        Color::Red.paint("ERROR")
                    );
                    case_records.push(CaseRecord { idx, status: "error", score: None });
                    continue;
                }
            };

            // Call the LLM.
            let output = match call_provider(&provider, &rendered, model, None) {
                Ok(o) => o,
                Err(e) => {
                    println!(
                        "[{n}/{total}] {}  LLM error: {e}",
                        Color::Red.paint("ERROR")
                    );
                    case_records.push(CaseRecord { idx, status: "error", score: None });
                    continue;
                }
            };

            if show {
                println!("{}", Color::DarkGray.paint("--- rendered prompt ---"));
                println!("{}", rendered);
                println!("{}", Color::DarkGray.paint("--- LLM output ---"));
                println!("{}", output);
                println!("{}", Color::DarkGray.paint("--- end ---"));
            }

            // Assertion on LLM output.
            if let Some(expected_output) = case.get("expected_output") {
                if output.contains(expected_output.as_str()) {
                    println!(
                        "[{n}/{total}] {}  output contains \"{}\"",
                        Color::Green.paint("PASS"),
                        truncate(expected_output, 40)
                    );
                    passed += 1;
                    case_records.push(CaseRecord { idx, status: "pass", score: None });
                } else {
                    println!(
                        "[{n}/{total}] {}  output missing \"{}\"",
                        Color::Red.paint("FAIL"),
                        truncate(expected_output, 40)
                    );
                    case_records.push(CaseRecord { idx, status: "fail", score: None });
                }
            } else if !judge {
                // No assertion, no judge: just report it ran.
                println!("[{n}/{total}] {}  LM responded", Color::Cyan.paint("OK"));
                passed += 1;
                case_records.push(CaseRecord { idx, status: "ok", score: None });
            }

            // LLM-as-judge scoring.
            if judge {
                if let Some(rubric) = case.get("rubric") {
                    match judge_output(&provider, &rendered, &output, rubric, model) {
                        Ok(score) => {
                            judge_scores.push(score);
                            let badge = if score >= 8.0 {
                                Color::Green.paint(format!("judge: {score}/10"))
                            } else if score >= 5.0 {
                                Color::Yellow.paint(format!("judge: {score}/10"))
                            } else {
                                Color::Red.paint(format!("judge: {score}/10"))
                            };
                            println!("[{n}/{total}] {}  rubric: \"{}\"", badge, truncate(rubric, 40));
                            case_records.push(CaseRecord { idx, status: "judged", score: Some(score) });
                        }
                        Err(e) => {
                            println!(
                                "[{n}/{total}] {}  judge error: {e}",
                                Color::Red.paint("ERROR")
                            );
                            case_records.push(CaseRecord { idx, status: "error", score: None });
                        }
                    }
                } else {
                    println!(
                        "[{n}/{total}] {}  no rubric — skipped judging",
                        Color::DarkGray.paint("SKIP")
                    );
                    case_records.push(CaseRecord { idx, status: "skip", score: None });
                }
            }
        }

        println!();
        let mean_score = if judge && !judge_scores.is_empty() {
            let mean = judge_scores.iter().sum::<f64>() / judge_scores.len() as f64;
            let summary = format!(
                "Judge mean: {mean:.2}/10  ({} cases judged, {} asserted)",
                judge_scores.len(),
                passed
            );
            if mean >= 8.0 {
                printer::ok(&summary);
            } else if mean >= 5.0 {
                printer::warn(&summary);
            } else {
                printer::error(&summary);
            }
            Some(mean)
        } else {
            let pct = (passed as f64 / cases.len() as f64) * 100.0;
            let summary = format!("Summary: {}/{} passed ({:.0}%)", passed, cases.len(), pct);
            if passed == cases.len() {
                printer::ok(&summary);
            } else {
                printer::warn(&summary);
            }
            None
        };

        if record {
            let mode_str: &'static str = if judge { "judge" } else { "llm" };
            let rec = EvalRecord {
                ts: chrono::Utc::now().timestamp(),
                mode: mode_str,
                prompt: prompt.to_string(),
                total: cases.len(),
                passed,
                mean_score,
                cases: case_records,
            };
            record_eval(&repo, prompt, &rec);
        }
        Ok(())
    }

    /// Call a provider and return the text output. Reuses `commands::run` internals.
    fn call_provider(
        provider: &Provider,
        prompt: &str,
        model: Option<&str>,
        max_tokens: Option<u32>,
    ) -> Result<String> {
        // We reach into the run module's private functions via a tiny shim
        // because they're not exported. The simplest path is to re-call through
        // the public `run::run` is wrong (it prints). Instead we duplicate the
        // HTTP call surface here — but to avoid divergence we expose them.
        //
        // The run module already exposes `Provider`; the call_* fns are private.
        // We re-implement the dispatch using its public surface by calling
        // `commands::run::run` with show_prompt=false would print the response.
        // Cleanest: add a `pub` helper in run.rs. For now, inline the dispatch.
        crate::commands::run::dispatch(provider, prompt, model, max_tokens)
    }

    /// Ask the LLM to score `output` against `rubric` on a 0-10 scale.
    fn judge_output(
        provider: &Provider,
        prompt: &str,
        output: &str,
        rubric: &str,
        model: Option<&str>,
    ) -> Result<f64> {
        let judge_prompt = format!(
            "You are a strict prompt evaluator. Score the assistant's response on a 0-10 scale.\n\
             \n\
             Evaluation rubric:\n{rubric}\n\
             \n\
             --- Original prompt given to the assistant ---\n{prompt}\n\
             \n\
             --- Assistant's response ---\n{output}\n\
             \n\
             Reply with ONLY a single integer 0-10. No explanation, no text, no punctuation."
        );
        let resp = call_provider(provider, &judge_prompt, model, Some(16))?;
        let resp = resp.trim();
        // Take the first integer-looking token.
        let score: f64 = resp
            .split_whitespace()
            .next()
            .and_then(|s| s.trim_matches(|c: char| !c.is_ascii_digit() && c != '.' && c != '-').parse().ok())
            .context(format!("judge returned non-numeric: {resp:?}"))?;
        Ok(score.clamp(0.0, 10.0))
    }
}

#[cfg(feature = "run")]
pub use llm::run as run_llm;

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

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
        if let Some(obj) = try_read_object(repo, spec)? {
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
/// Special keys: `expected` (render-assert), `expected_output` (LLM-output-assert),
/// `rubric` (LLM-judge rubric).
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

#[allow(dead_code)]
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}

// Used by `status`/`list` to detect working-tree changes — re-exported for consistency.
#[allow(dead_code)]
pub fn _blob_hash_of(data: &[u8]) -> String {
    use crate::core::objects::hash_blob;
    hash_blob(data)
}
