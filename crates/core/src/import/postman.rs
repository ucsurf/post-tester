//! Imports a Postman collection (format v2.0 / v2.1).
//!
//! Folders, requests, headers, bodies, auth, scripts and collection variables
//! are converted. Postman's `{{variable}}` syntax is the same as ours, and
//! scripts keep working through the `pm.*` compatibility API.

use super::Imported;
use crate::error::{Error, Result};
use crate::model::{ApiKeyLocation, Auth, Body, Environment, FolderSettings, KeyValue, Method, Request, Scripts};
use crate::workspace::slugify;
use serde_json::Value;

pub fn parse(text: &str) -> Result<Imported> {
    let root: Value = serde_json::from_str(text).map_err(|e| Error::Import(format!("not valid JSON: {e}")))?;
    let name = root.pointer("/info/name").and_then(Value::as_str).unwrap_or("Postman collection").to_string();
    let mut imported = Imported { name: name.clone(), ..Default::default() };

    let root_settings = FolderSettings {
        name: name.clone(),
        description: description(&root["info"]),
        auth: root.get("auth").and_then(auth),
        ..Default::default()
    };
    if root_settings != FolderSettings::default() {
        imported.folders.push((String::new(), root_settings));
    }

    let variables = root.get("variable").and_then(Value::as_array).cloned().unwrap_or_default();
    if !variables.is_empty() {
        let mut environment = Environment { description: format!("Variables imported from \"{name}\""), ..Default::default() };
        for variable in variables {
            if let Some(key) = variable.get("key").and_then(Value::as_str) {
                environment.variables.insert(key.into(), variable.get("value").cloned().unwrap_or(Value::Null));
            }
        }
        imported.environments.push((name, environment));
    }

    let items = root.get("item").and_then(Value::as_array).ok_or_else(|| Error::Import("missing `item` list".into()))?;
    walk(items, "", &mut imported);
    Ok(imported)
}

fn walk(items: &[Value], folder: &str, imported: &mut Imported) {
    for item in items {
        let name = item.get("name").and_then(Value::as_str).unwrap_or("Untitled").to_string();
        if let Some(children) = item.get("item").and_then(Value::as_array) {
            let id = if folder.is_empty() { slugify(&name) } else { format!("{folder}/{}", slugify(&name)) };
            imported.folders.push((
                id.clone(),
                FolderSettings { name, description: description(item), auth: item.get("auth").and_then(auth), ..Default::default() },
            ));
            walk(children, &id, imported);
        } else if let Some(request) = item.get("request") {
            imported.add_request(folder, convert(&name, request, item));
        }
    }
}

fn convert(name: &str, r: &Value, item: &Value) -> Request {
    // A request may be just a URL string.
    let r = if let Some(url) = r.as_str() { &serde_json::json!({ "url": url, "method": "GET" }) } else { r };
    let url = match &r["url"] {
        Value::String(s) => s.clone(),
        Value::Object(o) => o.get("raw").and_then(Value::as_str).unwrap_or_default().to_string(),
        _ => String::new(),
    };
    let headers = r["header"].as_array().map(|list| list.iter().filter_map(key_value).collect()).unwrap_or_default();
    Request {
        name: name.to_string(),
        description: description(r),
        method: Method::from(r["method"].as_str().unwrap_or("GET")),
        url,
        headers,
        body: body(&r["body"]),
        auth: r.get("auth").and_then(auth),
        scripts: scripts(item),
        ..Default::default()
    }
}

fn description(v: &Value) -> String {
    match &v["description"] {
        Value::String(s) => s.clone(),
        Value::Object(o) => o.get("content").and_then(Value::as_str).unwrap_or_default().to_string(),
        _ => String::new(),
    }
}

fn key_value(v: &Value) -> Option<KeyValue> {
    Some(KeyValue {
        key: v.get("key")?.as_str()?.to_string(),
        value: v.get("value").and_then(Value::as_str).unwrap_or_default().to_string(),
        enabled: !v.get("disabled").and_then(Value::as_bool).unwrap_or(false),
    })
}

fn body(b: &Value) -> Body {
    match b["mode"].as_str() {
        Some("raw") => {
            let content = b["raw"].as_str().unwrap_or_default().to_string();
            let language = b.pointer("/options/raw/language").and_then(Value::as_str).unwrap_or("text");
            match language {
                "json" => Body::Json { content },
                "xml" => Body::Text { content, content_type: "application/xml".into() },
                "html" => Body::Text { content, content_type: "text/html".into() },
                "javascript" => Body::Text { content, content_type: "application/javascript".into() },
                _ => Body::Text { content, content_type: "text/plain".into() },
            }
        }
        Some("urlencoded") | Some("formdata") => {
            let key = if b["mode"] == "urlencoded" { "urlencoded" } else { "formdata" };
            let fields = b[key].as_array().map(|list| list.iter().filter_map(key_value).collect()).unwrap_or_default();
            Body::Form { fields }
        }
        Some("graphql") => Body::Graphql {
            query: b.pointer("/graphql/query").and_then(Value::as_str).unwrap_or_default().to_string(),
            variables: b.pointer("/graphql/variables").and_then(Value::as_str).unwrap_or_default().to_string(),
            operation_name: None,
        },
        _ => Body::None,
    }
}

/// Reads Postman's auth format: `{ type: "bearer", bearer: [{key, value}, ...] }`.
fn auth(a: &Value) -> Option<Auth> {
    let kind = a["type"].as_str()?;
    let field = |name: &str| -> String {
        a[kind]
            .as_array()
            .and_then(|list| list.iter().find(|kv| kv["key"] == name))
            .and_then(|kv| kv["value"].as_str())
            .unwrap_or_default()
            .to_string()
    };
    Some(match kind {
        "noauth" => Auth::None,
        "bearer" => Auth::Bearer { token: field("token") },
        "basic" => Auth::Basic { username: field("username"), password: field("password") },
        "apikey" => Auth::ApiKey {
            key: field("key"),
            value: field("value"),
            location: if field("in") == "query" { ApiKeyLocation::Query } else { ApiKeyLocation::Header },
        },
        _ => return None,
    })
}

fn scripts(item: &Value) -> Scripts {
    let mut scripts = Scripts::default();
    for event in item["event"].as_array().into_iter().flatten() {
        let code = match &event.pointer("/script/exec") {
            Some(Value::Array(lines)) => lines.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n"),
            Some(Value::String(s)) => s.clone(),
            _ => continue,
        };
        match event["listen"].as_str() {
            Some("prerequest") => scripts.before_request = code,
            Some("test") => scripts.after_response = code,
            _ => {}
        }
    }
    scripts
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLLECTION: &str = r#"{
      "info": { "name": "Shop API", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json" },
      "variable": [{ "key": "baseUrl", "value": "https://shop.dev" }],
      "auth": { "type": "bearer", "bearer": [{ "key": "token", "value": "{{token}}" }] },
      "item": [
        { "name": "Users", "item": [
          { "name": "Create user",
            "event": [{ "listen": "test", "script": { "exec": ["pm.test('ok', () => {", "  pm.response.to.have.status(201);", "});"] } }],
            "request": { "method": "POST", "url": { "raw": "{{baseUrl}}/users" },
              "header": [{ "key": "X-Off", "value": "1", "disabled": true }],
              "body": { "mode": "raw", "raw": "{\"name\":\"a\"}", "options": { "raw": { "language": "json" } } } } }
        ]},
        { "name": "Health", "request": "https://shop.dev/health" }
      ]
    }"#;

    #[test]
    fn imports_folders_requests_auth_scripts_and_variables() {
        let imported = parse(COLLECTION).unwrap();
        let ids: Vec<_> = imported.requests.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, vec!["users/create-user", "health"]);
        let (_, create) = &imported.requests[0];
        assert_eq!(create.method.as_str(), "POST");
        assert!(matches!(create.body, Body::Json { .. }));
        assert!(!create.headers[0].enabled);
        assert!(create.scripts.after_response.contains("pm.response.to.have.status(201)"));
        assert!(matches!(imported.folders[0].1.auth, Some(Auth::Bearer { .. })));
        assert_eq!(imported.environments[0].1.variables["baseUrl"], "https://shop.dev");
    }
}
