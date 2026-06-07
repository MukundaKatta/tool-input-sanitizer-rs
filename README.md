# tool-input-sanitizer

Sanitize, normalize, and validate LLM tool-call inputs before you execute them.

Large language models routinely emit tool/function-call arguments that are
*almost* right: padded with whitespace, in the wrong case, far longer than your
backend allows, an unexpected enum value, or simply missing. `tool-input-sanitizer`
lets you declare per-field rules once and apply them to every incoming tool call,
so the rest of your program can trust its inputs.

## Features

- **Normalization rules** that always succeed and clean values in place:
  trim whitespace, truncate to a max length, lowercase/uppercase, strip
  unwanted characters, and substitute a default for `null`.
- **Validation rules** that reject unacceptable input with a structured error:
  require a field, forbid empty strings, enforce a hard length limit, and
  restrict to an allow-list of values.
- **Order-preserving pipelines** — chain a normalizer before a validator
  (e.g. trim → lowercase → `OneOf`) on the same field.
- Operates on `serde_json::Value` objects, leaving unrelated fields untouched.
- No `unsafe`, one dependency (`serde_json`).

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

fn main() {
    let mut sanitizer = InputSanitizer::new();

    // Normalize the `query` field, then enforce constraints on it.
    sanitizer.add_rule("query", SanitizeRule::TrimWhitespace);
    sanitizer.add_rule("query", SanitizeRule::NonEmpty);
    sanitizer.add_rule("query", SanitizeRule::MaxLength(64));

    // Restrict `mode` to a known set, case-insensitively.
    sanitizer.add_rule("mode", SanitizeRule::Lowercase);
    sanitizer.add_rule(
        "mode",
        SanitizeRule::OneOf(vec!["read".into(), "write".into()]),
    );

    let raw = json!({"query": "  hello world  ", "mode": "READ"});
    let clean = sanitizer.sanitize("my_tool", &raw).unwrap();

    assert_eq!(clean["query"], "hello world");
    assert_eq!(clean["mode"], "read");

    // Invalid input is rejected with a structured error.
    let bad = json!({"query": "ok", "mode": "delete"});
    let err = sanitizer.sanitize("my_tool", &bad).unwrap_err();
    assert_eq!(err.field, "mode");
    println!("{err}"); // sanitize error on 'mode': value 'delete' is not one of the allowed options
}
```

## API

### `InputSanitizer`

| Method | Description |
| --- | --- |
| `InputSanitizer::new()` | Create an empty sanitizer with no rules. |
| `add_rule(field, rule) -> &mut Self` | Add a rule for `field`. Rules on the same field run in insertion order; returns `&mut self` for chaining. |
| `field_count() -> usize` | Number of distinct fields that have at least one rule. |
| `sanitize(tool_name, args) -> Result<Value, SanitizeError>` | Apply the rules to `args` and return a new value, or a `SanitizeError` if a validation rule fails. A non-object `args` is returned unchanged. |

### `SanitizeRule`

Normalization rules (always succeed):

| Rule | Effect on string values |
| --- | --- |
| `TrimWhitespace` | Remove leading/trailing whitespace. |
| `MaxLength(n)` | Truncate to at most `n` characters. |
| `Lowercase` | Convert to lowercase. |
| `Uppercase` | Convert to uppercase. |
| `StripChars(chars)` | Remove every character contained in `chars`. |
| `DefaultOnNull(value)` | Replace a `null` or absent field with `value`. |

Validation rules (leave the value untouched, return a `SanitizeError` on failure):

| Rule | Rejects when |
| --- | --- |
| `Required` | The field is missing or `null`. |
| `NonEmpty` | The string is empty (add `TrimWhitespace` first to also reject whitespace-only). |
| `MaxLengthError(n)` | The string is longer than `n` characters (vs. `MaxLength`, which silently truncates). |
| `OneOf(options)` | The string is not exactly equal to one of `options`. |

Non-string values pass through string-oriented rules unchanged.

### `SanitizeError`

Returned by `sanitize` when a validation rule fails. It implements
`std::error::Error` and `Display`, and exposes two public fields:

- `field: String` — the field that failed validation.
- `message: String` — a human-readable reason.

## Development

```sh
cargo build
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

## License

Licensed under the MIT License.
