//! Tiny `{{var}}` template renderer.
//!
//! Supports `{{name}}` and `{{ name }}` (whitespace-tolerant).
//! In strict mode, an undefined variable is an error; otherwise it is left as-is.
//!
//! Works on `char` boundaries (UTF-8 safe), so Chinese / emoji / accented
//! content is preserved exactly.

use std::collections::{HashMap, HashSet};

use anyhow::{bail, Result};

/// Render `template` by substituting `{{var}}` with values from `vars`.
///
/// - `strict = true`:  any `{{var}}` not present in `vars` is an error.
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
                    None => {
                        if strict {
                            bail!("undefined template variable: {name}");
                        }
                        out.push_str("{{");
                        out.push_str(&name_raw);
                        out.push_str("}}");
                    }
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
#[allow(dead_code)]
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
}
