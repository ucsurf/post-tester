//! Imports an OpenAPI 3 document (JSON or YAML).
//!
//! Each operation becomes a request, grouped into folders by tag. Path
//! parameters `{id}` become variables `{{id}}`, and the first server URL
//! becomes the `baseUrl` variable of a new environment.

use super::Imported;
use crate::error::{Error, Result};
use crate::model::{Body, Environment, FolderSettings, KeyValue, Method, Request};
use crate::workspace::slugify;
use serde_json::Value;

const METHODS: &[&str] = &["get", "post", "put", "patch", "delete", "head", "options", "trace"];

pub fn parse(text: &str) -> Result<Imported> {
    let doc: Value = serde_json::from_str(text)
        .or_else(|_| serde_yaml::from_str(text))
        .map_err(|e| Error::Import(format!("not valid JSON or YAML: {e}")))?;
    if doc.get("swagger").is_some() {
        return Err(Error::Import("Swagger 2.0 is not supported; convert it to OpenAPI 3 first".into()));
    }
    let title = doc.pointer("/info/title").and_then(Value::as_str).unwrap_or("OpenAPI").to_string();
    let mut imported = Imported { name: title.clone(), ..Default::default() };
    imported.folders.push((
        String::new(),
        FolderSettings { name: title.clone(), description: str_at(&doc, "/info/description"), ..Default::default() },
    ));

    if let Some(server) = doc.pointer("/servers/0/url").and_then(Value::as_str) {
        let mut environment = Environment { description: format!("Imported from the \"{title}\" OpenAPI document"), ..Default::default() };
        environment.variables.insert("baseUrl".into(), server.trim_end_matches('/').into());
        imported.environments.push((title, environment));
    }

    let paths = doc.get("paths").and_then(Value::as_object).ok_or_else(|| Error::Import("missing `paths`".into()))?;
    for (path, item) in paths {
        let shared_params = item.get("parameters").and_then(Value::as_array).cloned().unwrap_or_default();
        for method in METHODS {
            let Some(operation) = item.get(*method) else { continue };
            let folder = operation.pointer("/tags/0").and_then(Value::as_str).map(slugify).unwrap_or_default();
            if !folder.is_empty() && !imported.folders.iter().any(|(id, _)| id == &folder) {
                let name = operation.pointer("/tags/0").and_then(Value::as_str).unwrap_or_default().to_string();
                imported.folders.push((folder.clone(), FolderSettings { name, ..Default::default() }));
            }
            let request = convert(&doc, path, method, operation, &shared_params);
            imported.add_request(&folder, request);
        }
    }
    Ok(imported)
}

fn convert(doc: &Value, path: &str, method: &str, op: &Value, shared: &[Value]) -> Request {
    let name = op
        .get("summary")
        .or_else(|| op.get("operationId"))
        .and_then(Value::as_str)
        .map_or_else(|| format!("{} {path}", method.to_uppercase()), str::to_string);
    let mut request = Request {
        name,
        description: str_at(op, "/description"),
        method: Method::from(method),
        url: format!("{{{{baseUrl}}}}{}", path.replace('{', "{{").replace('}', "}}")),
        ..Default::default()
    };

    let own = op.get("parameters").and_then(Value::as_array).cloned().unwrap_or_default();
    for param in shared.iter().chain(own.iter()) {
        let param = resolve(doc, param);
        let Some(name) = param.get("name").and_then(Value::as_str) else { continue };
        let required = param.get("required").and_then(Value::as_bool).unwrap_or(false);
        let example = param
            .get("example")
            .or_else(|| param.pointer("/schema/example"))
            .or_else(|| param.pointer("/schema/default"))
            .map(crate::model::value_to_string)
            .unwrap_or_default();
        let pair = KeyValue { key: name.to_string(), value: example, enabled: required };
        match param.get("in").and_then(Value::as_str) {
            Some("query") => request.query.push(pair),
            Some("header") => request.headers.push(pair),
            _ => {}
        }
    }

    if let Some(content) = op.pointer("/requestBody/content").map(|c| resolve(doc, c))
        && let Some(json) = content.get("application/json")
    {
        let example = json
            .get("example")
            .cloned()
            .or_else(|| json.pointer("/examples").and_then(|e| e.as_object()?.values().next()?.get("value").cloned()))
            .or_else(|| json.get("schema").map(|s| example_from_schema(doc, s, 0)));
        let content = example.map(|e| serde_json::to_string_pretty(&e).unwrap_or_default()).unwrap_or_else(|| "{}".into());
        request.body = Body::Json { content };
    }

    if let Some(responses) = op.get("responses").and_then(Value::as_object)
        && let Some(code) = responses.keys().find(|k| k.starts_with('2'))
    {
        request.tests.push(format!("status == {code}"));
    }
    request
}

/// Follows a local `$ref` such as `#/components/schemas/User`.
fn resolve<'a>(doc: &'a Value, value: &'a Value) -> &'a Value {
    match value.get("$ref").and_then(Value::as_str).and_then(|r| r.strip_prefix('#')) {
        Some(pointer) => doc.pointer(pointer).unwrap_or(value),
        None => value,
    }
}

/// Builds a placeholder JSON value from a schema, so bodies start useful.
fn example_from_schema(doc: &Value, schema: &Value, depth: u8) -> Value {
    let schema = resolve(doc, schema);
    if depth > 6 {
        return Value::Null;
    }
    if let Some(example) = schema.get("example") {
        return example.clone();
    }
    match schema.get("type").and_then(Value::as_str) {
        Some("object") | None if schema.get("properties").is_some() => {
            let mut object = serde_json::Map::new();
            for (key, property) in schema["properties"].as_object().into_iter().flatten() {
                object.insert(key.clone(), example_from_schema(doc, property, depth + 1));
            }
            Value::Object(object)
        }
        Some("array") => Value::Array(vec![example_from_schema(doc, &schema["items"], depth + 1)]),
        Some("integer") | Some("number") => 0.into(),
        Some("boolean") => false.into(),
        Some("string") => schema.pointer("/enum/0").cloned().unwrap_or_else(|| "string".into()),
        _ => Value::Null,
    }
}

fn str_at(v: &Value, pointer: &str) -> String {
    v.pointer(pointer).and_then(Value::as_str).unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r##"
openapi: 3.0.3
info: { title: Pet Store }
servers: [{ url: "https://pets.dev/v1/" }]
paths:
  /pets/{petId}:
    parameters:
      - { name: petId, in: path, required: true, schema: { type: integer } }
    get:
      summary: Get a pet
      tags: [pets]
      parameters:
        - { name: fields, in: query, schema: { type: string, example: name } }
      responses: { "200": { description: ok } }
  /pets:
    post:
      operationId: createPet
      tags: [pets]
      requestBody:
        content:
          application/json:
            schema: { $ref: "#/components/schemas/Pet" }
      responses: { "201": { description: created } }
components:
  schemas:
    Pet:
      type: object
      properties:
        name: { type: string }
        age: { type: integer }
"##;

    #[test]
    fn imports_operations_grouped_by_tag() {
        let imported = parse(DOC).unwrap();
        let ids: Vec<_> = imported.requests.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, vec!["pets/get-a-pet", "pets/createpet"]);

        let (_, get) = &imported.requests[0];
        assert_eq!(get.url, "{{baseUrl}}/pets/{{petId}}");
        assert_eq!(get.query[0].value, "name");
        assert!(!get.query[0].enabled, "optional parameters start disabled");
        assert_eq!(get.tests, vec!["status == 200"]);

        let (_, create) = &imported.requests[1];
        assert!(matches!(&create.body, Body::Json { content } if content.contains("\"age\": 0")));
        assert_eq!(imported.environments[0].1.variables["baseUrl"], "https://pets.dev/v1");
    }
}
