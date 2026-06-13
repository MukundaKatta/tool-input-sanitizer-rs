# tool-input-sanitizer

Sanitize and normalize LLM tool call inputs before execution.

When a language model decides to call a tool, the arguments it produces are
not always clean: strings may carry stray whitespace, exceed length limits,
contain unwanted characters, or be missing entirely. `tool-input-sanitizer`
lets you declare per-field rules and apply them to a tool call's JSON
arguments, returning a normalized object that is safe to hand off to the
actual tool implementation.

## Features

- Declarative, per-field sanitization rules applied in the order you add them.
- Operates directly on `serde_json::Value` objects (the natural shape of tool
  call arguments).
- Non-string fields and unrelated fields are passed through untouched.
- Unicode-safe length truncation (counts characters, not bytes).
- No-op on non-object arguments, so it is safe to call unconditionally.

### Available rules

| Rule | Effect |
| --- | --- |
| `TrimWhitespace` | Trims leading/trailing whitespace from string values. |
| `MaxLength(n)` | Truncates strings to at most `n` characters (Unicode-safe). |
| `Lowercase` | Converts a string to lowercase. |
| `Uppercase` | Converts a string to uppercase. |
| `StripChars(chars)` | Removes any character found in `chars` from the string. |
| `DefaultOnNull(value)` | Substitutes a default when the field is `null` or absent. |

## Installation

Add the crate to your `Cargo.toml`:

```toml
[dependencies]
tool-input-sanitizer = "0.1"
serde_json = "1"
```

## Usage

```rust
use tool_input_sanitizer::{InputSanitizer, SanitizeRule};
use serde_json::json;

let mut s = InputSanitizer::new();
s.add_rule("query", SanitizeRule::TrimWhitespace);
s.add_rule("query", SanitizeRule::MaxLength(100));

let result = s.sanitize("search", &json!({ "query": "  hello world  " })).unwrap();
assert_eq!(result["query"], "hello world");
```

Rules added to the same field are applied in order, so they compose:

```rust
use tool_input_sanitizer::{InputSanitizer, SanitizeRule};
use serde_json::json;

let mut s = InputSanitizer::new();
s.add_rule("q", SanitizeRule::TrimWhitespace);
s.add_rule("q", SanitizeRule::Uppercase);

let result = s.sanitize("fn", &json!({ "q": "  hello  " })).unwrap();
assert_eq!(result["q"], "HELLO");
```

Fields without rules are preserved as-is, and `DefaultOnNull` fills in missing
or null fields:

```rust
use tool_input_sanitizer::{InputSanitizer, SanitizeRule};
use serde_json::json;

let mut s = InputSanitizer::new();
s.add_rule("limit", SanitizeRule::DefaultOnNull(json!(10)));

let result = s.sanitize("fn", &json!({})).unwrap();
assert_eq!(result["limit"], 10);
```

## API overview

- `InputSanitizer::new()` / `InputSanitizer::default()` — create an empty sanitizer.
- `InputSanitizer::add_rule(field, rule)` — register a `SanitizeRule` for a field.
- `InputSanitizer::sanitize(tool_name, args)` — apply all rules to `args`,
  returning a new `serde_json::Value` or a `SanitizeError`.

## Tech stack

- Language: Rust (edition 2021)
- Dependency: [`serde_json`](https://crates.io/crates/serde_json)

## Testing

```sh
cargo test
```

## License

Licensed under the MIT License.
