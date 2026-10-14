//! Generates code that sends a request: "Copy as cURL / JavaScript / Python".
//!
//! Plugins can add more languages; these are the built-in ones.

use crate::http::PreparedRequest;

/// A built-in code generator.
#[derive(Debug, Clone, Copy)]
pub struct Generator {
    pub id: &'static str,
    pub name: &'static str,
    pub language: &'static str,
    generate: fn(&PreparedRequest) -> String,
}

impl Generator {
    pub fn generate(&self, request: &PreparedRequest) -> String {
        (self.generate)(request)
    }
}

pub const GENERATORS: &[Generator] = &[
    Generator { id: "curl", name: "cURL", language: "shell", generate: curl },
    Generator { id: "javascript-fetch", name: "JavaScript (fetch)", language: "javascript", generate: javascript_fetch },
    Generator { id: "python-requests", name: "Python (requests)", language: "python", generate: python_requests },
];

pub fn find(id: &str) -> Option<&'static Generator> {
    GENERATORS.iter().find(|g| g.id == id)
}

/// Headers worth showing in generated code (skips ones every client adds itself).
fn visible_headers(request: &PreparedRequest) -> impl Iterator<Item = &(String, String)> {
    request
        .headers
        .iter()
        .filter(|(k, _)| !["host", "content-length", "user-agent", "accept-encoding"].iter().any(|h| k.eq_ignore_ascii_case(h)))
}

fn body_text(request: &PreparedRequest) -> Option<String> {
    (!request.body.is_empty()).then(|| String::from_utf8_lossy(&request.body).into_owned())
}

/// Quotes for a POSIX shell: wraps in single quotes, escaping embedded ones.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn curl(request: &PreparedRequest) -> String {
    let mut parts = vec!["curl".to_string()];
    if request.method != "GET" || !request.body.is_empty() {
        parts.push(format!("-X {}", request.method));
    }
    parts.push(shell_quote(request.url.as_str()));
    for (name, value) in visible_headers(request) {
        parts.push(format!("-H {}", shell_quote(&format!("{name}: {value}"))));
    }
    if let Some(body) = body_text(request) {
        parts.push(format!("--data-raw {}", shell_quote(&body)));
    }
    if !request.options.verify_tls {
        parts.push("--insecure".into());
    }
    if request.options.follow_redirects {
        parts.push("--location".into());
    }
    parts.join(" \\\n  ")
}

fn javascript_fetch(request: &PreparedRequest) -> String {
    let json = |s: &str| serde_json::to_string(s).expect("strings always serialize");
    let mut options = vec![format!("  method: {}", json(&request.method))];
    let headers: Vec<String> = visible_headers(request).map(|(k, v)| format!("    {}: {}", json(k), json(v))).collect();
    if !headers.is_empty() {
        options.push(format!("  headers: {{\n{}\n  }}", headers.join(",\n")));
    }
    if let Some(body) = body_text(request) {
        options.push(format!("  body: {}", json(&body)));
    }
    format!(
        "const response = await fetch({}, {{\n{}\n}});\n\nconsole.log(response.status, await response.text());",
        json(request.url.as_str()),
        options.join(",\n")
    )
}

fn python_requests(request: &PreparedRequest) -> String {
    // JSON string escaping is also valid Python string syntax.
    let py = |s: &str| serde_json::to_string(s).expect("strings always serialize");
    let mut lines = vec!["import requests".to_string(), String::new()];
    let headers: Vec<String> = visible_headers(request).map(|(k, v)| format!("    {}: {},", py(k), py(v))).collect();
    let mut args = vec![py(&request.method), py(request.url.as_str())];
    if !headers.is_empty() {
        lines.push(format!("headers = {{\n{}\n}}", headers.join("\n")));
        args.push("headers=headers".into());
    }
    if let Some(body) = body_text(request) {
        lines.push(format!("data = {}", py(&body)));
        args.push("data=data".into());
    }
    if !request.options.verify_tls {
        args.push("verify=False".into());
    }
    if !headers.is_empty() || body_text(request).is_some() {
        lines.push(String::new());
    }
    lines.push(format!("response = requests.request({})", args.join(", ")));
    lines.push("print(response.status_code, response.text)".into());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> PreparedRequest {
        PreparedRequest {
            method: "POST".into(),
            url: "https://api.example.com/users?x=1".parse().unwrap(),
            headers: vec![("Content-Type".into(), "application/json".into()), ("Host".into(), "skip".into())],
            body: r#"{"name":"O'Brien"}"#.into(),
            options: Default::default(),
        }
    }

    #[test]
    fn curl_quotes_safely() {
        let code = curl(&request());
        assert!(code.contains(r#"--data-raw '{"name":"O'\''Brien"}'"#), "{code}");
        assert!(!code.contains("Host"));
    }

    #[test]
    fn every_generator_includes_the_url() {
        for generator in GENERATORS {
            assert!(generator.generate(&request()).contains("https://api.example.com/users?x=1"), "{}", generator.id);
        }
    }
}
