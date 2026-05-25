/*!
tool-input-sanitizer: sanitize and normalize LLM tool call inputs.

```rust
use tool_input_sanitizer::{InputSanitizer, SanitizeRule};
use serde_json::json;

let mut s = InputSanitizer::new();
s.add_rule("query", SanitizeRule::TrimWhitespace);
s.add_rule("query", SanitizeRule::MaxLength(100));
let result = s.sanitize("search", &json!({"query": "  hello world  "})).unwrap();
assert_eq!(result["query"], "hello world");
```
*/

use serde_json::Value;
use std::collections::HashMap;
use std::fmt;

/// A sanitization rule for a field.
#[derive(Debug, Clone)]
pub enum SanitizeRule {
    /// Trim leading/trailing whitespace from strings.
    TrimWhitespace,
    /// Truncate strings to at most N characters.
    MaxLength(usize),
    /// Convert string to lowercase.
    Lowercase,
    /// Convert string to uppercase.
    Uppercase,
    /// Remove characters matching a set (provided as a string of chars to strip).
    StripChars(String),
    /// Replace nulls with a default value.
    DefaultOnNull(Value),
}

/// Errors from sanitization.
#[derive(Debug)]
pub struct SanitizeError {
    pub field: String,
    pub message: String,
}

impl fmt::Display for SanitizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sanitize error on '{}': {}", self.field, self.message)
    }
}

impl std::error::Error for SanitizeError {}

/// Applies sanitization rules to tool call arguments.
#[derive(Debug, Default)]
pub struct InputSanitizer {
    /// Rules per field name. Applied in order.
    rules: HashMap<String, Vec<SanitizeRule>>,
}

impl InputSanitizer {
    pub fn new() -> Self { Self::default() }

    /// Add a rule for `field`.
    pub fn add_rule(&mut self, field: &str, rule: SanitizeRule) {
        self.rules.entry(field.to_string()).or_default().push(rule);
    }

    /// Sanitize a JSON object `args` for `tool_name`.
    /// Returns a new object with rules applied to matching fields.
    pub fn sanitize(&self, _tool_name: &str, args: &Value) -> Result<Value, SanitizeError> {
        let obj = match args.as_object() {
            Some(o) => o,
            None => return Ok(args.clone()),
        };

        let mut result = obj.clone();
        for (field, rules) in &self.rules {
            if let Some(val) = result.get_mut(field) {
                for rule in rules {
                    *val = apply_rule(field, val, rule)?;
                }
            } else {
                // Apply DefaultOnNull if field is absent.
                for rule in rules {
                    if let SanitizeRule::DefaultOnNull(default) = rule {
                        result.insert(field.clone(), default.clone());
                    }
                }
            }
        }
        Ok(Value::Object(result))
    }
}

fn apply_rule(field: &str, val: &Value, rule: &SanitizeRule) -> Result<Value, SanitizeError> {
    match rule {
        SanitizeRule::TrimWhitespace => {
            if let Some(s) = val.as_str() {
                Ok(Value::String(s.trim().to_string()))
            } else {
                Ok(val.clone())
            }
        }
        SanitizeRule::MaxLength(n) => {
            if let Some(s) = val.as_str() {
                Ok(Value::String(s.chars().take(*n).collect()))
            } else {
                Ok(val.clone())
            }
        }
        SanitizeRule::Lowercase => {
            if let Some(s) = val.as_str() {
                Ok(Value::String(s.to_lowercase()))
            } else {
                Ok(val.clone())
            }
        }
        SanitizeRule::Uppercase => {
            if let Some(s) = val.as_str() {
                Ok(Value::String(s.to_uppercase()))
            } else {
                Ok(val.clone())
            }
        }
        SanitizeRule::StripChars(chars) => {
            if let Some(s) = val.as_str() {
                let stripped: String = s.chars().filter(|c| !chars.contains(*c)).collect();
                Ok(Value::String(stripped))
            } else {
                Ok(val.clone())
            }
        }
        SanitizeRule::DefaultOnNull(default) => {
            if val.is_null() {
                Ok(default.clone())
            } else {
                Ok(val.clone())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn trim_whitespace() {
        let mut s = InputSanitizer::new();
        s.add_rule("q", SanitizeRule::TrimWhitespace);
        let r = s.sanitize("fn", &json!({"q": "  hello  "})).unwrap();
        assert_eq!(r["q"], "hello");
    }

    #[test]
    fn max_length_truncates() {
        let mut s = InputSanitizer::new();
        s.add_rule("q", SanitizeRule::MaxLength(5));
        let r = s.sanitize("fn", &json!({"q": "hello world"})).unwrap();
        assert_eq!(r["q"], "hello");
    }

    #[test]
    fn lowercase() {
        let mut s = InputSanitizer::new();
        s.add_rule("q", SanitizeRule::Lowercase);
        let r = s.sanitize("fn", &json!({"q": "HELLO"})).unwrap();
        assert_eq!(r["q"], "hello");
    }

    #[test]
    fn uppercase() {
        let mut s = InputSanitizer::new();
        s.add_rule("q", SanitizeRule::Uppercase);
        let r = s.sanitize("fn", &json!({"q": "hello"})).unwrap();
        assert_eq!(r["q"], "HELLO");
    }

    #[test]
    fn strip_chars() {
        let mut s = InputSanitizer::new();
        s.add_rule("q", SanitizeRule::StripChars("<>".to_string()));
        let r = s.sanitize("fn", &json!({"q": "<script>"})).unwrap();
        assert_eq!(r["q"], "script");
    }

    #[test]
    fn default_on_null_field_present() {
        let mut s = InputSanitizer::new();
        s.add_rule("q", SanitizeRule::DefaultOnNull(json!("default")));
        let r = s.sanitize("fn", &json!({"q": null})).unwrap();
        assert_eq!(r["q"], "default");
    }

    #[test]
    fn default_on_null_field_absent() {
        let mut s = InputSanitizer::new();
        s.add_rule("missing", SanitizeRule::DefaultOnNull(json!("fallback")));
        let r = s.sanitize("fn", &json!({})).unwrap();
        assert_eq!(r["missing"], "fallback");
    }

    #[test]
    fn rules_applied_in_order() {
        let mut s = InputSanitizer::new();
        s.add_rule("q", SanitizeRule::TrimWhitespace);
        s.add_rule("q", SanitizeRule::Uppercase);
        let r = s.sanitize("fn", &json!({"q": "  hello  "})).unwrap();
        assert_eq!(r["q"], "HELLO");
    }

    #[test]
    fn non_string_field_unchanged_by_string_rules() {
        let mut s = InputSanitizer::new();
        s.add_rule("n", SanitizeRule::TrimWhitespace);
        let r = s.sanitize("fn", &json!({"n": 42})).unwrap();
        assert_eq!(r["n"], 42);
    }

    #[test]
    fn non_object_args_returned_as_is() {
        let s = InputSanitizer::new();
        let r = s.sanitize("fn", &json!([1, 2, 3])).unwrap();
        assert_eq!(r, json!([1, 2, 3]));
    }

    #[test]
    fn no_rules_returns_unchanged() {
        let s = InputSanitizer::new();
        let input = json!({"a": "hello", "b": 42});
        let r = s.sanitize("fn", &input).unwrap();
        assert_eq!(r, input);
    }

    #[test]
    fn unrelated_fields_preserved() {
        let mut s = InputSanitizer::new();
        s.add_rule("q", SanitizeRule::Uppercase);
        let r = s.sanitize("fn", &json!({"q": "hi", "extra": 99})).unwrap();
        assert_eq!(r["q"], "HI");
        assert_eq!(r["extra"], 99);
    }

    #[test]
    fn max_length_unicode_safe() {
        let mut s = InputSanitizer::new();
        s.add_rule("q", SanitizeRule::MaxLength(3));
        let r = s.sanitize("fn", &json!({"q": "héllo"})).unwrap();
        let result = r["q"].as_str().unwrap();
        assert_eq!(result.chars().count(), 3);
    }

    #[test]
    fn error_display() {
        let e = SanitizeError { field: "q".into(), message: "bad".into() };
        assert!(e.to_string().contains("q"));
    }
}
