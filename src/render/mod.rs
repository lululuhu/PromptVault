//! Tiny `{{var}}` template renderer.
//!
//! Supports `{{name}}` and `{{ name }}` (whitespace-tolerant).
//! In strict mode, an undefined variable is an error; otherwise it is left as-is.

use std::collections::HashMap;

use anyhow::{bail, Result};

/// Render `template` by substituting `{{var}}` with values from `vars`.
///
/// - `strict = true`:  any `{{var}}` not present in `vars` is an error.
/// - `strict = false`: undefined `{{var}}` is left verbatim in the output.
pub fn render(template: &str, vars: &HashMap<String, String>, strict: bool) -> Result<String> {
    let bytes = template.as_bytes();
    let mut out = String::with_capacity(template.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            // Find the closing }}.
            if let Some(end) = find_close(&bytes[i + 2..]) {
                let name_raw = &template[i + 2..i + 2 + end];
                let name = name_raw.trim();
                if name.is_empty() {
                    bail!("empty variable name at position {i}");
                }
                match vars.get(name) {
                    Some(v) => out.push_str(v),
                    None => {
                        if strict {
                            bail!("undefined variable: {{{{{name}}}}}");
                        }
                        out.push_str("{{");
                        out.push_str(name_raw);
                        out.push_str("}}");
                    }
                }
                i += 2 + end + 2;
            } else {
                // No closing }}: treat the rest as literal.
                out.push_str(&template[i..]);
                break;
            }
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    Ok(out)
}

/// Find the index of `}}` starting from `bytes`.
fn find_close(bytes: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'}' && bytes[i + 1] == b'}' {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Extract the set of `{{var}}` names referenced in a template.
#[allow(dead_code)]
pub fn extract_vars(template: &str) -> Vec<String> {
    let mut names = Vec::new();
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            if let Some(end) = find_close(&bytes[i + 2..]) {
                let name = template[i + 2..i + 2 + end].trim().to_string();
                if !name.is_empty() && !names.contains(&name) {
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
}
