//! `pv metrics` — full prompt analytics dashboard.
//!
//! Inspects a prompt template (rendered with magic vars and any provided
//! `--var` bindings) and reports: token/char/word/line counts, variable and
//! include breakdown, a complexity score, readability signals, and per-model
//! cost estimates. Designed for CI gates and prompt-quality dashboards.

use std::collections::HashMap;

use anyhow::Result;
use serde::Serialize;

use crate::core::prompt_ref;
use crate::core::repository::Repo;
use crate::pricing;
use crate::render::{extract_user_vars, extract_vars, render_full, resolve_includes};
use crate::tokens::{self, TokenCount};
use crate::ui::printer;

const DEFAULT_MODELS: &[&str] = &[
    "gpt-4o-mini",
    "gpt-4o",
    "claude-3-5-sonnet-latest",
    "o1-mini",
];

pub fn run(
    prompt: &str,
    vars: Vec<(String, String)>,
    models: Vec<String>,
    json: bool,
) -> Result<()> {
    let repo = Repo::find()?;
    let template = prompt_ref::load_prompt(&repo, prompt)?;

    let mut var_map: HashMap<String, String> = HashMap::new();
    for (k, v) in vars {
        var_map.insert(k, v);
    }

    // Pre-render: resolve includes, then substitute user + magic vars.
    let with_includes = resolve_includes(&repo, &template, false, 0)
        .unwrap_or_else(|_| template.clone());
    let rendered = render_full(Some(&repo), &template, &var_map, false)?;

    let tc = tokens::count(&rendered);
    let raw_tc = tokens::count(&template);

    // Variable inventory.
    let all_refs = extract_vars(&template);
    let user_vars = extract_user_vars(&template);
    let include_count = all_refs.iter().filter(|v| v.starts_with("include:")).count();
    let magic_count = all_refs.iter().filter(|v| crate::render::resolve_magic(v).is_some()).count();

    // Composition metrics on the (with-includes) template.
    let sentences = count_sentences(&with_includes);
    let avg_word_len = avg_word_length(&with_includes);
    let avg_sentence_len = if sentences > 0 {
        with_includes.split_whitespace().count() as f64 / sentences as f64
    } else { 0.0 };
    let instruction_density = instruction_density(&with_includes);

    let complexity = complexity_score(
        tc.tokens,
        user_vars.len(),
        include_count,
        instruction_density,
    );

    let model_ids: Vec<String> = if models.is_empty() {
        DEFAULT_MODELS.iter().map(|s| s.to_string()).collect()
    } else {
        models
    };

    let out_tokens = tokens::assumed_output_tokens(tc.tokens);
    let mut cost_rows = Vec::new();
    for id in &model_ids {
        let est = pricing::estimate(id, tc.tokens, out_tokens);
        let row = est.map(|e| (e.input_cost_usd, e.total_cost_usd, e.context_window));
        cost_rows.push((id.clone(), row));
    }

    if json {
        let out = MetricsReport {
            prompt: prompt.to_string(),
            template_tokens: raw_tc.tokens,
            template_chars: raw_tc.chars,
            template_words: raw_tc.words,
            template_lines: raw_tc.lines,
            rendered: RenderedMetrics {
                tokens: tc.tokens,
                chars: tc.chars,
                words: tc.words,
                lines: tc.lines,
                method: match tc.method {
                    tokens::TokenMethod::Exact { encoding } => Method::Exact { encoding },
                    tokens::TokenMethod::Estimate => Method::Estimate,
                },
            },
            variables: VarBreakdown {
                user: user_vars.clone(),
                user_count: user_vars.len(),
                magic_count,
                include_count,
            },
            readability: Readability {
                sentences,
                avg_word_len,
                avg_sentence_len,
                instruction_density,
            },
            complexity_score: complexity,
            assumed_output_tokens: out_tokens,
            costs: cost_rows.iter().map(|(id, row)| {
                serde_json::json!({
                    "model": id,
                    "input_cost_usd": row.map(|r| r.0),
                    "total_cost_usd": row.map(|r| r.1),
                    "context_window": row.map(|r| r.2),
                })
            }).collect(),
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    // Pretty dashboard.
    println!("{}", printer::bold(&format!("pv metrics · {}", prompt)));
    println!();
    println!("{}", printer::bold("size"));
    println!("  template  {:>6} tok  {:>6} chars  {:>4} words  {:>4} lines",
        raw_tc.tokens, raw_tc.chars, raw_tc.words, raw_tc.lines);
    println!("  rendered  {:>6} tok  {:>6} chars  {:>4} words  {:>4} lines  ({})",
        tc.tokens, tc.chars, tc.words, tc.lines, method_str(&tc));

    println!();
    println!("{}", printer::bold("composition"));
    println!("  user vars    {}  {}",
        user_vars.len(),
        if user_vars.is_empty() { "".to_string() } else {
            format!("({})", user_vars.iter().map(|v| format!("{{{v}}}")).collect::<Vec<_>>().join(" "))
        });
    println!("  magic vars   {}", magic_count);
    println!("  includes     {}", include_count);

    println!();
    println!("{}", printer::bold("readability"));
    println!("  sentences           {}", sentences);
    println!("  avg word length     {:.2} chars", avg_word_len);
    println!("  avg sentence length {:.1} words", avg_sentence_len);
    println!("  instruction density {:.2}  (imperative verbs per 100 words)",
        instruction_density);

    println!();
    println!("{}", printer::bold(&format!("complexity  score {complexity}/100")));
    let bar = progress_bar(complexity);
    println!("  {} {}",
        bar,
        complexity_label(complexity));

    println!();
    println!("{}", printer::bold(&format!("cost (in + assumed {out_tokens} out)")));
    println!("  {:<32} {:>12} {:>14}", "model", "input", "total");
    println!("  {}", "-".repeat(62));
    for (id, row) in &cost_rows {
        match row {
            Some((i, t, _ctx)) => {
                println!("  {:<32} {:>12} {:>14}",
                    id,
                    pricing::fmt_usd(*i),
                    pricing::fmt_usd(*t));
            }
            None => {
                println!("  {:<32} {:>12}", id, "(unknown model)");
            }
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct MetricsReport {
    prompt: String,
    template_tokens: usize,
    template_chars: usize,
    template_words: usize,
    template_lines: usize,
    rendered: RenderedMetrics,
    variables: VarBreakdown,
    readability: Readability,
    complexity_score: u32,
    assumed_output_tokens: usize,
    costs: Vec<serde_json::Value>,
}

#[derive(Serialize)]
struct RenderedMetrics {
    tokens: usize,
    chars: usize,
    words: usize,
    lines: usize,
    method: Method,
}

#[derive(Serialize)]
#[serde(tag = "kind")]
enum Method {
    Exact { encoding: &'static str },
    Estimate,
}

#[derive(Serialize)]
struct VarBreakdown {
    user: Vec<String>,
    user_count: usize,
    magic_count: usize,
    include_count: usize,
}

#[derive(Serialize)]
struct Readability {
    sentences: usize,
    avg_word_len: f64,
    avg_sentence_len: f64,
    instruction_density: f64,
}

fn method_str(tc: &TokenCount) -> String {
    match tc.method {
        tokens::TokenMethod::Exact { encoding } => format!("exact · {encoding}"),
        tokens::TokenMethod::Estimate => "heuristic".to_string(),
    }
}

/// Count "sentences" via terminal punctuation (. ! ?) followed by whitespace/EOF.
fn count_sentences(text: &str) -> usize {
    let mut n = 0;
    let mut prev = '\n';
    for c in text.chars() {
        if (prev == '.' || prev == '!' || prev == '?') && c.is_whitespace() {
            n += 1;
        }
        prev = c;
    }
    // Count a final non-empty sentence without trailing whitespace.
    if !text.is_empty() && !text.ends_with(|c: char| c.is_whitespace()) {
        let last = text.chars().last().unwrap();
        if last == '.' || last == '!' || last == '?' {
            n += 1;
        }
    }
    n.max(1)
}

fn avg_word_length(text: &str) -> f64 {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return 0.0;
    }
    let total_chars: usize = words.iter().map(|w| w.chars().count()).sum();
    total_chars as f64 / words.len() as f64
}

/// Rough count of imperative-instruction verbs per 100 words. Uses a small
/// stop-list of common prompt-instruction verbs (English) — a higher density
/// tends to indicate a more "directive" prompt.
fn instruction_density(text: &str) -> f64 {
    const VERBS: &[&str] = &[
        "summarize", "summarise", "translate", "explain", "list", "describe",
        "extract", "classify", "compare", "generate", "rewrite", "format",
        "answer", "find", "analyze", "analyse", "review", "convert", "draft",
        "write", "create", "provide", "identify", "evaluate", "score", "judge",
        "respond", "return", "output", "act", "imagine", "suppose", "assume",
        "ignore", "never", "always", "must", "should",
    ];
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return 0.0;
    }
    let hits = words.iter().filter(|w| {
        let lower = w.trim_matches(|c: char| !c.is_alphanumeric()).to_ascii_lowercase();
        VERBS.iter().any(|v| *v == lower.as_str())
    }).count();
    (hits as f64 / words.len() as f64) * 100.0
}

/// 0-100 heuristic complexity score. Combines token length, variable count,
/// include count, and instruction density. Larger = more complex.
fn complexity_score(tokens: usize, vars: usize, includes: usize, density: f64) -> u32 {
    // Each axis is normalized and weighted.
    let token_axis = (tokens as f64 / 4000.0).min(1.0);          // 4000 tok → max
    let var_axis = (vars as f64 / 12.0).min(1.0);                // 12 vars → max
    let include_axis = (includes as f64 / 5.0).min(1.0);        // 5 includes → max
    let density_axis = (density / 12.0).min(1.0);               // 12/100 words imperative → max

    // Weighted sum, scaled to 100.
    let score = token_axis * 35.0
        + var_axis * 20.0
        + include_axis * 15.0
        + density_axis * 30.0;
    score.round() as u32
}

fn progress_bar(score: u32) -> String {
    const W: usize = 30;
    let filled = ((score as f64 / 100.0) * W as f64).round() as usize;
    let filled = filled.min(W);
    let empty = W - filled;
    let bar: String = "█".repeat(filled);
    let rest: String = "░".repeat(empty);
    format!("[{}{}]", bar, rest)
}

fn complexity_label(score: u32) -> &'static str {
    match score {
        0..=20 => "trivial — short, no variables",
        21..=40 => "simple — minimal composition",
        41..=60 => "moderate — some variables / instructions",
        61..=80 => "complex — many moving parts",
        _ => "very complex — large, dense, composed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complexity_is_bounded() {
        let s = complexity_score(1_000_000, 100, 100, 100.0);
        assert!(s <= 100);
        let s0 = complexity_score(0, 0, 0, 0.0);
        assert!(s0 < 10);
    }

    #[test]
    fn instruction_density_counts_imperatives() {
        let d = instruction_density("Summarize the text. List the bullets. Return as JSON.");
        assert!(d > 5.0, "got {d}");
    }

    #[test]
    fn sentences_are_counted() {
        assert_eq!(count_sentences("One. Two. Three."), 3);
        assert_eq!(count_sentences("Hello world"), 1);
        assert_eq!(count_sentences(""), 1);
    }
}
