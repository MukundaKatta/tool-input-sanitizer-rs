/*!
`tool-input-sanitizer`: sanitize, normalize, and validate LLM tool call inputs
before you hand them to real code.

Large language models routinely emit tool/function call arguments that are
*almost* right: padded with whitespace, wrapped in stray markdown, the wrong
case, far longer than your backend allows, or simply missing. This crate lets
you declare per-field [`SanitizeRule`]s once and apply them to every incoming
tool call, so the rest of your program can trust its inputs.

# Quick start

```rust
use tool_input_sanitizer::{InputSanitizer, SanitizeRule};
use serde_json::json;

let mut s = InputSanitizer::new();
s.add_rule("query", SanitizeRule::TrimWhitespace);
s.add_rule("query", SanitizeRule::MaxLength(100));

let result = s
    .sanitize("search", &json!({"query": "  hello world  "}))
    .unwrap();
assert_eq!(result["query"], "hello world");
```

# Normalization vs. validation

Most rules *normalize* (they always succeed and transform the value in place):
[`SanitizeRule::TrimWhitespace`], [`SanitizeRule::MaxLength`],
[`SanitizeRule::Lowercase`], [`SanitizeRule::Uppercase`],
[`SanitizeRule::StripChars`], and [`SanitizeRule::DefaultOnNull`].

A few rules *validate* (they leave the value untouched but return a
[`SanitizeError`] when the input is unacceptable): [`SanitizeRule::Required`],
[`SanitizeRule::NonEmpty`], [`SanitizeRule::MaxLengthError`], and
[`SanitizeRule::OneOf`]. Mixing the two lets you both clean up sloppy input and
reject input that can never be made valid:

```rust
use tool_input_sanitizer::{InputSanitizer, SanitizeRule};
use serde_json::json;

let mut s = InputSanitizer::new();
s.add_rule("mode", SanitizeRule::TrimWhitespace);
s.add_rule("mode", SanitizeRule::Lowercase);
s.add_rule(
    "mode",
    SanitizeRule::OneOf(vec!["read".into(), "write".into()]),
);

// Normalized then accepted.
assert_eq!(
    s.sanitize("fs", &json!({"mode": " READ "})).unwrap()["mode"],
    "read"
);

// Normalized then rejected.
assert!(s.sanitize("fs", &json!({"mode": "delete"})).is_err());
```
*/
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![doc(html_root_url = "https://docs.rs/tool-input-sanitizer")]

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
    /// Require the field to be present and non-null.
    ///
    /// Unlike [`SanitizeRule::DefaultOnNull`], this is a *validation* rule: a
    /// missing or null field produces a [`SanitizeError`] instead of being
    /// filled in. Useful for arguments the tool genuinely cannot run without.
    Required,
    /// Reject empty strings.
    ///
    /// Non-string values pass through unchanged. Add
    /// [`SanitizeRule::TrimWhitespace`] before this rule to also reject
    /// whitespace-only strings.
    NonEmpty,
    /// Reject strings longer than `N` characters with a [`SanitizeError`].
    ///
    /// Contrast with [`SanitizeRule::MaxLength`], which silently truncates.
    /// Use this when over-long input signals a bug or abuse rather than
    /// something to quietly fix.
    MaxLengthError(usize),
    /// Reject string values that are not one of the allowed options.
    ///
    /// Comparison is exact and case-sensitive; normalize first (e.g. with
    /// [`SanitizeRule::Lowercase`]) if you want case-insensitive matching.
    /// Non-string values pass through unchanged.
    OneOf(Vec<String>),
}

/// An error produced when a validation rule rejects a field value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SanitizeError {
    /// Name of the field that failed validation.
    pub field: String,
    /// Human-readable description of why the field was rejected.
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
    /// Create an empty sanitizer with no rules.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a rule for `field`.
    ///
    /// Rules for the same field are applied in insertion order, so you can
    /// chain a normalizer before a validator (e.g. trim, then `NonEmpty`).
    /// Returns `&mut self` so calls can be chained in a builder style.
    pub fn add_rule(&mut self, field: &str, rule: SanitizeRule) -> &mut Self {
        self.rules.entry(field.to_string()).or_default().push(rule);
        self
    }

    /// Return the number of distinct fields that have at least one rule.
    pub fn field_count(&self) -> usize {
        self.rules.len()
    }

    /// Sanitize a JSON object `args` for `tool_name`.
    ///
    /// Returns a new object with rules applied to matching fields. Fields that
    /// have no rules are passed through untouched, and a non-object `args`
    /// value (array, string, number, etc.) is returned unchanged.
    ///
    /// # Errors
    ///
    /// Returns a [`SanitizeError`] if any validation rule
    /// ([`SanitizeRule::Required`], [`SanitizeRule::NonEmpty`],
    /// [`SanitizeRule::MaxLengthError`], or [`SanitizeRule::OneOf`]) rejects a
    /// value. Evaluation stops at the first failing rule.
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
                // Field is absent: only a few rules are meaningful here.
                for rule in rules {
                    match rule {
                        SanitizeRule::DefaultOnNull(default) => {
                            result.insert(field.clone(), default.clone());
                        }
                        SanitizeRule::Required => {
                            return Err(SanitizeError {
                                field: field.clone(),
                                message: "required field is missing".to_string(),
                            });
                        }
                        _ => {}
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
        SanitizeRule::Required => {
            if val.is_null() {
                Err(SanitizeError {
                    field: field.to_string(),
                    message: "required field is null".to_string(),
                })
            } else {
                Ok(val.clone())
            }
        }
        SanitizeRule::NonEmpty => {
            if let Some(s) = val.as_str() {
                if s.is_empty() {
                    return Err(SanitizeError {
                        field: field.to_string(),
                        message: "value must not be empty".to_string(),
                    });
                }
            }
            Ok(val.clone())
        }
        SanitizeRule::MaxLengthError(n) => {
            if let Some(s) = val.as_str() {
                let len = s.chars().count();
                if len > *n {
                    return Err(SanitizeError {
                        field: field.to_string(),
                        message: format!("value is too long: {len} chars (max {n})"),
                    });
                }
            }
            Ok(val.clone())
        }
        SanitizeRule::OneOf(allowed) => {
            if let Some(s) = val.as_str() {
                if !allowed.iter().any(|a| a == s) {
                    return Err(SanitizeError {
                        field: field.to_string(),
                        message: format!("value '{s}' is not one of the allowed options"),
                    });
                }
            }
            Ok(val.clone())
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
        let e = SanitizeError {
            field: "q".into(),
            message: "bad".into(),
        };
        assert!(e.to_string().contains("q"));
    }

    #[test]
    fn required_present_ok() {
        let mut s = InputSanitizer::new();
        s.add_rule("q", SanitizeRule::Required);
        let r = s.sanitize("fn", &json!({"q": "x"})).unwrap();
        assert_eq!(r["q"], "x");
    }

    #[test]
    fn required_missing_errors() {
        let mut s = InputSanitizer::new();
        s.add_rule("q", SanitizeRule::Required);
        let err = s.sanitize("fn", &json!({})).unwrap_err();
        assert_eq!(err.field, "q");
        assert!(err.message.contains("missing"));
    }

    #[test]
    fn required_null_errors() {
        let mut s = InputSanitizer::new();
        s.add_rule("q", SanitizeRule::Required);
        let err = s.sanitize("fn", &json!({"q": null})).unwrap_err();
        assert_eq!(err.field, "q");
        assert!(err.message.contains("null"));
    }

    #[test]
    fn non_empty_rejects_empty_string() {
        let mut s = InputSanitizer::new();
        s.add_rule("q", SanitizeRule::NonEmpty);
        assert!(s.sanitize("fn", &json!({"q": ""})).is_err());
    }

    #[test]
    fn non_empty_accepts_non_empty_string() {
        let mut s = InputSanitizer::new();
        s.add_rule("q", SanitizeRule::NonEmpty);
        assert!(s.sanitize("fn", &json!({"q": "ok"})).is_ok());
    }

    #[test]
    fn non_empty_after_trim_rejects_whitespace_only() {
        let mut s = InputSanitizer::new();
        s.add_rule("q", SanitizeRule::TrimWhitespace);
        s.add_rule("q", SanitizeRule::NonEmpty);
        assert!(s.sanitize("fn", &json!({"q": "   "})).is_err());
    }

    #[test]
    fn non_empty_ignores_non_strings() {
        let mut s = InputSanitizer::new();
        s.add_rule("n", SanitizeRule::NonEmpty);
        assert!(s.sanitize("fn", &json!({"n": 0})).is_ok());
    }

    #[test]
    fn max_length_error_rejects_too_long() {
        let mut s = InputSanitizer::new();
        s.add_rule("q", SanitizeRule::MaxLengthError(3));
        let err = s.sanitize("fn", &json!({"q": "hello"})).unwrap_err();
        assert_eq!(err.field, "q");
        assert!(err.message.contains("too long"));
    }

    #[test]
    fn max_length_error_accepts_within_limit() {
        let mut s = InputSanitizer::new();
        s.add_rule("q", SanitizeRule::MaxLengthError(5));
        assert!(s.sanitize("fn", &json!({"q": "hello"})).is_ok());
    }

    #[test]
    fn max_length_error_counts_chars_not_bytes() {
        let mut s = InputSanitizer::new();
        s.add_rule("q", SanitizeRule::MaxLengthError(3));
        // "héllo" is 5 chars but more than 5 bytes; must reject on char count.
        assert!(s.sanitize("fn", &json!({"q": "héllo"})).is_err());
        assert!(s.sanitize("fn", &json!({"q": "hél"})).is_ok());
    }

    #[test]
    fn one_of_accepts_allowed() {
        let mut s = InputSanitizer::new();
        s.add_rule(
            "mode",
            SanitizeRule::OneOf(vec!["read".into(), "write".into()]),
        );
        assert!(s.sanitize("fn", &json!({"mode": "read"})).is_ok());
    }

    #[test]
    fn one_of_rejects_disallowed() {
        let mut s = InputSanitizer::new();
        s.add_rule(
            "mode",
            SanitizeRule::OneOf(vec!["read".into(), "write".into()]),
        );
        let err = s.sanitize("fn", &json!({"mode": "delete"})).unwrap_err();
        assert_eq!(err.field, "mode");
        assert!(err.message.contains("delete"));
    }

    #[test]
    fn normalize_then_validate_pipeline() {
        let mut s = InputSanitizer::new();
        s.add_rule("mode", SanitizeRule::TrimWhitespace);
        s.add_rule("mode", SanitizeRule::Lowercase);
        s.add_rule(
            "mode",
            SanitizeRule::OneOf(vec!["read".into(), "write".into()]),
        );
        let r = s.sanitize("fn", &json!({"mode": "  READ "})).unwrap();
        assert_eq!(r["mode"], "read");
        assert!(s.sanitize("fn", &json!({"mode": "  DELETE "})).is_err());
    }

    #[test]
    fn add_rule_is_chainable() {
        let mut s = InputSanitizer::new();
        s.add_rule("q", SanitizeRule::TrimWhitespace)
            .add_rule("q", SanitizeRule::Uppercase);
        let r = s.sanitize("fn", &json!({"q": "  hi "})).unwrap();
        assert_eq!(r["q"], "HI");
    }

    #[test]
    fn field_count_tracks_distinct_fields() {
        let mut s = InputSanitizer::new();
        assert_eq!(s.field_count(), 0);
        s.add_rule("a", SanitizeRule::TrimWhitespace);
        s.add_rule("a", SanitizeRule::Uppercase);
        s.add_rule("b", SanitizeRule::Lowercase);
        assert_eq!(s.field_count(), 2);
    }
}
