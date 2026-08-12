//! `pv serve` — turn a prompt vault into an HTTP API + Web GUI.
//!
//! SECURITY DESIGN:
//!   - Only compiled when the `serve` cargo feature is enabled. Default
//!     build has zero HTTP server code.
//!   - Binds to 127.0.0.1 by default (loopback only). Use `--host 0.0.0.0`
//!     explicitly to expose to the network.
//!   - Read-only: serves prompt content and history. Never writes to the vault.
//!
//! Routes:
//!   GET  /                          → built-in Web GUI (single HTML page)
//!   GET  /v1/health                  → { "status": "ok" }
//!   GET  /v1/prompts                 → [ { "name": ..., "hash": ... } ]
//!   GET  /v1/prompts/{name}          → { "name": ..., "content": ... }
//!   POST /v1/prompts/{name}/render   → { "content": rendered_string }
//!   GET  /v1/commits                 → [ { "hash", "message", "author", "ts" } ]
//!   GET  /v1/commits/{hash}/tree     → [ { "path", "hash" } ]
//!   GET  /v1/prompts/{name}/history  → commit list that touched this prompt

#![cfg(feature = "serve")]

use std::collections::HashMap;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use nu_ansi_term::Color;
use serde_json::json;
use tiny_http::{Header, Method, Response, Server};

use crate::core::objects;
use crate::core::repository::Repo;
use crate::render::render_full;
use crate::tokens;
use crate::ui::printer;

/// Built-in single-page Web GUI (HTML + vanilla JS, no build step).
const INDEX_HTML: &str = include_str!("../gui/index.html");

pub fn run(host: &str, port: u16) -> Result<()> {
    let addr_str = format!("{host}:{port}");
    let addr = SocketAddr::from_str(&addr_str)
        .with_context(|| format!("invalid bind address: {addr_str}"))?;

    // Verify we're inside a vault before starting.
    let repo = Repo::find()?;
    let repo = Arc::new(repo);

    let server = Server::http(addr).map_err(|e| anyhow!("failed to bind {addr}: {e}"))?;
    let url = format!("http://{addr}");

    printer::ok(&format!("prv serve listening at {url}"));
    println!(
        "  vault: {}",
        Color::Cyan.paint(repo.root.display().to_string())
    );
    println!();
    println!("  {} GET  /                        (Web GUI)", Color::DarkGray.paint("•"));
    println!("  {} GET  /v1/prompts               (list prompts)", Color::DarkGray.paint("•"));
    println!("  {} GET  /v1/prompts/{{name}}        (latest content)", Color::DarkGray.paint("•"));
    println!("  {} POST /v1/prompts/{{name}}/render (render with vars)", Color::DarkGray.paint("•"));
    println!("  {} POST /v1/chat/completions      (OpenAI-compatible: model=pv:name)", Color::DarkGray.paint("•"));
    println!("  {} GET  /v1/prompts/{{name}}/evals  (eval history)", Color::DarkGray.paint("•"));
    println!("  {} GET  /v1/commits               (commit history)", Color::DarkGray.paint("•"));
    println!("  {} GET  /v1/objects/{{hash}}        (raw object content)", Color::DarkGray.paint("•"));
    println!("  {} GET  /v1/variables/{{name}}       (extract template vars)", Color::DarkGray.paint("•"));
    println!("  {} GET  /v1/tokens?prompt=        (token count + cost)", Color::DarkGray.paint("•"));
    println!("  {} GET  /v1/metrics?prompt=        (full analytics)", Color::DarkGray.paint("•"));
    println!();
    println!("{}", Color::DarkGray.paint("Ctrl+C to stop."));

    for mut request in server.incoming_requests() {
        let response = build_response(&mut request, &repo);
        let _ = request.respond(response);
    }
    Ok(())
}

fn build_response(request: &mut tiny_http::Request, repo: &Repo) -> Response<std::io::Cursor<Vec<u8>>> {
    let method = request.method().clone();
    let url = request.url().to_string();
    let path = url.split('?').next().unwrap_or("/").to_string();
    let query = url.split_once('?').map(|(_, q)| q.to_string()).unwrap_or_default();

    // URL-decode the path so that names containing slashes (e.g. "prompts/foo.md"
    // encoded as "prompts%2Ffoo.md") are handled correctly.
    let decoded_path = url_decode(&path);
    let parts: Vec<&str> = decoded_path.split('/').filter(|s| !s.is_empty()).collect();

    // --- Fixed-segment routes (no name parameter) ---
    match (method.clone(), parts.as_slice()) {
        (Method::Get, []) => return html(INDEX_HTML),
        (Method::Get, ["v1", "health"]) => return json_ok(&json!({"status": "ok"})),
        (Method::Get, ["v1", "prompts"]) => {
            return match list_prompts(repo) {
                Ok(v) => json_ok(&v),
                Err(e) => json_err(500, &e.to_string()),
            };
        }
        (Method::Post, ["v1", "chat", "completions"]) => {
            let mut body = String::new();
            if request.as_reader().read_to_string(&mut body).is_err() {
                return json_err(400, "invalid request body");
            }
            return match chat_completions(repo, &body) {
                Ok(v) => json_ok(&v),
                Err(e) => json_err(500, &e.to_string()),
            };
        }
        (Method::Get, ["v1", "commits"]) => {
            return match list_commits(repo) {
                Ok(v) => json_ok(&v),
                Err(e) => json_err(500, &e.to_string()),
            };
        }
        (Method::Get, ["v1", "diff"]) => {
            let q = parse_query(&query);
            let from = q.get("from").map(|s| s.as_str());
            let to = q.get("to").map(|s| s.as_str());
            return match diff(repo, from, to) {
                Ok(v) => json_ok(&v),
                Err(e) => json_err(400, &e.to_string()),
            };
        }
        (Method::Get, ["v1", "tokens"]) => {
            let q = parse_query(&query);
            let prompt = match q.get("prompt") {
                Some(p) => p.as_str(),
                None => return json_err(400, "missing 'prompt' query parameter"),
            };
            let max_tokens = q.get("max_tokens").and_then(|s| s.parse::<u32>().ok());
            let models: Vec<String> = q.get("models")
                .map(|s| s.split(',').map(|m| m.trim().to_string()).filter(|m| !m.is_empty()).collect())
                .unwrap_or_default();
            let vars = parse_var_query(&query);
            return match tokens_endpoint(repo, prompt, &vars, models, max_tokens) {
                Ok(v) => json_ok(&v),
                Err(e) => json_err(500, &e.to_string()),
            };
        }
        (Method::Get, ["v1", "metrics"]) => {
            let q = parse_query(&query);
            let prompt = match q.get("prompt") {
                Some(p) => p.as_str(),
                None => return json_err(400, "missing 'prompt' query parameter"),
            };
            let models: Vec<String> = q.get("models")
                .map(|s| s.split(',').map(|m| m.trim().to_string()).filter(|m| !m.is_empty()).collect())
                .unwrap_or_default();
            let vars = parse_var_query(&query);
            return match metrics_endpoint(repo, prompt, &vars, models) {
                Ok(v) => json_ok(&v),
                Err(e) => json_err(500, &e.to_string()),
            };
        }
        (Method::Post, ["v1", "tokens"]) => {
            let mut body = String::new();
            if request.as_reader().read_to_string(&mut body).is_err() {
                return json_err(400, "invalid request body");
            }
            return match tokens_post(repo, &body) {
                Ok(v) => json_ok(&v),
                Err(e) => json_err(500, &e.to_string()),
            };
        }
        (Method::Post, ["v1", "metrics"]) => {
            let mut body = String::new();
            if request.as_reader().read_to_string(&mut body).is_err() {
                return json_err(400, "invalid request body");
            }
            return match metrics_post(repo, &body) {
                Ok(v) => json_ok(&v),
                Err(e) => json_err(500, &e.to_string()),
            };
        }
        _ => {}
    }

    // --- Routes with a name/hash parameter (may contain slashes) ---
    // These use prefix/suffix matching on the decoded path.
    let p = decoded_path.as_str();

    if method == Method::Get {
        // GET /v1/objects/{hash}
        if let Some(rest) = p.strip_prefix("/v1/objects/") {
            let hash = rest.trim_end_matches('/');
            return match get_object(repo, hash) {
                Ok(v) => json_ok(&v),
                Err(e) => json_err(404, &e.to_string()),
            };
        }
        // GET /v1/variables/{name}
        if let Some(rest) = p.strip_prefix("/v1/variables/") {
            let name = rest.trim_end_matches('/');
            return match extract_variables(repo, name) {
                Ok(v) => json_ok(&v),
                Err(e) => json_err(404, &e.to_string()),
            };
        }
        // GET /v1/commits/{hash}/tree
        if let Some(rest) = p.strip_prefix("/v1/commits/") {
            if let Some(hash) = rest.strip_suffix("/tree") {
                return match get_tree(repo, hash) {
                    Ok(v) => json_ok(&v),
                    Err(e) => json_err(404, &e.to_string()),
                };
            }
        }
        // GET /v1/prompts/{name}/history  and  /evals
        if let Some(rest) = p.strip_prefix("/v1/prompts/") {
            if let Some(name) = rest.strip_suffix("/history") {
                return match prompt_history(repo, name) {
                    Ok(v) => json_ok(&v),
                    Err(e) => json_err(404, &e.to_string()),
                };
            }
            if let Some(name) = rest.strip_suffix("/evals") {
                return match eval_history(repo, name) {
                    Ok(v) => json_ok(&v),
                    Err(e) => json_err(500, &e.to_string()),
                };
            }
            // GET /v1/prompts/{name}  (fetch content — name may contain slashes)
            if !rest.is_empty() {
                return match get_prompt(repo, rest) {
                    Ok(v) => json_ok(&v),
                    Err(e) => json_err(404, &e.to_string()),
                };
            }
        }
    }

    if method == Method::Post {
        // POST /v1/prompts/{name}/render
        if let Some(rest) = p.strip_prefix("/v1/prompts/") {
            if let Some(name) = rest.strip_suffix("/render") {
                let mut body = String::new();
                if request.as_reader().read_to_string(&mut body).is_err() {
                    return json_err(400, "invalid request body");
                }
                let vars: HashMap<String, String> = serde_json::from_str(&body).unwrap_or_default();
                return match render_prompt(repo, name, &vars) {
                    Ok(v) => json_ok(&v),
                    Err(e) => json_err(400, &e.to_string()),
                };
            }
        }
    }

    json_err(404, &format!("not found: /{}", path))
}

// --- API helpers ----------------------------------------------------------

fn list_prompts(repo: &Repo) -> Result<serde_json::Value> {
    let head = repo.head_commit()?;
    let Some(h) = head else {
        return Ok(json!([]));
    };
    let commit = objects::read_commit(&repo.pv_dir, &h)?;
    let entries = objects::read_tree(&repo.pv_dir, &commit.tree)?;
    let out: Vec<_> = entries
        .iter()
        .map(|e| json!({ "name": e.path, "hash": e.hash }))
        .collect();
    Ok(json!(out))
}

fn get_prompt(repo: &Repo, name: &str) -> Result<serde_json::Value> {
    let head = repo
        .head_commit()?
        .ok_or_else(|| anyhow!("no commits yet"))?;
    let commit = objects::read_commit(&repo.pv_dir, &head)?;
    let entries = objects::read_tree(&repo.pv_dir, &commit.tree)?;
    let entry = entries
        .iter()
        .find(|e| e.path == name)
        .ok_or_else(|| anyhow!("prompt not found: {name}"))?;
    let blob = objects::read_object(&repo.pv_dir, &entry.hash)?;
    let content = String::from_utf8_lossy(&blob.data).to_string();
    Ok(json!({ "name": name, "hash": entry.hash, "content": content }))
}

fn render_prompt(repo: &Repo, name: &str, vars: &HashMap<String, String>) -> Result<serde_json::Value> {
    let prompt = get_prompt(repo, name)?;
    let template = prompt["content"]
        .as_str()
        .ok_or_else(|| anyhow!("invalid prompt content"))?;
    let rendered = render_full(Some(repo), template, vars, false).context("render failed")?;
    Ok(json!({ "name": name, "content": rendered }))
}

fn list_commits(repo: &Repo) -> Result<serde_json::Value> {
    let mut out = Vec::new();
    let mut cur = repo.head_commit()?;
    while let Some(h) = cur {
        let c = objects::read_commit(&repo.pv_dir, &h)?;
        out.push(json!({
            "hash": h,
            "message": c.message,
            "author": c.author,
            "timestamp": c.timestamp,
        }));
        cur = c.parent;
        if out.len() >= 200 {
            break;
        }
    }
    Ok(json!(out))
}

fn get_tree(repo: &Repo, hash: &str) -> Result<serde_json::Value> {
    let commit = objects::read_commit(&repo.pv_dir, hash)?;
    let entries = objects::read_tree(&repo.pv_dir, &commit.tree)?;
    let out: Vec<_> = entries
        .iter()
        .map(|e| json!({ "path": e.path, "hash": e.hash }))
        .collect();
    Ok(json!(out))
}

/// Read a raw object by hash and return its content (blobs only).
/// Used by the Web GUI diff viewer to fetch prompt content at any commit.
fn get_object(repo: &Repo, hash: &str) -> Result<serde_json::Value> {
    let obj = objects::read_object(&repo.pv_dir, hash)?;
    let kind = match obj.kind {
        objects::ObjectType::Blob => "blob",
        objects::ObjectType::Tree => "tree",
        objects::ObjectType::Commit => "commit",
    };
    let content = String::from_utf8_lossy(&obj.data).to_string();
    Ok(json!({ "hash": hash, "type": kind, "content": content, "size": obj.data.len() }))
}

/// Extract and classify all {{variables}} from a prompt's template.
/// Returns user vars, magic vars, and includes separately so the GUI can
/// render an inline form editor for user-defined variables.
fn extract_variables(repo: &Repo, name: &str) -> Result<serde_json::Value> {
    let template = crate::core::prompt_ref::load_prompt(repo, name)?;
    let all = crate::render::extract_vars(&template);
    let user = crate::render::extract_user_vars(&template);
    let magic: Vec<String> = all
        .iter()
        .filter(|v| crate::render::resolve_magic(v).is_some())
        .cloned()
        .collect();
    let includes: Vec<String> = all
        .iter()
        .filter(|v| v.starts_with("include:"))
        .cloned()
        .collect();
    Ok(json!({
        "user": user,
        "magic": magic,
        "includes": includes,
    }))
}

fn prompt_history(repo: &Repo, name: &str) -> Result<serde_json::Value> {
    let mut out = Vec::new();
    let mut cur = repo.head_commit()?;
    while let Some(h) = cur {
        let c = objects::read_commit(&repo.pv_dir, &h)?;
        let entries = objects::read_tree(&repo.pv_dir, &c.tree)?;
        if entries.iter().any(|e| e.path == name) {
            out.push(json!({
                "hash": h,
                "message": c.message,
                "author": c.author,
                "timestamp": c.timestamp,
            }));
        }
        cur = c.parent;
        if out.len() >= 200 {
            break;
        }
    }
    Ok(json!(out))
}

fn diff(repo: &Repo, from: Option<&str>, to: Option<&str>) -> Result<serde_json::Value> {
    let resolve = |spec: Option<&str>| -> Result<Option<String>> {
        match spec {
            None => repo.head_commit(),
            Some("HEAD") => repo.head_commit(),
            Some(s) if s.len() >= 4 && s.chars().all(|c| c.is_ascii_hexdigit()) => Ok(Some(s.to_string())),
            Some(s) => Ok(Some(s.to_string())),
        }
    };
    let from_h = resolve(from)?;
    let to_h = resolve(to)?;
    Ok(json!({ "from": from_h, "to": to_h, "note": "use `pv diff` for full text diff" }))
}

/// OpenAI-compatible chat completions endpoint.
///
/// If `model` starts with `pv:`, the rest is treated as a prompt name in the
/// vault. The latest committed version is rendered with `vars` from the body
/// and sent to the configured LLM provider. The response is returned in
/// standard OpenAI format.
///
/// If `model` does not start with `pv:`, the request is passed through to
/// OpenAI directly (acts as a transparent proxy).
fn chat_completions(repo: &Repo, body: &str) -> Result<serde_json::Value> {
    use crate::commands::run::{dispatch, Provider};

    let req: serde_json::Value = serde_json::from_str(body)
        .context("invalid JSON body")?;

    let model = req["model"]
        .as_str()
        .ok_or_else(|| anyhow!("missing 'model' field"))?;

    let provider_str = req["provider"]
        .as_str()
        .unwrap_or("openai");
    let provider = Provider::parse(provider_str)?;

    let (prompt_text, actual_model) = if let Some(name) = model.strip_prefix("pv:") {
        // Vault prompt mode.
        let prompt_val = get_prompt(repo, name)?;
        let template = prompt_val["content"]
            .as_str()
            .ok_or_else(|| anyhow!("invalid prompt content for '{name}'"))?;
        let vars: HashMap<String, String> = req["vars"]
            .as_object()
            .map(|o| {
                o.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let rendered = render_full(Some(repo), template, &vars, false).context("render failed")?;
        (rendered, req["model"].as_str().unwrap_or("").to_string())
    } else {
        // Pass-through mode: use the last user message as the prompt.
        let messages = req["messages"]
            .as_array()
            .ok_or_else(|| anyhow!("missing 'messages' array"))?;
        let last_user = messages
            .iter()
            .rev()
            .find(|m| m["role"].as_str() == Some("user"))
            .and_then(|m| m["content"].as_str())
            .ok_or_else(|| anyhow!("no user message found"))?;
        (last_user.to_string(), model.to_string())
    };

    let max_tokens = req["max_tokens"].as_u64().map(|n| n as u32);
    let model_opt = if model.starts_with("pv:") {
        // For vault prompts, use the provider's default model unless overridden.
        req["underlying_model"].as_str()
    } else {
        Some(model)
    };

    let content = dispatch(&provider, &prompt_text, model_opt, max_tokens)?;

    let id = format!("pvchat-{:016x}", chrono::Utc::now().timestamp_millis() as u64);
    Ok(json!({
        "id": id,
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": actual_model,
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": content},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0}
    }))
}

/// Return eval history for a prompt.
fn eval_history(repo: &Repo, name: &str) -> Result<serde_json::Value> {
    let history = crate::commands::eval::read_history(repo, name)?;
    Ok(json!(history))
}

// --- tokens / metrics endpoints -------------------------------------------

const DEFAULT_TOKEN_MODELS: &[&str] = &[
    "gpt-4o-mini",
    "gpt-4o",
    "claude-3-5-haiku-latest",
    "claude-3-5-sonnet-latest",
    "o1-mini",
];

fn tokens_endpoint(
    repo: &Repo,
    prompt: &str,
    vars: &[(String, String)],
    models: Vec<String>,
    max_tokens: Option<u32>,
) -> Result<serde_json::Value> {
    let template = crate::core::prompt_ref::load_prompt(repo, prompt)?;
    let var_map: HashMap<String, String> = vars.iter().cloned().collect();
    let rendered = render_full(Some(repo), &template, &var_map, false)?;
    let tc = tokens::count(&rendered);
    let out_tokens = max_tokens.map(|n| n as usize).unwrap_or_else(|| tokens::assumed_output_tokens(tc.tokens));

    let model_ids: Vec<String> = if models.is_empty() {
        DEFAULT_TOKEN_MODELS.iter().map(|s| s.to_string()).collect()
    } else {
        models
    };

    let mut estimates = Vec::new();
    for id in &model_ids {
        let entry = match crate::pricing::estimate(id, tc.tokens, out_tokens) {
            Some(e) => json!({
                "model": e.model_id,
                "input_cost_usd": e.input_cost_usd,
                "output_cost_usd": e.output_cost_usd,
                "total_cost_usd": e.total_cost_usd,
                "context_window": e.context_window,
            }),
            None => json!({ "model": id, "error": "unknown model" }),
        };
        estimates.push(entry);
    }

    Ok(json!({
        "prompt": prompt,
        "rendered_tokens": tc.tokens,
        "rendered_chars": tc.chars,
        "rendered_words": tc.words,
        "rendered_lines": tc.lines,
        "method": match tc.method {
            tokens::TokenMethod::Exact { encoding } => json!({"kind": "exact", "encoding": encoding}),
            tokens::TokenMethod::Estimate => json!({"kind": "estimate"}),
        },
        "assumed_output_tokens": out_tokens,
        "estimates": estimates,
    }))
}

fn metrics_endpoint(
    repo: &Repo,
    prompt: &str,
    vars: &[(String, String)],
    models: Vec<String>,
) -> Result<serde_json::Value> {
    let template = crate::core::prompt_ref::load_prompt(repo, prompt)?;
    let var_map: HashMap<String, String> = vars.iter().cloned().collect();
    let rendered = render_full(Some(repo), &template, &var_map, false)?;
    let tc = tokens::count(&rendered);
    let tc_template = tokens::count(&template);

    let user_vars = crate::render::extract_user_vars(&template);
    let all_refs = crate::render::extract_vars(&template);
    let include_count = all_refs.iter().filter(|v| v.starts_with("include:")).count();
    let magic_count = all_refs.iter().filter(|v| crate::render::resolve_magic(v).is_some()).count();

    let out_tokens = tokens::assumed_output_tokens(tc.tokens);

    // Readability metrics (computed on the rendered text).
    let readability = compute_readability(&rendered);

    // Complexity score (0-100).
    let complexity = compute_complexity_score(
        tc.tokens,
        user_vars.len(),
        include_count,
        readability.instruction_density,
    );

    let model_ids: Vec<String> = if models.is_empty() {
        DEFAULT_TOKEN_MODELS.iter().map(|s| s.to_string()).collect()
    } else {
        models
    };
    let mut costs = Vec::new();
    for id in &model_ids {
        let entry = match crate::pricing::estimate(id, tc.tokens, out_tokens) {
            Some(e) => json!({
                "model": e.model_id,
                "input_cost_usd": e.input_cost_usd,
                "total_cost_usd": e.total_cost_usd,
                "context_window": e.context_window,
            }),
            None => json!({ "model": id, "error": "unknown model" }),
        };
        costs.push(entry);
    }

    Ok(json!({
        "prompt": prompt,
        "template_tokens": tc_template.tokens,
        "template_chars": tc_template.chars,
        "template_words": tc_template.words,
        "template_lines": tc_template.lines,
        "rendered": {
            "tokens": tc.tokens,
            "chars": tc.chars,
            "words": tc.words,
            "lines": tc.lines,
            "method": match tc.method {
                tokens::TokenMethod::Exact { encoding } => json!({"kind": "exact", "encoding": encoding}),
                tokens::TokenMethod::Estimate => json!({"kind": "estimate"}),
            },
        },
        "variables": {
            "user": user_vars,
            "user_count": user_vars.len(),
            "magic_count": magic_count,
            "include_count": include_count,
        },
        "readability": {
            "sentences": readability.sentences,
            "avg_word_len": readability.avg_word_len,
            "avg_sentence_len": readability.avg_sentence_len,
            "instruction_density": readability.instruction_density,
        },
        "complexity_score": complexity,
        "assumed_output_tokens": out_tokens,
        "costs": costs,
    }))
}

/// Readability metrics for a rendered prompt.
struct Readability {
    sentences: usize,
    avg_word_len: f64,
    avg_sentence_len: f64,
    instruction_density: f64,
}

fn compute_readability(text: &str) -> Readability {
    let words: Vec<&str> = text.split_whitespace().collect();
    let word_count = words.len().max(1);
    let total_chars: usize = words.iter().map(|w| w.chars().count()).sum();
    let avg_word_len = total_chars as f64 / word_count as f64;

    let sentences = text.split('.').filter(|s| !s.trim().is_empty()).count().max(1);
    let avg_sentence_len = word_count as f64 / sentences as f64;

    let imperative_verbs = ["summarize", "translate", "write", "create", "generate", "list",
        "explain", "analyze", "compare", "describe", "identify", "extract", "convert",
        "format", "classify", "evaluate", "check", "ensure", "use", "return",
        "respond", "answer", "provide", "include", "avoid", "follow", "make",
        "add", "remove", "replace", "find", "count", "match", "replace"];
    let instruction_count = words.iter()
        .filter(|w| imperative_verbs.contains(&w.to_lowercase().trim_matches(|c: char| !c.is_alphanumeric())))
        .count();
    let instruction_density = (instruction_count as f64 / word_count as f64) * 100.0;

    Readability {
        sentences,
        avg_word_len,
        avg_sentence_len,
        instruction_density,
    }
}

fn compute_complexity_score(tokens: usize, vars: usize, includes: usize, density: f64) -> u32 {
    let token_axis = (tokens as f64 / 4000.0).min(1.0);
    let var_axis = (vars as f64 / 12.0).min(1.0);
    let include_axis = (includes as f64 / 5.0).min(1.0);
    let density_axis = (density / 12.0).min(1.0);
    let score = token_axis * 35.0 + var_axis * 20.0 + include_axis * 15.0 + density_axis * 30.0;
    score.round() as u32
}

fn tokens_post(repo: &Repo, body: &str) -> Result<serde_json::Value> {
    let req: serde_json::Value = serde_json::from_str(body).context("invalid JSON body")?;
    let prompt = req["prompt"].as_str().ok_or_else(|| anyhow!("missing 'prompt'"))?.to_string();
    let vars = parse_json_vars(&req["vars"]);
    let models = req["models"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    let max_tokens = req["max_tokens"].as_u64().map(|n| n as u32);
    tokens_endpoint(repo, &prompt, &vars, models, max_tokens)
}

fn metrics_post(repo: &Repo, body: &str) -> Result<serde_json::Value> {
    let req: serde_json::Value = serde_json::from_str(body).context("invalid JSON body")?;
    let prompt = req["prompt"].as_str().ok_or_else(|| anyhow!("missing 'prompt'"))?.to_string();
    let vars = parse_json_vars(&req["vars"]);
    let models = req["models"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    metrics_endpoint(repo, &prompt, &vars, models)
}

/// Parse `vars=key:val,key:val` query-string value into a (k,v) list.
/// Uses `:` as the inner separator (since `=` is the query separator).
fn parse_var_query(query: &str) -> Vec<(String, String)> {
    let q = parse_query(query);
    let Some(s) = q.get("vars") else { return Vec::new() };
    s.split(',')
        .filter_map(|kv| kv.split_once(':').map(|(k, v)| (k.trim().to_string(), v.trim().to_string())))
        .filter(|(k, _)| !k.is_empty())
        .collect()
}

fn parse_json_vars(vars: &serde_json::Value) -> Vec<(String, String)> {
    vars.as_object()
        .map(|o| {
            o.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                .collect()
        })
        .unwrap_or_default()
}

// --- HTTP plumbing --------------------------------------------------------

fn html(body: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let bytes = body.as_bytes().to_vec();
    Response::from_data(bytes)
        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap())
}

fn json_ok(v: &serde_json::Value) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = serde_json::to_string_pretty(v).unwrap_or_else(|_| "{}".into());
    let bytes = body.into_bytes();
    Response::from_data(bytes)
        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap())
}

fn json_err(code: u16, msg: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = json!({ "error": msg });
    let bytes = serde_json::to_string(&body).unwrap_or_else(|_| "{}".into()).into_bytes();
    Response::from_data(bytes)
        .with_status_code(code)
        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap())
}

fn parse_query(q: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in q.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            let k = url_decode(k);
            let v = url_decode(v);
            map.insert(k, v);
        }
    }
    map
}

fn url_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let h1 = chars.next();
            let h2 = chars.next();
            if let (Some(a), Some(b)) = (h1, h2) {
                let hex = format!("{a}{b}");
                if let Ok(n) = u8::from_str_radix(&hex, 16) {
                    out.push(n as char);
                    continue;
                }
            }
            out.push('%');
        } else if c == '+' {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}
