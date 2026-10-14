//! A tiny expression language for tests and variable extraction.
//!
//! **Subjects** pick a value from the response:
//!
//! | Subject          | Value                                  |
//! |------------------|----------------------------------------|
//! | `status`         | Status code, e.g. `200`                |
//! | `time`           | Total time in milliseconds             |
//! | `size`           | Body size in bytes                     |
//! | `body`           | Body as text                           |
//! | `header(name)`   | A response header (case-insensitive)   |
//! | `json(path)`     | A JSONPath query, e.g. `json($.id)`    |
//!
//! **Tests** compare a subject to a value:
//!
//! ```text
//! status == 200
//! time < 500
//! header(content-type) contains json
//! json($.items) exists
//! json($.items[0].name) == "Alice"
//! body matches ^\{.*\}$
//! ```
//!
//! Operators: `==` `!=` `<` `<=` `>` `>=` `contains` `!contains`
//! `exists` `!exists` `matches` `!matches`.

use crate::error::{Error, Result};
use serde_json::Value;
use serde_json_path::JsonPath;

/// The parts of a response that expressions can read.
#[derive(Debug, Clone, Copy)]
pub struct ResponseData<'a> {
    pub status: u16,
    pub headers: &'a [(String, String)],
    pub body: &'a [u8],
    pub time_ms: f64,
}

/// What an expression reads from the response.
#[derive(Debug, Clone, PartialEq)]
pub enum Subject {
    Status,
    Time,
    Size,
    Body,
    Header(String),
    Json(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Contains,
    NotContains,
    Exists,
    NotExists,
    Matches,
    NotMatches,
}

impl Op {
    const ALL: &'static [(&'static str, Op)] = &[
        // Longer tokens first, so `<=` is not read as `<`.
        ("!contains", Op::NotContains),
        ("contains", Op::Contains),
        ("!exists", Op::NotExists),
        ("exists", Op::Exists),
        ("!matches", Op::NotMatches),
        ("matches", Op::Matches),
        ("==", Op::Eq),
        ("!=", Op::Ne),
        ("<=", Op::Le),
        (">=", Op::Ge),
        ("<", Op::Lt),
        (">", Op::Gt),
    ];

    fn needs_value(self) -> bool {
        !matches!(self, Op::Exists | Op::NotExists)
    }
}

/// A parsed test such as `status == 200`.
#[derive(Debug, Clone, PartialEq)]
pub struct Assertion {
    pub source: String,
    pub subject: Subject,
    pub op: Op,
    pub expected: Option<Value>,
}

/// The outcome of one assertion.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct AssertionResult {
    pub name: String,
    pub passed: bool,
    /// Explains a failure, e.g. "expected 200, got 404".
    #[ts(optional)]
    pub detail: Option<String>,
}

fn expr_error(expr: &str, reason: impl Into<String>) -> Error {
    Error::Expression { expr: expr.to_string(), reason: reason.into() }
}

/// Parses a subject at the start of `input` and returns the rest.
pub fn parse_subject(input: &str) -> Result<(Subject, &str)> {
    let s = input.trim_start();
    for (word, subject) in [("status", Subject::Status), ("time", Subject::Time), ("size", Subject::Size), ("body", Subject::Body)] {
        if let Some(rest) = s.strip_prefix(word)
            && !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_' || c == '(')
        {
            return Ok((subject, rest));
        }
    }
    for (word, make) in [("header(", Subject::Header as fn(String) -> Subject), ("json(", Subject::Json)] {
        if let Some(after) = s.strip_prefix(word) {
            let close = matching_paren(after).ok_or_else(|| expr_error(input, format!("missing `)` after `{word}`")))?;
            let arg = after[..close].trim();
            if arg.is_empty() {
                return Err(expr_error(input, format!("`{word})` needs an argument")));
            }
            let subject = make(arg.to_string());
            if let Subject::Json(path) = &subject {
                JsonPath::parse(path).map_err(|e| expr_error(input, format!("invalid JSONPath `{path}`: {e}")))?;
            }
            return Ok((subject, &after[close + 1..]));
        }
    }
    Err(expr_error(input, "expected a subject: status, time, size, body, header(name) or json($.path)"))
}

/// Index of the `)` that closes an already-opened `(`.
fn matching_paren(s: &str) -> Option<usize> {
    let mut depth = 1;
    let mut in_string: Option<char> = None;
    for (i, c) in s.char_indices() {
        match (c, in_string) {
            ('"' | '\'', None) => in_string = Some(c),
            (q, Some(open)) if q == open => in_string = None,
            ('(', None) => depth += 1,
            (')', None) => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

impl Assertion {
    /// Parses a test line.
    pub fn parse(source: &str) -> Result<Self> {
        let (subject, rest) = parse_subject(source)?;
        let rest = rest.trim_start();
        let (op, rest) = Op::ALL.iter().find_map(|(token, op)| rest.strip_prefix(token).map(|r| (*op, r))).ok_or_else(|| {
            expr_error(source, "expected an operator: == != < <= > >= contains exists matches (or !contains !exists !matches)")
        })?;
        let value_text = rest.trim();
        let expected = match (op.needs_value(), value_text.is_empty()) {
            (true, true) => return Err(expr_error(source, "missing the value to compare with")),
            (false, false) => return Err(expr_error(source, format!("`exists` takes no value, found `{value_text}`"))),
            (false, true) => None,
            (true, false) => Some(parse_value(value_text)),
        };
        if matches!(op, Op::Matches | Op::NotMatches)
            && let Some(Value::String(pattern)) = &expected
        {
            regex::Regex::new(pattern).map_err(|e| expr_error(source, format!("invalid regular expression: {e}")))?;
        }
        Ok(Self { source: source.trim().to_string(), subject, op, expected })
    }

    /// Checks the assertion against a response.
    pub fn evaluate(&self, response: &ResponseData<'_>) -> AssertionResult {
        let actual = read(&self.subject, response);
        let (passed, detail) = self.compare(actual.as_ref());
        AssertionResult { name: self.source.clone(), passed, detail: (!passed).then_some(detail) }
    }

    fn compare(&self, actual: Option<&Value>) -> (bool, String) {
        let shown = actual.map_or_else(|| "nothing".to_string(), display);
        let expected = self.expected.as_ref();
        let expected_shown = expected.map(display).unwrap_or_default();
        match self.op {
            Op::Exists => (actual.is_some_and(|v| !v.is_null()), "the value does not exist".into()),
            Op::NotExists => (actual.is_none_or(Value::is_null), format!("expected nothing, got {shown}")),
            Op::Eq => (actual.is_some_and(|a| loose_eq(a, expected.unwrap())), format!("expected {expected_shown}, got {shown}")),
            Op::Ne => (!actual.is_some_and(|a| loose_eq(a, expected.unwrap())), format!("expected anything but {expected_shown}")),
            Op::Lt | Op::Le | Op::Gt | Op::Ge => {
                let ord = actual.and_then(as_number).zip(expected.and_then(as_number));
                let passed = ord.is_some_and(|(a, e)| match self.op {
                    Op::Lt => a < e,
                    Op::Le => a <= e,
                    Op::Gt => a > e,
                    _ => a >= e,
                });
                let symbol = match self.op {
                    Op::Lt => "<",
                    Op::Le => "<=",
                    Op::Gt => ">",
                    _ => ">=",
                };
                (passed, format!("expected a number {symbol} {expected_shown}, got {shown}"))
            }
            Op::Contains | Op::NotContains => {
                let found = actual.is_some_and(|a| contains(a, expected.unwrap()));
                let passed = found == (self.op == Op::Contains);
                let verb = if self.op == Op::Contains { "to contain" } else { "not to contain" };
                (passed, format!("expected {shown} {verb} {expected_shown}"))
            }
            Op::Matches | Op::NotMatches => {
                let pattern = expected.map(as_text).unwrap_or_default();
                let found = regex::Regex::new(&pattern).is_ok_and(|re| actual.is_some_and(|a| re.is_match(&as_text(a))));
                let passed = found == (self.op == Op::Matches);
                let verb = if self.op == Op::Matches { "to match" } else { "not to match" };
                (passed, format!("expected {shown} {verb} /{pattern}/"))
            }
        }
    }
}

/// Reads a subject's value from a response. `None` means "does not exist".
pub fn read(subject: &Subject, response: &ResponseData<'_>) -> Option<Value> {
    match subject {
        Subject::Status => Some(response.status.into()),
        Subject::Time => serde_json::Number::from_f64((response.time_ms * 100.0).round() / 100.0).map(Value::Number),
        Subject::Size => Some(response.body.len().into()),
        Subject::Body => Some(String::from_utf8_lossy(response.body).into_owned().into()),
        Subject::Header(name) => response.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| Value::String(v.clone())),
        Subject::Json(path) => {
            let body: Value = serde_json::from_slice(response.body).ok()?;
            let path = JsonPath::parse(path).ok()?;
            let nodes = path.query(&body).all();
            match nodes.len() {
                0 => None,
                1 => Some(nodes[0].clone()),
                _ => Some(Value::Array(nodes.into_iter().cloned().collect())),
            }
        }
    }
}

/// Evaluates an extraction expression such as `json($.token)` to a string.
pub fn extract(expr: &str, response: &ResponseData<'_>) -> Result<Option<String>> {
    let (subject, rest) = parse_subject(expr)?;
    if !rest.trim().is_empty() {
        return Err(expr_error(expr, format!("unexpected `{}` after the subject", rest.trim())));
    }
    Ok(read(&subject, response).map(|v| as_text(&v)))
}

/// Interprets a written value: JSON if it parses (`200`, `"a"`, `true`), otherwise plain text.
fn parse_value(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string()))
}

fn as_number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Strings without quotes; everything else as JSON.
pub fn as_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn display(v: &Value) -> String {
    let text = v.to_string();
    if text.chars().count() > 120 { format!("{}…", text.chars().take(120).collect::<String>()) } else { text }
}

/// Equality that treats `200` and `"200"` as equal, since headers are always text.
fn loose_eq(actual: &Value, expected: &Value) -> bool {
    if actual == expected {
        return true;
    }
    match (as_number(actual), as_number(expected)) {
        (Some(a), Some(e)) => a == e,
        _ => matches!((actual, expected), (Value::String(_), _) | (_, Value::String(_))) && as_text(actual) == as_text(expected),
    }
}

fn contains(haystack: &Value, needle: &Value) -> bool {
    match haystack {
        Value::Array(items) => items.iter().any(|item| loose_eq(item, needle)),
        Value::Object(map) => map.contains_key(&as_text(needle)),
        other => as_text(other).contains(&as_text(needle)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(body: &str) -> (Vec<(String, String)>, Vec<u8>) {
        (vec![("Content-Type".into(), "application/json; charset=utf-8".into())], body.as_bytes().to_vec())
    }

    fn check(expr: &str, body: &str) -> AssertionResult {
        let (headers, body) = response(body);
        let data = ResponseData { status: 200, headers: &headers, body: &body, time_ms: 123.4 };
        Assertion::parse(expr).unwrap().evaluate(&data)
    }

    #[test]
    fn status_and_time() {
        assert!(check("status == 200", "").passed);
        assert!(!check("status != 200", "").passed);
        assert!(check("time < 500", "").passed);
        assert!(!check("time > 500", "").passed);
    }

    #[test]
    fn headers_are_case_insensitive() {
        assert!(check("header(content-type) contains json", "").passed);
        assert!(check("header(x-missing) !exists", "").passed);
    }

    #[test]
    fn json_paths() {
        let body = r#"{"user":{"id":7,"name":"Alice"},"tags":["a","b"]}"#;
        assert!(check("json($.user.id) == 7", body).passed);
        assert!(check(r#"json($.user.name) == "Alice""#, body).passed);
        assert!(check("json($.user.name) == Alice", body).passed, "bare words are text");
        assert!(check("json($.tags) contains b", body).passed);
        assert!(check("json($.nope) !exists", body).passed);
    }

    #[test]
    fn failures_explain_themselves() {
        let result = check("status == 404", "");
        assert!(!result.passed);
        assert_eq!(result.detail.as_deref(), Some("expected 404, got 200"));
    }

    #[test]
    fn regex_matching() {
        assert!(check(r"body matches ^\{.*\}$", "{}").passed);
        assert!(Assertion::parse("body matches (").is_err());
    }

    #[test]
    fn parse_errors_are_helpful() {
        let err = Assertion::parse("stauts == 200").unwrap_err().to_string();
        assert!(err.contains("expected a subject"), "{err}");
        let err = Assertion::parse("status").unwrap_err().to_string();
        assert!(err.contains("expected an operator"), "{err}");
        let err = Assertion::parse("json($..[) exists").unwrap_err().to_string();
        assert!(err.contains("invalid JSONPath"), "{err}");
    }

    #[test]
    fn extraction() {
        let (headers, body) = response(r#"{"token":"abc","n":1}"#);
        let data = ResponseData { status: 200, headers: &headers, body: &body, time_ms: 1.0 };
        assert_eq!(extract("json($.token)", &data).unwrap().as_deref(), Some("abc"));
        assert_eq!(extract("json($.n)", &data).unwrap().as_deref(), Some("1"));
        assert_eq!(extract("header(Content-Type)", &data).unwrap().as_deref(), Some("application/json; charset=utf-8"));
        assert_eq!(extract("json($.missing)", &data).unwrap(), None);
    }

    #[test]
    fn json_path_with_filter_parentheses() {
        let body = r#"{"items":[{"id":1,"ok":true},{"id":2,"ok":false}]}"#;
        assert!(check("json($.items[?(@.id == 2)].ok) == false", body).passed);
    }
}
