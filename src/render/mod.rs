//! Tiny `{{var}}` template renderer with magic variables and includes.
//!
//! Supports:
//!   - `{{name}}` and `{{ name }}` (whitespace-tolerant) — user-supplied vars
//!   - **Magic variables** (auto-resolved, see [`resolve_magic`]):
//!       `{{date}}` `{{time}}` `{{datetime}}` `{{timestamp}}` `{{iso8601}}`
//!       `{{year}}` `{{month}}` `{{day}}` `{{hour}}` `{{minute}}` `{{second}}`
//!       `{{uuid}}` `{{random:N}}` `{{os}}` `{{cwd}}`
//!   - **Includes** (`{{include:path}}` or `{{include:HEAD:path}}`) — compose
//!     a prompt from reusable partials stored in the vault. Resolved by
//!     [`resolve_includes`] before variable substitution.
//!
//! Works on `char` boundaries (UTF-8 safe), so Chinese / emoji / accented
//! content is preserved exactly.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Result};

use crate::core::prompt_ref;
use crate::core::repository::Repo;
use crate::core::hash::hash_bytes;

/// Render `template` by substituting `{{var}}` with values from `vars`.
///
/// User-supplied `vars` take precedence over magic variables, so callers
/// can override `{{date}}` etc. if they really want to.
///
/// - `strict = true`:  any `{{var}}` not present in `vars` and not a magic var is an error.
/// - `strict = false`: undefined `{{var}}` is left verbatim in the output.
pub fn render(template: &str, vars: &HashMap<String, String>, strict: bool) -> Result<String> {
    let chars: Vec<char> = template.chars().collect();
    let mut out = String::with_capacity(template.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{' && i + 1 < chars.len() && chars[i + 1] == '{' {
            // Find the closing }}.
            if let Some(end) = find_close(&chars[i + 2..]) {
                let name_raw: String = chars[i + 2..i + 2 + end].iter().collect();
                let name = name_raw.trim();
                if name.is_empty() {
                    bail!("empty variable name at char {i}");
                }
                match vars.get(name) {
                    Some(v) => out.push_str(v),
                    None => match resolve_magic(name) {
                        Some(v) => out.push_str(&v),
                        None => {
                            if strict {
                                bail!("undefined template variable: {name}");
                            }
                            out.push_str("{{");
                            out.push_str(&name_raw);
                            out.push_str("}}");
                        }
                    },
                }
                i += 2 + end + 2;
            } else {
                // No closing }}: treat the rest as literal.
                out.extend(&chars[i..]);
                break;
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    Ok(out)
}

/// Full pipeline: resolve `{{include:path}}` directives against the vault,
/// then substitute user vars + magic vars. Use this when you have a `Repo`
/// and want includes to work. Otherwise [`render`] is enough.
pub fn render_full(
    repo: Option<&Repo>,
    template: &str,
    vars: &HashMap<String, String>,
    strict: bool,
) -> Result<String> {
    let resolved = match repo {
        Some(r) => resolve_includes(r, template, strict, 0)?,
        None => template.to_string(),
    };
    render(&resolved, vars, strict)
}

/// Resolve `{{include:path}}` (or `{{include:HEAD:path}}`) directives by
/// inlining the referenced prompt's content. Recurses up to a small max
/// depth to prevent infinite cycles. Unknown includes are left verbatim in
/// non-strict mode, or error in strict mode.
pub fn resolve_includes(repo: &Repo, template: &str, strict: bool, depth: u32) -> Result<String> {
    const MAX_DEPTH: u32 = 16;
    if depth >= MAX_DEPTH {
        bail!("include nesting too deep (>{MAX_DEPTH}) — possible cycle");
    }

    let chars: Vec<char> = template.chars().collect();
    let mut out = String::with_capacity(template.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{' && i + 1 < chars.len() && chars[i + 1] == '{' {
            if let Some(end) = find_close(&chars[i + 2..]) {
                let name_raw: String = chars[i + 2..i + 2 + end].iter().collect();
                let name = name_raw.trim();
                if let Some(path) = name.strip_prefix("include:") {
                    let path = path.trim();
                    if path.is_empty() {
                        bail!("{{include:...}} with empty path at char {i}");
                    }
                    match prompt_ref::load_prompt(repo, path) {
                        Ok(content) => {
                            // Recurse to allow nested includes.
                            let inner =
                                resolve_includes(repo, &content, strict, depth + 1)?;
                            out.push_str(&inner);
                        }
                        Err(e) => {
                            if strict {
                                bail!("include '{path}' failed: {e}");
                            }
                            out.push_str("{{");
                            out.push_str(&name_raw);
                            out.push_str("}}");
                        }
                    }
                    i += 2 + end + 2;
                    continue;
                }
                // Not an include directive — leave verbatim, render() will handle it.
                out.push_str("{{");
                out.push_str(&name_raw);
                out.push_str("}}");
                i += 2 + end + 2;
            } else {
                out.extend(&chars[i..]);
                break;
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    Ok(out)
}

/// Resolve a magic variable name to its value. Returns `None` if `name` is
/// not a recognized magic variable.
///
/// Magic variables (all evaluated at render time, in UTC):
///   - `date`       → YYYY-MM-DD
///   - `time`       → HH:MM:SS
///   - `datetime`   → YYYY-MM-DD HH:MM:SS
///   - `timestamp`  → Unix epoch seconds
///   - `iso8601`    → ISO 8601 UTC (e.g. 2026-08-12T14:30:00Z)
///   - `year`, `month`, `day`, `hour`, `minute`, `second` — date components
///   - `uuid`       → random UUID v4 (lowercase, dashed)
///   - `random:N`   → N random lowercase hex chars (N clamped to 1..=128)
///   - `os`         → linux / macos / windows
///   - `cwd`        → basename of the current working directory
pub fn resolve_magic(name: &str) -> Option<String> {
    let now = chrono::Utc::now();
    let ts = now.timestamp();
    match name {
        "date" => Some(now.format("%Y-%m-%d").to_string()),
        "time" => Some(now.format("%H:%M:%S").to_string()),
        "datetime" => Some(now.format("%Y-%m-%d %H:%M:%S").to_string()),
        "timestamp" => Some(ts.to_string()),
        "iso8601" => Some(now.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        "year" => Some(now.format("%Y").to_string()),
        "month" => Some(now.format("%m").to_string()),
        "day" => Some(now.format("%d").to_string()),
        "hour" => Some(now.format("%H").to_string()),
        "minute" => Some(now.format("%M").to_string()),
        "second" => Some(now.format("%S").to_string()),
        "os" => Some(std::env::consts::OS.to_string()),
        "cwd" => std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())),
        "uuid" => Some(gen_uuid_v4()),
        _ => {
            if let Some(n_str) = name.strip_prefix("random:") {
                let n: usize = n_str.trim().parse().ok()?;
                let n = n.clamp(1, 128);
                Some(random_hex(n))
            } else {
                None
            }
        }
    }
}

// ---- entropy helpers (no extra deps) -------------------------------------

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn entropy_seed() -> [u8; 32] {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let pid = std::process::id() as u64;
    let ctr = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mix = nanos
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(pid.wrapping_mul(0xBF58_476D_1CE4_E5B9))
        .wrapping_add(ctr.wrapping_mul(0x94D0_49BB_1331_116E));
    // Two SHA-256 rounds over the seed give us 32 bytes of pseudo-randomness.
    // Good enough for prompt-template placeholders, NOT for crypto.
    let s1 = mix.to_le_bytes();
    let h1 = hash_bytes(&s1);
    let h2 = hash_bytes(h1.as_bytes());
    hex_to_bytes32(&h2)
}

/// Decode a 64-char hex string into 32 raw bytes.
fn hex_to_bytes32(hex: &str) -> [u8; 32] {
    let mut buf = [0u8; 32];
    for i in 0..32 {
        if hex.len() >= i * 2 + 2 {
            buf[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap_or(0);
        }
    }
    buf
}

fn random_hex(n: usize) -> String {
    let mut seed = entropy_seed();
    let mut out = String::with_capacity(n);
    while out.len() < n {
        // Re-mix to extend.
        seed = {
            let h = hash_bytes(&seed);
            hex_to_bytes32(&h)
        };
        for b in &seed {
            if out.len() >= n {
                break;
            }
            out.push(hex_digit(b >> 4));
            if out.len() < n {
                out.push(hex_digit(b & 0x0F));
            }
        }
    }
    out
}

fn hex_digit(v: u8) -> char {
    match v {
        0..=9 => (b'0' + v) as char,
        10..=15 => (b'a' + (v - 10)) as char,
        _ => '0',
    }
}

fn gen_uuid_v4() -> String {
    let mut seed = entropy_seed();
    // Set version (4) and variant (10xx) bits per RFC 4122.
    seed[6] = (seed[6] & 0x0F) | 0x40;
    seed[8] = (seed[8] & 0x3F) | 0x80;
    let mut out = String::with_capacity(36);
    for (i, b) in seed.iter().take(16).enumerate() {
        if i == 4 || i == 6 || i == 8 || i == 10 {
            out.push('-');
        }
        out.push(hex_digit(b >> 4));
        out.push(hex_digit(b & 0x0F));
    }
    out
}

/// Find the index of `}}` starting from `chars`.
fn find_close(chars: &[char]) -> Option<usize> {
    let mut i = 0;
    while i + 1 < chars.len() {
        if chars[i] == '}' && chars[i + 1] == '}' {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Extract the set of `{{var}}` names referenced in a template (deduped, order-preserving).
/// Includes `{{include:path}}` directives and magic variables.
pub fn extract_vars(template: &str) -> Vec<String> {
    let chars: Vec<char> = template.chars().collect();
    let mut names = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{' && i + 1 < chars.len() && chars[i + 1] == '{' {
            if let Some(end) = find_close(&chars[i + 2..]) {
                let name: String = chars[i + 2..i + 2 + end].iter().collect();
                let name = name.trim().to_string();
                if !name.is_empty() && seen.insert(name.clone()) {
                    names.push(name);
                }
                i += 2 + end + 2;
            } else {
                break;
            }
        } else {
            i += 1;
        }
    }
    names
}

/// Return only the user-defined variable references (excludes magic vars and
/// `{{include:...}}` directives). Useful for surfacing "fill these in" prompts.
pub fn extract_user_vars(template: &str) -> Vec<String> {
    extract_vars(template)
        .into_iter()
        .filter(|n| !n.starts_with("include:"))
        .filter(|n| resolve_magic(n).is_none())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn basic_substitution() {
        let t = "Hello {{name}}, you are {{role}}.";
        let v = vars(&[("name", "Alice"), ("role", "admin")]);
        assert_eq!(render(t, &v, false).unwrap(), "Hello Alice, you are admin.");
    }

    #[test]
    fn whitespace_tolerant() {
        let t = "{{  name  }}";
        let v = vars(&[("name", "Bob")]);
        assert_eq!(render(t, &v, false).unwrap(), "Bob");
    }

    #[test]
    fn undefined_strict_errors() {
        let t = "{{missing}}";
        assert!(render(t, &HashMap::new(), true).is_err());
    }

    #[test]
    fn undefined_non_strict_preserved() {
        let t = "{{missing}}";
        assert_eq!(render(t, &HashMap::new(), false).unwrap(), "{{missing}}");
    }

    #[test]
    fn no_closing_braces_literal() {
        let t = "hello {{ world";
        assert_eq!(render(t, &HashMap::new(), false).unwrap(), "hello {{ world");
    }

    #[test]
    fn extract_vars_works() {
        let t = "{{a}} and {{b}} and {{a}}";
        let names = extract_vars(t);
        assert_eq!(names, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn preserves_unicode() {
        let t = "你是 {{role}}，请总结：{{text}}";
        let v = vars(&[("role", "助手"), ("text", "你好世界")]);
        assert_eq!(render(t, &v, false).unwrap(), "你是 助手，请总结：你好世界");
    }

    #[test]
    fn preserves_emoji() {
        let t = "{{emoji}} rocket";
        let v = vars(&[("emoji", "🚀")]);
        assert_eq!(render(t, &v, false).unwrap(), "🚀 rocket");
    }

    #[test]
    fn preserves_accented() {
        let t = "Café {{name}}";
        let v = vars(&[("name", "Résumé")]);
        assert_eq!(render(t, &v, false).unwrap(), "Café Résumé");
    }

    // ---- magic variable tests ----

    #[test]
    fn magic_date_resolves() {
        let t = "Today is {{date}}.";
        let out = render(t, &HashMap::new(), false).unwrap();
        assert!(out.starts_with("Today is 20") && out.len() == "Today is YYYY-MM-DD.".len(),
            "got: {out}");
    }

    #[test]
    fn magic_uuid_resolves() {
        let t = "id={{uuid}}";
        let out = render(t, &HashMap::new(), false).unwrap();
        assert_eq!(out.len(), "id=".len() + 36);
        assert_eq!(out.as_bytes()["id=".len() + 8], b'-');
    }

    #[test]
    fn magic_random_n_resolves() {
        let t = "code={{random:8}}";
        let out = render(t, &HashMap::new(), false).unwrap();
        assert_eq!(out.len(), "code=".len() + 8);
        assert!(out["code=".len()..].chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn magic_random_default_unknown_var_preserved() {
        let t = "{{random:}}";
        // Empty N isn't a valid magic var → falls through to undefined.
        let out = render(t, &HashMap::new(), false).unwrap();
        assert_eq!(out, "{{random:}}");
    }

    #[test]
    fn user_var_overrides_magic() {
        let t = "ts={{timestamp}}";
        let v = vars(&[("timestamp", "12345")]);
        assert_eq!(render(t, &v, false).unwrap(), "ts=12345");
    }

    #[test]
    fn magic_os_resolves() {
        let t = "platform={{os}}";
        let out = render(t, &HashMap::new(), false).unwrap();
        assert!(out.starts_with("platform=") && out.len() > "platform=".len());
    }

    #[test]
    fn extract_user_vars_filters_magic_and_includes() {
        let t = "{{name}} {{date}} {{include:partials/head.md}} {{uuid}} {{role}}";
        let uv = extract_user_vars(t);
        assert_eq!(uv, vec!["name".to_string(), "role".to_string()]);
    }

    #[test]
    fn strict_mode_treats_magic_var_as_defined() {
        // strict=true should NOT error on a magic var.
        let t = "{{date}}";
        let out = render(t, &HashMap::new(), true).unwrap();
        assert!(out.starts_with("20"));
    }
}
