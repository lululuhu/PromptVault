//! `pv tokens` — count tokens and estimate API cost for a prompt.
//!
//! Renders the prompt with the given `--var key=value` bindings (or magic
//! variables), counts tokens, and prints a cost estimate across the most
//! popular models. Works with no `run` feature (uses a heuristic in that case).

use std::collections::HashMap;

use anyhow::Result;

use crate::core::prompt_ref;
use crate::core::repository::Repo;
use crate::pricing;
use crate::render::render_full;
use crate::tokens::{self, TokenCount};
use crate::ui::printer;

/// Default models shown when `--model` is not given. Covers the popular
/// span from cheap (gpt-4o-mini) to flagship (gpt-4o, claude-3-5-sonnet).
const DEFAULT_MODELS: &[&str] = &[
    "gpt-4o-mini",
    "gpt-4o",
    "claude-3-5-haiku-latest",
    "claude-3-5-sonnet-latest",
    "o1-mini",
];

pub fn run(
    prompt: &str,
    vars: Vec<(String, String)>,
    models: Vec<String>,
    max_tokens: Option<u32>,
    json: bool,
) -> Result<()> {
    let repo = Repo::find()?;
    let template = prompt_ref::load_prompt(&repo, prompt)?;

    let mut var_map: HashMap<String, String> = HashMap::new();
    for (k, v) in vars {
        var_map.insert(k, v);
    }
    let rendered = render_full(Some(&repo), &template, &var_map, false)?;

    let tc = tokens::count(&rendered);
    let out_tokens = max_tokens.map(|n| n as usize).unwrap_or_else(|| tokens::assumed_output_tokens(tc.tokens));

    let model_ids: Vec<String> = if models.is_empty() {
        DEFAULT_MODELS.iter().map(|s| s.to_string()).collect()
    } else {
        models
    };

    let mut estimates: Vec<(String, Option<(f64, f64, f64, usize)>)> = Vec::new();
    for id in &model_ids {
        let est = pricing::estimate(id, tc.tokens, out_tokens);
        let row = est.map(|e| (e.input_cost_usd, e.output_cost_usd, e.total_cost_usd, e.context_window));
        estimates.push((id.clone(), row));
    }

    if json {
        print_json(&tc, &rendered, out_tokens, &estimates)?;
        return Ok(());
    }

    // Pretty print.
    println!("{}", printer::bold(&format!("pv tokens · {}", short_path(prompt))));
    println!();
    println!("  {} {} tokens  ({} chars · {} words · {} lines)",
        printer::dim("rendered:"),
        tc.tokens, tc.chars, tc.words, tc.lines);
    println!("  {} {}", printer::dim("method:  "),
        method_str(&tc));
    println!("  {} {} tokens", printer::dim("output:  "), out_tokens);
    if !var_map.is_empty() {
        let keys: Vec<&str> = var_map.keys().map(|s| s.as_str()).collect();
        println!("  {} {}", printer::dim("vars:    "), keys.join(", "));
    }
    println!();
    println!("{}", printer::bold("cost per call (1 input + 1 output):"));
    println!("  {:<32} {:>14} {:>14} {:>14}",
        "model", "input", "output", "total");
    println!("  {}", "-".repeat(78));
    for (id, row) in &estimates {
        match row {
            Some((i, o, t, _ctx)) => {
                println!("  {:<32} {:>14} {:>14} {:>14}",
                    id,
                    pricing::fmt_usd(*i),
                    pricing::fmt_usd(*o),
                    pricing::fmt_usd(*t));
            }
            None => {
                println!("  {:<32} {:>14}", id, "(unknown model)");
            }
        }
    }
    println!();
    println!("  {}", printer::dim(
        "1M tokens = 1,000,000. Local models (ollama) show as free."));
    Ok(())
}

fn method_str(tc: &TokenCount) -> String {
    match tc.method {
        tokens::TokenMethod::Exact { encoding } => format!("exact ({encoding})"),
        tokens::TokenMethod::Estimate => "heuristic (build without --features run)".to_string(),
    }
}

fn short_path(spec: &str) -> String {
    // Show `HEAD:path` or just `path` without obscuring.
    spec.to_string()
}

fn print_json(
    tc: &TokenCount,
    rendered: &str,
    out_tokens: usize,
    estimates: &[(String, Option<(f64, f64, f64, usize)>)],
) -> Result<()> {
    let mut arr = Vec::new();
    for (id, row) in estimates {
        let entry = match row {
            Some((i, o, t, ctx)) => serde_json::json!({
                "model": id,
                "input_cost_usd": i,
                "output_cost_usd": o,
                "total_cost_usd": t,
                "context_window": ctx,
            }),
            None => serde_json::json!({ "model": id, "error": "unknown model" }),
        };
        arr.push(entry);
    }
    let out = serde_json::json!({
        "rendered_chars": tc.chars,
        "rendered_words": tc.words,
        "rendered_lines": tc.lines,
        "rendered_tokens": tc.tokens,
        "method": match tc.method {
            tokens::TokenMethod::Exact { encoding } => serde_json::json!({"kind": "exact", "encoding": encoding}),
            tokens::TokenMethod::Estimate => serde_json::json!({"kind": "estimate"}),
        },
        "assumed_output_tokens": out_tokens,
        "estimates": arr,
        "rendered_preview": {
            "first_200_chars": rendered.chars().take(200).collect::<String>(),
        },
    });
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}
