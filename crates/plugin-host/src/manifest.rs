//! `plugin.json`: what a plugin is, what it provides and what it needs.

use serde::{Deserialize, Serialize};
use std::path::Path;
use ts_rs::TS;

/// The plugin types a plugin can provide, and the `definePlugin` key for each.
pub const PLUGIN_TYPES: &[(&str, &str)] = &[
    ("template-function", "templateFunctions"),
    ("auth", "auth"),
    ("hook", "hooks"),
    ("importer", "importers"),
    ("exporter", "exporters"),
    ("codegen", "codegen"),
    ("viewer", "viewers"),
    ("mcp-tool", "mcpTools"),
];

/// The contents of `plugin.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct Manifest {
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    #[ts(skip)]
    pub schema: Option<String>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub display_name: Option<String>,
    pub description: String,
    pub version: String,
    /// Compatible post-tester versions, e.g. `^0.1.0`.
    pub engine: String,
    pub main: String,
    pub provides: Vec<String>,
    pub permissions: Vec<String>,
}

/// A permission a plugin can ask for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Permission {
    /// `network:<host>` or `network:*`.
    Network(String),
    SecretsRead,
    FsRead,
    FsWrite,
}

impl Permission {
    pub fn parse(text: &str) -> Result<Self, String> {
        Ok(match text {
            "secrets:read" => Permission::SecretsRead,
            "fs:read" => Permission::FsRead,
            "fs:write" => Permission::FsWrite,
            _ => match text.strip_prefix("network:") {
                Some(host) if !host.is_empty() => Permission::Network(host.to_ascii_lowercase()),
                _ => {
                    return Err(format!(
                        "unknown permission `{text}`. Valid permissions: network:<host>, network:*, secrets:read, fs:read, fs:write"
                    ));
                }
            },
        })
    }

    /// A sentence for the install prompt.
    pub fn explain(&self) -> String {
        match self {
            Permission::Network(host) if host == "*" => "Send HTTP requests to ANY website".into(),
            Permission::Network(host) => format!("Send HTTP requests to {host}"),
            Permission::SecretsRead => "Read your secrets".into(),
            Permission::FsRead => "Read files in the workspace".into(),
            Permission::FsWrite => "Create and change files in the workspace".into(),
        }
    }
}

/// A problem found while validating a plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct Problem {
    /// Where: `plugin.json`, `plugin.json: name`, `index.ts`…
    pub location: String,
    pub message: String,
    /// How to fix it.
    pub fix: String,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}\n  fix: {}", self.location, self.message, self.fix)
    }
}

impl Manifest {
    /// Reads `plugin.json` from a plugin folder.
    pub fn load(dir: &Path) -> Result<Self, Problem> {
        let path = dir.join("plugin.json");
        let text = std::fs::read_to_string(&path).map_err(|e| Problem {
            location: "plugin.json".into(),
            message: format!("cannot read {}: {e}", path.display()),
            fix: "create plugin.json in the plugin folder (run `pt plugin new` for a template)".into(),
        })?;
        serde_json::from_str(&text).map_err(|e| Problem {
            location: format!("plugin.json:{}:{}", e.line(), e.column()),
            message: e.to_string(),
            fix: "required fields: name, description, version, engine, main, provides, permissions".into(),
        })
    }

    /// Checks every field. Returns all problems at once, so they can be fixed in one go.
    pub fn validate(&self) -> Vec<Problem> {
        let mut problems = Vec::new();
        let mut problem = |field: &str, message: String, fix: &str| {
            problems.push(Problem { location: format!("plugin.json: {field}"), message, fix: fix.into() });
        };
        let valid_name = !self.name.is_empty()
            && self.name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && !self.name.starts_with('-');
        if !valid_name {
            problem("name", format!("`{}` is not a valid name", self.name), "use lowercase letters, numbers and `-`, e.g. \"totp-generator\"");
        }
        if self.description.trim().is_empty() {
            problem("description", "is empty".into(), "describe what the plugin does in one sentence");
        }
        if parse_version(&self.version).is_none() {
            problem("version", format!("`{}` is not a semantic version", self.version), "use MAJOR.MINOR.PATCH, e.g. \"1.0.0\"");
        }
        if !engine_matches(&self.engine, post_tester_core::VERSION) {
            problem(
                "engine",
                format!("`{}` does not include this post-tester version ({})", self.engine, post_tester_core::VERSION),
                &format!("use a range that includes {}, e.g. \"^{}\"", post_tester_core::VERSION, post_tester_core::VERSION),
            );
        }
        if self.main.contains("..") || Path::new(&self.main).is_absolute() {
            problem("main", format!("`{}` must be a file inside the plugin folder", self.main), "use a relative path like \"index.ts\"");
        }
        if self.provides.is_empty() {
            problem("provides", "is empty".into(), "list at least one plugin type, e.g. [\"template-function\"]");
        }
        for kind in &self.provides {
            if !PLUGIN_TYPES.iter().any(|(t, _)| t == kind) {
                let valid: Vec<&str> = PLUGIN_TYPES.iter().map(|(t, _)| *t).collect();
                problem("provides", format!("unknown plugin type `{kind}`"), &format!("use one of: {}", valid.join(", ")));
            }
        }
        for permission in &self.permissions {
            if let Err(message) = Permission::parse(permission) {
                problem("permissions", message, "remove it or fix the spelling");
            }
        }
        problems
    }

    pub fn display_name(&self) -> &str {
        self.display_name.as_deref().unwrap_or(&self.name)
    }

    pub fn parsed_permissions(&self) -> Vec<Permission> {
        self.permissions.iter().filter_map(|p| Permission::parse(p).ok()).collect()
    }
}

fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let core = text.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

/// Supports `*`, exact versions, `>=x.y.z`, and caret ranges `^x.y.z` (with 0.x semantics).
pub fn engine_matches(range: &str, version: &str) -> bool {
    let range = range.trim();
    let Some(v) = parse_version(version) else { return false };
    if range == "*" {
        return true;
    }
    if let Some(min) = range.strip_prefix(">=").and_then(|r| parse_version(r.trim())) {
        return v >= min;
    }
    let Some(min) = parse_version(range.trim_start_matches('^').trim_start_matches('=')) else { return false };
    if !range.starts_with('^') {
        return v == min;
    }
    let compatible = match min {
        (0, 0, patch) => v == (0, 0, patch),
        (0, minor, _) => v.0 == 0 && v.1 == minor,
        (major, _, _) => v.0 == major,
    };
    compatible && v >= min
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Manifest {
        Manifest {
            schema: None,
            name: "totp-generator".into(),
            display_name: None,
            description: "Generates TOTP codes.".into(),
            version: "1.0.0".into(),
            engine: format!("^{}", post_tester_core::VERSION),
            main: "index.ts".into(),
            provides: vec!["template-function".into()],
            permissions: vec!["network:api.example.com".into()],
        }
    }

    #[test]
    fn a_valid_manifest_has_no_problems() {
        assert_eq!(manifest().validate(), vec![]);
    }

    #[test]
    fn every_problem_is_reported_with_a_fix() {
        let mut m = manifest();
        m.name = "Bad Name".into();
        m.version = "1".into();
        m.provides = vec!["widget".into()];
        m.permissions = vec!["everything".into()];
        let problems = m.validate();
        assert_eq!(problems.len(), 4, "{problems:#?}");
        assert!(problems.iter().all(|p| !p.fix.is_empty()));
    }

    #[test]
    fn engine_ranges() {
        assert!(engine_matches("^0.1.0", "0.1.5"));
        assert!(!engine_matches("^0.1.0", "0.2.0"));
        assert!(engine_matches("^1.2.0", "1.9.0"));
        assert!(!engine_matches("^1.2.0", "2.0.0"));
        assert!(engine_matches(">=0.1.0", "3.0.0"));
        assert!(engine_matches("*", "0.0.1"));
        assert!(engine_matches("0.1.0", "0.1.0"));
    }

    #[test]
    fn permissions() {
        assert_eq!(Permission::parse("network:API.github.com"), Ok(Permission::Network("api.github.com".into())));
        assert!(Permission::parse("network:").is_err());
    }
}
