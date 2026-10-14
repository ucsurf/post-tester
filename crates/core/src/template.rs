//! `{{ }}` templates.
//!
//! | Syntax                       | Meaning                                        |
//! |------------------------------|------------------------------------------------|
//! | `{{baseUrl}}`                | A variable (runtime → environment → workspace) |
//! | `{{secret:apiKey}}`          | A secret from the keychain or `PT_SECRET_*`    |
//! | `{{$uuid}}`                  | A built-in dynamic value                       |
//! | `{{totp(secret:"x")}}`       | A function provided by a plugin                |
//!
//! Variables may contain templates themselves (`baseUrl: https://{{host}}`).
//! Unknown variables are left as they are and reported, so a typo is visible
//! in Live Activity instead of silently becoming an empty string.

use crate::error::{Error, Result};
use crate::model::OrderedMap;
use async_trait::async_trait;
use std::collections::HashMap;

/// How deep variables may reference other variables before we assume a loop.
const MAX_DEPTH: usize = 8;

/// Built-in dynamic values, available as `{{$name}}`.
pub const BUILTINS: &[(&str, &str)] = &[
    ("$uuid", "A random UUID v4"),
    ("$timestamp", "Current Unix time in seconds"),
    ("$timestampMs", "Current Unix time in milliseconds"),
    ("$isoTimestamp", "Current time in ISO 8601 format"),
    ("$randomInt", "A random integer from 0 to 1000"),
];

/// Where a template gets its values from.
#[async_trait]
pub trait TemplateContext: Send + Sync {
    /// Looks up a plain variable.
    fn variable(&self, name: &str) -> Option<String>;

    /// Reads a secret.
    async fn secret(&self, name: &str) -> Result<String>;

    /// Calls a template function. Returns `Ok(None)` if no function has this name.
    async fn function(&self, name: &str, args: &OrderedMap) -> Result<Option<String>>;
}

/// Variables in layers. Earlier layers win.
#[derive(Debug, Clone, Default)]
pub struct Variables {
    layers: Vec<HashMap<String, String>>,
}

impl Variables {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a layer with lower priority than all existing layers.
    pub fn push_layer<'a>(&mut self, vars: impl IntoIterator<Item = (&'a String, &'a serde_json::Value)>) {
        self.layers.push(vars.into_iter().map(|(k, v)| (k.clone(), crate::model::value_to_string(v))).collect());
    }

    /// Adds a layer from plain strings, with lower priority than existing layers.
    pub fn push_strings(&mut self, vars: HashMap<String, String>) {
        self.layers.push(vars);
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.layers.iter().find_map(|layer| layer.get(name)).map(String::as_str)
    }

    /// All variables, resolved by priority, sorted by name.
    pub fn flatten(&self) -> Vec<(String, String)> {
        let mut out: HashMap<&str, &str> = HashMap::new();
        for layer in self.layers.iter().rev() {
            for (k, v) in layer {
                out.insert(k, v);
            }
        }
        let mut list: Vec<_> = out.into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        list.sort();
        list
    }
}

/// The result of rendering a template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub text: String,
    /// Names of variables that were not found (left as `{{name}}` in `text`).
    pub unresolved: Vec<String>,
}

/// Renders every `{{ }}` in `template`.
pub async fn render(template: &str, ctx: &dyn TemplateContext) -> Result<Rendered> {
    // Most bodies and headers contain no template at all: skip the parser.
    if !template.contains("{{") {
        return Ok(Rendered { text: template.to_string(), unresolved: Vec::new() });
    }
    let mut unresolved = Vec::new();
    let text = render_inner(template, ctx, 0, &mut unresolved).await?;
    unresolved.sort();
    unresolved.dedup();
    Ok(Rendered { text, unresolved })
}

fn render_inner<'a>(
    template: &'a str,
    ctx: &'a dyn TemplateContext,
    depth: usize,
    unresolved: &'a mut Vec<String>,
) -> futures::future::BoxFuture<'a, Result<String>> {
    Box::pin(async move {
        if depth > MAX_DEPTH {
            return Err(Error::Template(format!(
                "variables reference each other more than {MAX_DEPTH} levels deep; check for a loop near `{}`",
                excerpt(template)
            )));
        }
        if !template.contains("{{") {
            return Ok(template.to_string());
        }
        let mut out = String::with_capacity(template.len());
        let mut rest = template;
        while let Some(start) = rest.find("{{") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            let Some(end) = find_close(after) else {
                // No closing braces: keep the rest as literal text.
                out.push_str(&rest[start..]);
                return Ok(out);
            };
            let raw = &after[..end];
            let expr = raw.trim();
            match resolve(expr, ctx).await? {
                Some(value) => {
                    let nested = render_inner(&value, ctx, depth + 1, unresolved).await?;
                    out.push_str(&nested);
                }
                None => {
                    unresolved.push(expr.to_string());
                    out.push_str("{{");
                    out.push_str(raw);
                    out.push_str("}}");
                }
            }
            rest = &after[end + 2..];
        }
        out.push_str(rest);
        Ok(out)
    })
}

/// The start of `text`, short enough for an error message.
fn excerpt(text: &str) -> String {
    const MAX_CHARS: usize = 80;
    match text.char_indices().nth(MAX_CHARS) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_string(),
    }
}

/// Finds the `}}` that closes a template, skipping `}}` inside quoted strings.
fn find_close(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut in_string = false;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if in_string => i += 1,
            b'"' => in_string = !in_string,
            b'}' if !in_string && bytes.get(i + 1) == Some(&b'}') => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

async fn resolve(expr: &str, ctx: &dyn TemplateContext) -> Result<Option<String>> {
    if let Some(name) = expr.strip_prefix("secret:") {
        return ctx.secret(name.trim()).await.map(Some);
    }
    if expr.starts_with('$') {
        return Ok(builtin(expr));
    }
    if let Some((name, mut args)) = parse_call(expr)? {
        // Text arguments may use templates themselves: totp(secret: "{{secret:otp}}").
        for value in args.values_mut() {
            if let serde_json::Value::String(text) = value
                && text.contains("{{")
            {
                *text = render(text, ctx).await?.text;
            }
        }
        return match ctx.function(&name, &args).await? {
            Some(value) => Ok(Some(value)),
            None => {
                Err(Error::Template(format!("unknown function `{name}` in `{{{{{expr}}}}}`. Is the plugin that provides it installed?")))
            }
        };
    }
    Ok(ctx.variable(expr))
}

fn builtin(name: &str) -> Option<String> {
    let now = chrono::Utc::now();
    Some(match name {
        "$uuid" => uuid::Uuid::new_v4().to_string(),
        "$timestamp" => now.timestamp().to_string(),
        "$timestampMs" => now.timestamp_millis().to_string(),
        "$isoTimestamp" => now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "$randomInt" => rand::random_range(0..=1000).to_string(),
        _ => return None,
    })
}

/// Parses `name(arg: "value", n: 1)` or `name("positional")`.
///
/// Positional arguments are stored under their index (`"0"`, `"1"`…); the
/// plugin host maps them to the function's declared argument names.
pub fn parse_call(expr: &str) -> Result<Option<(String, OrderedMap)>> {
    let Some(open) = expr.find('(') else { return Ok(None) };
    if !expr.ends_with(')') {
        return Ok(None);
    }
    let name = expr[..open].trim();
    let valid_name = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | '/'));
    if !valid_name {
        return Ok(None);
    }
    let inner = &expr[open + 1..expr.len() - 1];
    let mut args = OrderedMap::new();
    for (index, part) in split_args(inner).into_iter().enumerate() {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (key, value) = match split_named(part) {
            Some((k, v)) => (k.to_string(), v),
            None => (index.to_string(), part),
        };
        let value = serde_json::from_str(value.trim()).map_err(|_| {
            Error::Template(format!("argument `{part}` in `{expr}` is not a valid value. Use \"quotes\" for text, e.g. name: \"value\""))
        })?;
        args.insert(key, value);
    }
    Ok(Some((name.to_string(), args)))
}

/// Splits on commas that are not inside quotes or brackets.
fn split_args(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut in_string, mut escaped, mut start) = (0i32, false, false, 0);
    for (i, c) in s.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            '[' | '{' if !in_string => depth += 1,
            ']' | '}' if !in_string => depth -= 1,
            ',' if !in_string && depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// Splits `key: value` when `key` is a plain identifier.
fn split_named(part: &str) -> Option<(&str, &str)> {
    let colon = part.find(':')?;
    let key = part[..colon].trim();
    let is_identifier = !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    is_identifier.then(|| (key, &part[colon + 1..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Ctx(Variables);

    #[async_trait]
    impl TemplateContext for Ctx {
        fn variable(&self, name: &str) -> Option<String> {
            self.0.get(name).map(str::to_string)
        }
        async fn secret(&self, name: &str) -> Result<String> {
            if name == "token" { Ok("s3cret".into()) } else { Err(Error::missing_secret(name)) }
        }
        async fn function(&self, name: &str, args: &OrderedMap) -> Result<Option<String>> {
            Ok((name == "upper").then(|| crate::model::value_to_string(&args["0"]).to_uppercase()))
        }
    }

    fn ctx(pairs: &[(&str, &str)]) -> Ctx {
        let mut vars = Variables::new();
        vars.push_strings(pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect());
        Ctx(vars)
    }

    #[tokio::test]
    async fn renders_variables_secrets_and_functions() {
        let c = ctx(&[("host", "example.com"), ("base", "https://{{host}}")]);
        let r = render("{{base}}/x?t={{ secret:token }}&n={{upper(\"ab}}c\")}}", &c).await.unwrap();
        assert_eq!(r.text, "https://example.com/x?t=s3cret&n=AB}}C");
        assert!(r.unresolved.is_empty());
    }

    #[tokio::test]
    async fn function_arguments_can_use_variables_and_secrets() {
        let c = ctx(&[("name", "ada")]);
        let r = render(r#"{{upper("{{name}}-{{secret:token}}")}}"#, &c).await.unwrap();
        assert_eq!(r.text, "ADA-S3CRET");
    }

    #[tokio::test]
    async fn unknown_variables_are_kept_and_reported() {
        let r = render("a {{missing}} b", &ctx(&[])).await.unwrap();
        assert_eq!(r.text, "a {{missing}} b");
        assert_eq!(r.unresolved, vec!["missing"]);
    }

    #[tokio::test]
    async fn loops_are_detected() {
        let err = render("{{a}}", &ctx(&[("a", "{{b}}"), ("b", "{{a}}")])).await.unwrap_err();
        assert!(err.to_string().contains("loop"));
    }

    #[tokio::test]
    async fn json_bodies_with_braces_render() {
        let c = ctx(&[("id", "7"), ("name", "Ada")]);
        let body = r#"{"user": {"id": {{id}}, "name": "{{name}}", "tags": {"a": {"b": 1}}}, "raw": "{{"}"#;
        let r = render(body, &c).await.unwrap();
        assert_eq!(r.text, r#"{"user": {"id": 7, "name": "Ada", "tags": {"a": {"b": 1}}}, "raw": "{{"}"#);
        // `{{` without a closing `}}` stays as it is.
        assert_eq!(render("a {{ b", &c).await.unwrap().text, "a {{ b");
        // An escaped quote does not end a string argument early.
        let r = render(r#"{{upper("say \"}}\" ok")}}"#, &c).await.unwrap();
        assert_eq!(r.text, r#"SAY "}}" OK"#);
    }

    #[tokio::test]
    async fn long_text_without_templates_is_returned_as_is() {
        let text = "x".repeat(1_000_000);
        assert_eq!(render(&text, &ctx(&[])).await.unwrap().text, text);
        let many = "{{id}},".repeat(10_000);
        assert_eq!(render(&many, &ctx(&[("id", "1")])).await.unwrap().text, "1,".repeat(10_000));
    }

    #[tokio::test]
    async fn loop_errors_quote_only_an_excerpt() {
        let long = format!("{{{{a}}}}{}", "y".repeat(10_000));
        let err = render("{{a}}", &ctx(&[("a", &long)])).await.unwrap_err().to_string();
        assert!(err.len() < 400, "{err}");
    }

    #[tokio::test]
    async fn unknown_function_explains_itself() {
        let err = render("{{nope(1)}}", &ctx(&[])).await.unwrap_err();
        assert!(err.to_string().contains("unknown function `nope`"));
    }

    #[test]
    fn parses_named_and_positional_arguments() {
        let (name, args) = parse_call(r#"totp(secret: "a,b", digits: 6, "x")"#).unwrap().unwrap();
        assert_eq!(name, "totp");
        assert_eq!(args["secret"], "a,b");
        assert_eq!(args["digits"], 6);
        assert_eq!(args["2"], "x");
    }

    #[test]
    fn earlier_layers_win() {
        let mut vars = Variables::new();
        vars.push_strings([("a".to_string(), "runtime".to_string())].into());
        vars.push_strings([("a".to_string(), "env".to_string()), ("b".to_string(), "env".to_string())].into());
        assert_eq!(vars.get("a"), Some("runtime"));
        assert_eq!(vars.get("b"), Some("env"));
    }
}
