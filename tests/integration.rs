//! Integration tests exercising the public API as an external consumer would.

use serde_json::json;
use tool_input_sanitizer::{InputSanitizer, SanitizeRule};

#[test]
fn realistic_search_tool_pipeline() {
    let mut s = InputSanitizer::new();
    s.add_rule("query", SanitizeRule::TrimWhitespace);
    s.add_rule("query", SanitizeRule::NonEmpty);
    s.add_rule("query", SanitizeRule::MaxLength(64));

    let out = s
        .sanitize(
            "web_search",
            &json!({"query": "   rust crates  ", "limit": 5}),
        )
        .unwrap();

    assert_eq!(out["query"], "rust crates");
    // Unrelated fields are preserved untouched.
    assert_eq!(out["limit"], 5);
}

#[test]
fn rejects_invalid_enum_and_reports_field() {
    let mut s = InputSanitizer::new();
    s.add_rule("direction", SanitizeRule::Lowercase);
    s.add_rule(
        "direction",
        SanitizeRule::OneOf(vec!["asc".into(), "desc".into()]),
    );

    let ok = s.sanitize("sort", &json!({"direction": "ASC"})).unwrap();
    assert_eq!(ok["direction"], "asc");

    let err = s
        .sanitize("sort", &json!({"direction": "sideways"}))
        .unwrap_err();
    assert_eq!(err.field, "direction");
    assert!(err.to_string().contains("direction"));
}

#[test]
fn required_field_enforced_end_to_end() {
    let mut s = InputSanitizer::new();
    s.add_rule("path", SanitizeRule::Required);
    s.add_rule("path", SanitizeRule::StripChars("\0".into()));

    assert!(s.sanitize("read_file", &json!({})).is_err());
    assert!(s.sanitize("read_file", &json!({"path": null})).is_err());
    assert!(s
        .sanitize("read_file", &json!({"path": "/etc/hosts"}))
        .is_ok());
}

#[test]
fn default_fills_missing_optional_field() {
    let mut s = InputSanitizer::new();
    s.add_rule("format", SanitizeRule::DefaultOnNull(json!("json")));

    let filled = s.sanitize("export", &json!({})).unwrap();
    assert_eq!(filled["format"], "json");

    let respected = s.sanitize("export", &json!({"format": "csv"})).unwrap();
    assert_eq!(respected["format"], "csv");
}

#[test]
fn non_object_arguments_pass_through_unchanged() {
    let s = InputSanitizer::new();
    assert_eq!(
        s.sanitize("x", &json!("plain string")).unwrap(),
        "plain string"
    );
    assert_eq!(s.sanitize("x", &json!(42)).unwrap(), 42);
    assert_eq!(
        s.sanitize("x", &json!([1, 2, 3])).unwrap(),
        json!([1, 2, 3])
    );
}
