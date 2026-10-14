//! Importers: bring requests in from other formats.
//!
//! | Format    | Function                |
//! |-----------|-------------------------|
//! | cURL      | [`curl::parse`]         |
//! | Postman   | [`postman::parse`]      |
//! | OpenAPI 3 | [`openapi::parse`]      |
//!
//! [`detect`] picks the right one automatically.

pub mod curl;
pub mod openapi;
pub mod postman;

use crate::error::{Error, Result};
use crate::model::{Environment, FolderSettings, Request};
use crate::workspace::{Workspace, slugify};
use serde::Serialize;

/// Everything an import produced, ready to be written into a workspace.
#[derive(Debug, Clone, Default)]
pub struct Imported {
    pub name: String,
    /// (relative id, request), e.g. ("users/get-user", ...).
    pub requests: Vec<(String, Request)>,
    pub folders: Vec<(String, FolderSettings)>,
    pub environments: Vec<(String, Environment)>,
}

/// What was written by [`Imported::write_to`].
#[derive(Debug, Clone, Serialize)]
pub struct ImportSummary {
    pub folder: String,
    pub requests: Vec<String>,
    pub environments: Vec<String>,
}

impl Imported {
    /// Adds a request, making its id unique within this import.
    pub fn add_request(&mut self, folder: &str, request: Request) {
        let base = slugify(if request.name.is_empty() { &request.url } else { &request.name });
        let join = |name: &str| if folder.is_empty() { name.to_string() } else { format!("{folder}/{name}") };
        let mut id = join(&base);
        let mut n = 2;
        while self.requests.iter().any(|(existing, _)| existing == &id) {
            id = join(&format!("{base}-{n}"));
            n += 1;
        }
        self.requests.push((id, request));
    }

    /// Writes everything under `collections/<into>/`. Existing files are not overwritten;
    /// clashing names get a numeric suffix.
    pub fn write_to(&self, workspace: &Workspace, into: &str) -> Result<ImportSummary> {
        let into = if into.is_empty() { slugify(&self.name) } else { into.to_string() };
        let prefixed = |id: &str| if id.is_empty() { into.clone() } else { format!("{into}/{id}") };
        let mut written = Vec::new();
        for (id, settings) in &self.folders {
            workspace.write_folder_settings(&prefixed(id), settings)?;
        }
        for (id, request) in &self.requests {
            let mut target = prefixed(id);
            let mut n = 2;
            while workspace.request_exists(&target) {
                target = format!("{}-{n}", prefixed(id));
                n += 1;
            }
            workspace.write_request(&target, request)?;
            written.push(target);
        }
        let mut environments = Vec::new();
        let existing = workspace.environments()?;
        for (name, environment) in &self.environments {
            let name = slugify(name);
            if existing.contains(&name) {
                continue; // never overwrite the user's environment
            }
            workspace.write_environment(&name, environment)?;
            environments.push(name);
        }
        Ok(ImportSummary { folder: into, requests: written, environments })
    }
}

/// The formats that can be imported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Curl,
    Postman,
    Openapi,
}

/// Guesses the format of `text`.
pub fn detect(text: &str) -> Option<Format> {
    let trimmed = text.trim_start();
    if trimmed.starts_with("curl ") || trimmed.starts_with("curl\t") {
        return Some(Format::Curl);
    }
    let value: serde_json::Value = serde_json::from_str(text).or_else(|_| serde_yaml::from_str(text)).ok()?;
    if value.get("openapi").is_some() || value.get("swagger").is_some() {
        Some(Format::Openapi)
    } else if value.pointer("/info/schema").and_then(|s| s.as_str()).is_some_and(|s| s.contains("getpostman.com"))
        || (value.get("info").is_some() && value.get("item").is_some())
    {
        Some(Format::Postman)
    } else {
        None
    }
}

/// Imports `text` in the given (or detected) format.
pub fn import(text: &str, format: Option<Format>) -> Result<Imported> {
    let format = format.or_else(|| detect(text)).ok_or_else(|| {
        Error::Import("unrecognized format. Supported: a cURL command, a Postman v2.1 collection, or an OpenAPI 3 document".into())
    })?;
    match format {
        Format::Curl => {
            let request = curl::parse(text)?;
            let mut imported = Imported { name: "Imported".into(), ..Default::default() };
            imported.add_request("", request);
            Ok(imported)
        }
        Format::Postman => postman::parse(text),
        Format::Openapi => openapi::parse(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_formats() {
        assert_eq!(detect("curl https://x.dev"), Some(Format::Curl));
        assert_eq!(detect(r#"{"openapi":"3.0.0"}"#), Some(Format::Openapi));
        assert_eq!(detect("openapi: 3.1.0\ninfo: {}\n"), Some(Format::Openapi));
        assert_eq!(detect(r#"{"info":{"schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},"item":[]}"#), Some(Format::Postman));
        assert_eq!(detect("hello"), None);
    }

    #[test]
    fn duplicate_names_get_suffixes() {
        let mut imported = Imported::default();
        imported.add_request("a", Request { name: "Get".into(), ..Default::default() });
        imported.add_request("a", Request { name: "Get".into(), ..Default::default() });
        let ids: Vec<_> = imported.requests.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, vec!["a/get", "a/get-2"]);
    }
}
