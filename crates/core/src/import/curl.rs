//! Parses a `curl` command line into a request.

use crate::error::{Error, Result};
use crate::model::{Auth, Body, KeyValue, Method, Request};

/// Parses a cURL command such as one copied from browser dev tools.
pub fn parse(command: &str) -> Result<Request> {
    // Join lines continued with a backslash (shell or Windows `^`).
    let joined = command.replace("\\\r\n", " ").replace("\\\n", " ").replace("^\n", " ");
    let args = shell_words::split(&joined).map_err(|e| Error::Import(format!("could not read the cURL command: {e}")))?;
    let mut args = args.into_iter().peekable();
    if args.next().as_deref() != Some("curl") {
        return Err(Error::Import("the command must start with `curl`".into()));
    }

    let mut request = Request::default();
    let mut method: Option<String> = None;
    let mut data: Vec<String> = Vec::new();
    let mut force_get = false;
    let mut is_json = false;

    while let Some(arg) = args.next() {
        // Support both `-XPOST` and `-X POST`, and `--opt=value`.
        let (flag, inline) = split_flag(&arg);
        let mut value = || inline.clone().or_else(|| args.next()).ok_or_else(|| Error::Import(format!("`{flag}` needs a value")));
        match flag.as_str() {
            "-X" | "--request" => method = Some(value()?),
            "-H" | "--header" => {
                let header = value()?;
                if let Some((name, val)) = header.split_once(':') {
                    request.headers.push(KeyValue::new(name.trim(), val.trim()));
                }
            }
            "-d" | "--data" | "--data-raw" | "--data-binary" | "--data-ascii" | "--data-urlencode" => data.push(value()?),
            "--json" => {
                data.push(value()?);
                is_json = true;
            }
            "-u" | "--user" => {
                let user = value()?;
                let (username, password) = user.split_once(':').unwrap_or((&user, ""));
                request.auth = Some(Auth::Basic { username: username.into(), password: password.into() });
            }
            "-A" | "--user-agent" => request.headers.push(KeyValue::new("User-Agent", value()?)),
            "-b" | "--cookie" => request.headers.push(KeyValue::new("Cookie", value()?)),
            "-e" | "--referer" => request.headers.push(KeyValue::new("Referer", value()?)),
            "--url" => request.url = value()?,
            "-G" | "--get" => force_get = true,
            "-k" | "--insecure" => request.options.verify_tls = false,
            "-L" | "--location" => request.options.follow_redirects = true,
            "-I" | "--head" => method = Some("HEAD".into()),
            "-F" | "--form" => {
                return Err(Error::Import("multipart form data (`-F`) is not supported yet".into()));
            }
            // Flags that take a value but don't change the request.
            "-o" | "--output" | "-m" | "--max-time" | "--connect-timeout" | "-w" | "--write-out" | "--retry" => {
                value()?;
            }
            f if f.starts_with('-') => {} // ignore other flags (--compressed, -s, -v…)
            _ => {
                if request.url.is_empty() {
                    request.url = arg;
                }
            }
        }
    }

    if request.url.is_empty() {
        return Err(Error::Import("no URL found in the cURL command".into()));
    }
    if force_get && !data.is_empty() {
        let separator = if request.url.contains('?') { '&' } else { '?' };
        request.url = format!("{}{separator}{}", request.url, data.join("&"));
        data.clear();
    }
    if !data.is_empty() {
        let content = data.join("&");
        let content_type = request.headers.iter().find(|h| h.key.eq_ignore_ascii_case("content-type")).map(|h| h.value.clone());
        let looks_like_json = is_json || content_type.as_deref().is_some_and(|ct| ct.contains("json"));
        request.body = if looks_like_json {
            request.headers.retain(|h| !h.key.eq_ignore_ascii_case("content-type"));
            Body::Json { content }
        } else {
            Body::Text { content, content_type: content_type.unwrap_or_else(|| "application/x-www-form-urlencoded".into()) }
        };
        if let Body::Text { .. } = request.body {
            request.headers.retain(|h| !h.key.eq_ignore_ascii_case("content-type"));
        }
    }
    let default_method = if data.is_empty() || force_get { "GET" } else { "POST" };
    request.method = Method::from(method.as_deref().unwrap_or(default_method));
    request.name = format!("{} {}", request.method.as_str(), path_of(&request.url));
    Ok(request)
}

fn split_flag(arg: &str) -> (String, Option<String>) {
    if let Some(rest) = arg.strip_prefix("--") {
        if let Some((flag, value)) = rest.split_once('=') {
            return (format!("--{flag}"), Some(value.to_string()));
        }
        return (arg.to_string(), None);
    }
    // Short flags with attached values: -XPOST, -H'…'
    if arg.len() > 2 && arg.starts_with('-') && matches!(&arg[..2], "-X" | "-H" | "-d" | "-u" | "-A" | "-b" | "-e") {
        return (arg[..2].to_string(), Some(arg[2..].to_string()));
    }
    (arg.to_string(), None)
}

fn path_of(url: &str) -> String {
    url::Url::parse(url).map(|u| u.path().to_string()).unwrap_or_else(|_| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_browser_style_command() {
        let request = parse(
            r#"curl 'https://api.example.com/users' \
  -H 'Content-Type: application/json' \
  -H 'Authorization: Bearer abc' \
  --data-raw '{"name":"a"}' \
  --compressed"#,
        )
        .unwrap();
        assert_eq!(request.method.as_str(), "POST");
        assert_eq!(request.url, "https://api.example.com/users");
        assert!(matches!(&request.body, Body::Json { content } if content == r#"{"name":"a"}"#));
        assert_eq!(request.headers.len(), 1, "content-type is implied by the JSON body");
        assert_eq!(request.name, "POST /users");
    }

    #[test]
    fn parses_method_auth_and_get_data() {
        let request = parse("curl -XDELETE -u user:pw https://x.dev/a").unwrap();
        assert_eq!(request.method.as_str(), "DELETE");
        assert!(matches!(request.auth, Some(Auth::Basic { ref username, .. }) if username == "user"));

        let request = parse("curl -G https://x.dev/search -d q=rust").unwrap();
        assert_eq!(request.url, "https://x.dev/search?q=rust");
        assert_eq!(request.method.as_str(), "GET");
    }

    #[test]
    fn explains_problems() {
        assert!(parse("wget x").unwrap_err().to_string().contains("must start with `curl`"));
        assert!(parse("curl -H").unwrap_err().to_string().contains("needs a value"));
    }
}
