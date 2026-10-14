//! Turns a saved [`Request`] into a [`PreparedRequest`]: templates rendered,
//! folder settings inherited, body encoded and built-in auth applied.

use crate::error::{Error, Result};
use crate::http::PreparedRequest;
use crate::model::{ApiKeyLocation, Auth, Body, FolderSettings, KeyValue, OrderedMap, Request};
use crate::template::{TemplateContext, render};
use base64::Engine as _;

/// The result of preparing a request.
#[derive(Debug, Clone)]
pub struct Prepared {
    pub request: PreparedRequest,
    /// Plugin auth still to apply: (auth method name, rendered config).
    pub plugin_auth: Option<(String, OrderedMap)>,
    /// Variables that were not found anywhere.
    pub unresolved: Vec<String>,
}

/// Prepares `request`. `folders` are the folder settings from root to leaf.
pub async fn prepare(request: &Request, folders: &[FolderSettings], ctx: &dyn TemplateContext) -> Result<Prepared> {
    let mut r = Renderer { ctx, unresolved: Vec::new() };

    // URL + query parameters
    let mut url_text = r.render(&request.url).await?;
    if url_text.trim().is_empty() {
        return Err(Error::InvalidRequest("the URL is empty".into()));
    }
    if !url_text.contains("://") {
        url_text = format!("http://{url_text}");
    }
    let mut url = url::Url::parse(url_text.trim()).map_err(|e| Error::InvalidUrl { url: url_text.clone(), reason: e.to_string() })?;
    let host = url.host_str().unwrap_or_default();
    if host.contains("{{") || host.contains("%7B%7B") {
        return Err(Error::InvalidUrl {
            url: url_text,
            reason: format!("the host contains unknown variable(s): {}. Select an environment that defines them", r.unresolved.join(", ")),
        });
    }
    let query = r.render_pairs(&request.query).await?;
    if !query.is_empty() {
        let mut pairs = url.query_pairs_mut();
        for (key, value) in &query {
            pairs.append_pair(key, value);
        }
    }

    // Headers: folders first (root → leaf), then the request; later ones replace earlier ones.
    let mut headers: Vec<(String, String)> = Vec::new();
    for list in folders.iter().map(|f| &f.headers).chain(std::iter::once(&request.headers)) {
        for (key, value) in r.render_pairs(list).await? {
            headers.retain(|(k, _)| !k.eq_ignore_ascii_case(&key));
            headers.push((key, value));
        }
    }

    let mut prepared = PreparedRequest {
        method: request.method.as_str().to_ascii_uppercase(),
        url,
        headers,
        body: bytes::Bytes::new(),
        options: request.options.clone(),
    };

    // Body
    let (body, content_type) = match &request.body {
        Body::None => (String::new(), None),
        Body::Text { content, content_type } => (r.render(content).await?, Some(content_type.clone())),
        Body::Json { content } => (r.render(content).await?, Some("application/json".to_string())),
        Body::Form { fields } => {
            let pairs = r.render_pairs(fields).await?;
            let encoded = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(pairs).finish();
            (encoded, Some("application/x-www-form-urlencoded".to_string()))
        }
        Body::Graphql { query, variables, operation_name } => {
            let variables = r.render(variables).await?;
            let variables: serde_json::Value = if variables.trim().is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::from_str(&variables).map_err(|e| Error::InvalidRequest(format!("GraphQL variables are not valid JSON: {e}")))?
            };
            let mut payload = serde_json::json!({ "query": r.render(query).await?, "variables": variables });
            if let Some(name) = operation_name {
                payload["operationName"] = name.clone().into();
            }
            (payload.to_string(), Some("application/json".to_string()))
        }
    };
    if let Some(content_type) = content_type
        && prepared.header("content-type").is_none()
    {
        prepared.headers.push(("Content-Type".into(), content_type));
    }
    prepared.body = body.into_bytes().into();

    // Auth: the request's own, or the nearest folder's.
    let auth = request.auth.as_ref().or_else(|| folders.iter().rev().find_map(|f| f.auth.as_ref()));
    let mut plugin_auth = None;
    match auth {
        None | Some(Auth::None) => {}
        Some(Auth::Basic { username, password }) => {
            let credentials = format!("{}:{}", r.render(username).await?, r.render(password).await?);
            let encoded = base64::engine::general_purpose::STANDARD.encode(credentials);
            prepared.set_header("Authorization", format!("Basic {encoded}"));
        }
        Some(Auth::Bearer { token }) => {
            prepared.set_header("Authorization", format!("Bearer {}", r.render(token).await?));
        }
        Some(Auth::ApiKey { key, value, location }) => {
            let (key, value) = (r.render(key).await?, r.render(value).await?);
            match location {
                ApiKeyLocation::Header => prepared.set_header(&key, value),
                ApiKeyLocation::Query => {
                    prepared.url.query_pairs_mut().append_pair(&key, &value);
                }
            }
        }
        Some(Auth::Plugin { name, config }) => {
            let mut rendered = OrderedMap::new();
            for (key, value) in config {
                let value = match value {
                    serde_json::Value::String(s) => serde_json::Value::String(r.render(s).await?),
                    other => other.clone(),
                };
                rendered.insert(key.clone(), value);
            }
            plugin_auth = Some((name.clone(), rendered));
        }
    }

    let mut unresolved = r.unresolved;
    unresolved.sort();
    unresolved.dedup();
    Ok(Prepared { request: prepared, plugin_auth, unresolved })
}

struct Renderer<'a> {
    ctx: &'a dyn TemplateContext,
    unresolved: Vec<String>,
}

impl Renderer<'_> {
    async fn render(&mut self, text: &str) -> Result<String> {
        let rendered = render(text, self.ctx).await?;
        self.unresolved.extend(rendered.unresolved);
        Ok(rendered.text)
    }

    /// Renders the enabled pairs, skipping ones with an empty key.
    async fn render_pairs(&mut self, pairs: &[KeyValue]) -> Result<Vec<(String, String)>> {
        let mut out = Vec::new();
        for pair in pairs.iter().filter(|p| p.enabled && !p.key.trim().is_empty()) {
            out.push((self.render(&pair.key).await?, self.render(&pair.value).await?));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::Variables;
    use async_trait::async_trait;

    struct Vars(Variables);

    #[async_trait]
    impl TemplateContext for Vars {
        fn variable(&self, name: &str) -> Option<String> {
            self.0.get(name).map(str::to_string)
        }
        async fn secret(&self, _: &str) -> Result<String> {
            Ok("s".into())
        }
        async fn function(&self, _: &str, _: &OrderedMap) -> Result<Option<String>> {
            Ok(None)
        }
    }

    fn vars() -> Vars {
        let mut v = Variables::new();
        v.push_strings([("base".to_string(), "api.example.com".to_string()), ("id".to_string(), "7".to_string())].into());
        Vars(v)
    }

    #[tokio::test]
    async fn builds_url_query_headers_and_json_body() {
        let request: Request = serde_yaml::from_str(
            r#"
method: post
url: "{{base}}/users/{{id}}"
query: [{ key: q, value: "a b" }, { key: off, value: x, enabled: false }]
headers: [{ key: X-Id, value: "{{id}}" }]
body: { type: json, content: '{"id": {{id}}}' }
"#,
        )
        .unwrap();
        let folders = vec![FolderSettings {
            headers: vec![KeyValue::new("x-id", "folder"), KeyValue::new("X-Team", "core")],
            auth: Some(Auth::Bearer { token: "{{secret:t}}".into() }),
            ..Default::default()
        }];
        let p = prepare(&request, &folders, &vars()).await.unwrap();
        assert_eq!(p.request.method, "POST");
        assert_eq!(p.request.url.as_str(), "http://api.example.com/users/7?q=a+b");
        assert_eq!(p.request.header("x-id"), Some("7"), "request headers override folder headers");
        assert_eq!(p.request.header("x-team"), Some("core"));
        assert_eq!(p.request.header("authorization"), Some("Bearer s"), "auth inherited from folder");
        assert_eq!(p.request.header("content-type"), Some("application/json"));
        assert_eq!(&p.request.body[..], br#"{"id": 7}"#);
    }

    #[tokio::test]
    async fn graphql_and_basic_auth() {
        let request = Request {
            url: "https://x.dev/graphql".into(),
            body: Body::Graphql { query: "{ me { id } }".into(), variables: r#"{"a":1}"#.into(), operation_name: None },
            auth: Some(Auth::Basic { username: "u".into(), password: "p".into() }),
            ..Default::default()
        };
        let p = prepare(&request, &[], &vars()).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&p.request.body).unwrap();
        assert_eq!(body["variables"]["a"], 1);
        assert_eq!(p.request.header("authorization"), Some("Basic dTpw"));
    }

    #[tokio::test]
    async fn reports_unresolved_variables() {
        let request = Request { url: "{{nope}}/x".into(), ..Default::default() };
        let err = prepare(&request, &[], &vars()).await.unwrap_err().to_string();
        assert!(err.contains("unknown variable(s): nope"), "{err}");
        let request = Request { url: "http://h/{{nope}}".into(), ..Default::default() };
        let p = prepare(&request, &[], &vars()).await.unwrap();
        assert_eq!(p.unresolved, vec!["nope"]);
    }
}
