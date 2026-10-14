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
                Some(host) if is_host_pattern(host) => Permission::Network(host.to_ascii_lowercase()),
                // Requests are matched by host name only, so a scheme, port or path
                // would make the permission silently never match.
                Some(host) if !host.is_empty() => {
                    return Err(format!(
                        "`{text}` is not a valid network permission: use a host name only, without scheme, port or path, e.g. network:api.example.com, network:*.example.com or network:*"
                    ));
                }
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
            problem(
                "name",
                format!("`{}` is not a valid name", self.name),
                "use lowercase letters, numbers and `-`, e.g. \"totp-generator\"",
            );
        }
        if self.description.trim().is_empty() {
            problem("description", "is empty".into(), "describe what the plugin does in one sentence");
        }
        if parse_version(&self.version).is_none() {
            problem("version", format!("`{}` is not a semantic version", self.version), "use MAJOR.MINOR.PATCH, e.g. \"1.0.0\"");
        }
        match check_engine(&self.engine, post_tester_core::VERSION) {
            Ok(true) => {}
            Ok(false) => problem(
                "engine",
                format!("`{}` does not include this post-tester version ({})", self.engine, post_tester_core::VERSION),
                &format!("use a range that includes {}, e.g. \"^{}\"", post_tester_core::VERSION, post_tester_core::VERSION),
            ),
            Err(message) => problem(
                "engine",
                message,
                "use `^x.y.z`, `~x.y.z`, `>=x.y.z`, `>=x.y.z <a.b.c`, `x.y.z`, `*`, or alternatives separated by `||`",
            ),
        }
        let main = Path::new(&self.main);
        if self.main.trim().is_empty()
            || main.is_absolute()
            || main.components().any(|c| !matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir))
        {
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

/// `*`, a host name, `*.` followed by a domain, or a bracketed IPv6 address.
fn is_host_pattern(host: &str) -> bool {
    if host == "*" {
        return true;
    }
    if let Some(ipv6) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        return !ipv6.is_empty() && ipv6.chars().all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.');
    }
    let name = host.strip_prefix("*.").unwrap_or(host);
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
}

/// A semantic version: `MAJOR.MINOR.PATCH`, with an optional pre-release
/// (`-beta.1`) and build metadata (`+abc`, ignored).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Version {
    core: (u64, u64, u64),
    /// Dot-separated pre-release identifiers; empty for a release.
    pre: Vec<String>,
}

impl Version {
    fn new(core: (u64, u64, u64)) -> Self {
        Self { core, pre: Vec::new() }
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        self.core.cmp(&other.core).then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            // A pre-release comes before its release: 1.0.0-beta < 1.0.0.
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => {
                for (a, b) in self.pre.iter().zip(&other.pre) {
                    let order = match (a.parse::<u64>(), b.parse::<u64>()) {
                        (Ok(a), Ok(b)) => a.cmp(&b),
                        (Ok(_), Err(_)) => Ordering::Less,
                        (Err(_), Ok(_)) => Ordering::Greater,
                        (Err(_), Err(_)) => a.cmp(b),
                    };
                    if order != Ordering::Equal {
                        return order;
                    }
                }
                self.pre.len().cmp(&other.pre.len())
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn parse_version(text: &str) -> Option<Version> {
    let text = text.split('+').next()?;
    let (core, pre) = match text.split_once('-') {
        Some((core, pre)) => (core, pre.split('.').map(str::to_string).collect::<Vec<_>>()),
        None => (text, Vec::new()),
    };
    if pre.iter().any(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')) {
        return None;
    }
    let mut parts = core.split('.').map(|p| if p.chars().all(|c| c.is_ascii_digit()) { p.parse::<u64>().ok() } else { None });
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(Version { core: version, pre })
}

/// `x.y.z` with optional parts left out or written as `x`/`*`: the parts present, and the core with zeros.
fn parse_partial(text: &str) -> Option<(usize, Version)> {
    let wildcard = |p: &str| matches!(p, "x" | "X" | "*");
    let core = text.split(['-', '+']).next()?;
    let parts: Vec<&str> = core.split('.').collect();
    let given = parts.iter().take_while(|p| !wildcard(p)).count();
    if parts.len() > 3 || parts[given..].iter().any(|p| !wildcard(p)) {
        return None;
    }
    if given == 3 {
        return parse_version(text).map(|v| (3, v));
    }
    let mut numbers = [0u64; 3];
    for (slot, part) in numbers.iter_mut().zip(&parts[..given]) {
        *slot = part.parse().ok()?;
    }
    (text == core).then(|| (given, Version::new((numbers[0], numbers[1], numbers[2]))))
}

/// A lower or upper limit of a range, and whether the version itself is included.
type Bound = Option<(Version, bool)>;

/// One comparator, such as `>=1.2.0` or `^0.3`, as the (lower, upper) bounds it allows.
fn comparator(text: &str) -> Result<(Bound, Bound), String> {
    let invalid = || format!("`{text}` is not a version or a range");
    let (op, rest) = ["^", "~", ">=", "<=", ">", "<", "="]
        .iter()
        .find_map(|op| text.strip_prefix(op).map(|rest| (*op, rest.trim_start())))
        .unwrap_or(("", text));
    let (given, version) = parse_partial(rest).ok_or_else(invalid)?;
    let (major, minor, patch) = version.core;
    let next = |given: usize| match given {
        0 => None,
        1 => Some(Version::new((major + 1, 0, 0))),
        2 => Some(Version::new((major, minor + 1, 0))),
        _ => Some(Version::new((major, minor, patch + 1))),
    };
    let at_least = Some((version.clone(), true));
    let full = given == 3;
    Ok(match op {
        "" | "=" if full => (at_least, Some((version, true))),
        // Partial versions are ranges: `1.2` means `>=1.2.0 <1.3.0`.
        "" | "=" => (if given == 0 { None } else { at_least }, next(given).map(|v| (v, false))),
        ">=" => (at_least, None),
        ">" if full => (Some((version, false)), None),
        ">" => (next(given).map(|v| (v, true)), None),
        "<" => (None, Some((version, false))),
        "<=" if full => (None, Some((version, true))),
        "<=" => (None, next(given).map(|v| (v, false))),
        // `~1.2.3` allows patch updates; `~1` allows minor updates.
        "~" => (at_least, next(given.min(2)).map(|v| (v, false))),
        // `^` allows changes that do not modify the left-most non-zero part.
        _ => {
            let upper = match (major, minor) {
                (0, 0) if given == 3 => Version::new((0, 0, patch + 1)),
                (0, _) if given >= 2 => Version::new((0, minor + 1, 0)),
                _ if given == 0 => return Ok((None, None)),
                _ => Version::new((major + 1, 0, 0)),
            };
            (at_least, Some((upper, false)))
        }
    })
}

/// True when `version` is in `range`. An invalid range or version never matches;
/// see [`check_engine`] for the reason.
pub fn engine_matches(range: &str, version: &str) -> bool {
    check_engine(range, version).unwrap_or(false)
}

/// Checks `version` against an npm-style `range`: `*`, `x.y.z`, `^x.y.z` (with
/// 0.x semantics), `~x.y.z`, comparators (`>=`, `>`, `<=`, `<`, `=`) combined
/// with spaces (all must match), and alternatives separated by `||`.
/// Pre-release versions (`1.0.0-beta.1`) only match ranges that name a
/// pre-release of the same `x.y.z`, as in npm.
pub fn check_engine(range: &str, version: &str) -> Result<bool, String> {
    let version = parse_version(version.trim()).ok_or_else(|| format!("`{version}` is not a semantic version"))?;
    if range.trim().is_empty() {
        return Err("the version range is empty".into());
    }
    let mut matched = false;
    for alternative in range.split("||") {
        let comparators: Vec<&str> = alternative.split_whitespace().collect();
        if comparators.is_empty() {
            return Err(format!("`{range}` has an empty alternative around `||`"));
        }
        if comparators.contains(&"-") {
            return Err(format!("hyphen ranges such as `{}` are not supported; use `>=a.b.c <=x.y.z`", alternative.trim()));
        }
        // Allow `>= 1.0.0` (an operator separated from its version).
        let mut joined = Vec::new();
        let mut pending: Option<&str> = None;
        for part in comparators {
            match pending.take() {
                Some(op) => joined.push(format!("{op}{part}")),
                None if ["^", "~", ">=", "<=", ">", "<", "="].contains(&part) => pending = Some(part),
                None => joined.push(part.to_string()),
            }
        }
        if let Some(op) = pending {
            return Err(format!("`{op}` in `{range}` is not followed by a version"));
        }
        let mut all = true;
        let mut names_prerelease_of_same_core = false;
        for text in &joined {
            let (lower, upper) = comparator(text)?;
            all &= lower.as_ref().is_none_or(|(v, inclusive)| if *inclusive { version >= *v } else { version > *v });
            all &= upper.as_ref().is_none_or(|(v, inclusive)| if *inclusive { version <= *v } else { version < *v });
            names_prerelease_of_same_core |= [lower, upper].iter().flatten().any(|(v, _)| !v.pre.is_empty() && v.core == version.core);
        }
        matched |= all && (version.pre.is_empty() || names_prerelease_of_same_core);
    }
    Ok(matched)
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
        assert!(!engine_matches("^0.1.3", "0.1.2"));
        assert!(engine_matches("^0.0.3", "0.0.3"));
        assert!(!engine_matches("^0.0.3", "0.0.4"));
        assert!(engine_matches("^1.2.0", "1.9.0"));
        assert!(!engine_matches("^1.2.0", "2.0.0"));
        assert!(engine_matches("^1", "1.9.9"));
        assert!(engine_matches("^0.1", "0.1.9"));
        assert!(engine_matches(">=0.1.0", "3.0.0"));
        assert!(engine_matches(">= 0.1.0", "3.0.0"));
        assert!(engine_matches("*", "0.0.1"));
        assert!(engine_matches("0.1.0", "0.1.0"));
        assert!(!engine_matches("0.1.0", "0.1.1"));
        assert!(engine_matches("=0.1.0", "0.1.0"));
        assert!(engine_matches("0.1", "0.1.7"));
        assert!(engine_matches("0.1.x", "0.1.7"));
        assert!(!engine_matches("0.1.x", "0.2.0"));
        // Tilde: patch updates only (minor updates for `~1`).
        assert!(engine_matches("~1.2.3", "1.2.9"));
        assert!(!engine_matches("~1.2.3", "1.3.0"));
        assert!(engine_matches("~1", "1.5.0"));
        // Comparator sets and alternatives.
        assert!(engine_matches(">=0.1.0 <1.0.0", "0.9.9"));
        assert!(!engine_matches(">=0.1.0 <1.0.0", "1.0.0"));
        assert!(engine_matches(">0.1.0", "0.1.1"));
        assert!(!engine_matches(">0.1.0", "0.1.0"));
        assert!(engine_matches("<=1.2", "1.2.5"));
        assert!(!engine_matches("<1.2", "1.2.0"));
        assert!(engine_matches("^0.1.0 || ^1.0.0", "1.4.0"));
        assert!(!engine_matches("^0.1.0 || ^1.0.0", "0.2.0"));
        // Build metadata is ignored.
        assert!(engine_matches("^1.0.0", "1.0.1+build.5"));
    }

    #[test]
    fn engine_ranges_with_pre_releases() {
        // A pre-release only matches ranges that mention a pre-release of the same version.
        assert!(!engine_matches("^0.2.0", "0.2.0-beta.1"));
        assert!(engine_matches("^0.2.0-alpha", "0.2.0-beta.1"));
        assert!(engine_matches(">=0.2.0-beta.2", "0.2.0-beta.10"));
        assert!(!engine_matches(">=0.2.0-beta.2", "0.2.0-beta.1"));
        assert!(engine_matches(">=0.2.0-beta.2", "0.2.0"));
        assert!(!engine_matches("<0.2.0", "0.2.0-beta.1"));
        assert!(engine_matches(">=0.1.0", "0.2.0"));
    }

    #[test]
    fn invalid_engine_ranges_say_why() {
        for (range, reason) in [
            ("", "empty"),
            ("latest", "is not a version or a range"),
            ("^1.2.3.4", "is not a version or a range"),
            ("1.0.0 - 2.0.0", "hyphen ranges"),
            ("^1.0.0 ||", "empty alternative"),
            (">=", "not followed by a version"),
        ] {
            let error = check_engine(range, "1.0.0").unwrap_err();
            assert!(error.contains(reason), "{range}: {error}");
        }
        let mut m = manifest();
        m.engine = ">=0.1 <1 junk".into();
        let problems = m.validate();
        assert_eq!(problems.len(), 1, "{problems:#?}");
        assert!(problems[0].message.contains("`junk` is not a version or a range"), "{}", problems[0]);
    }

    #[test]
    fn main_must_stay_inside_the_plugin_folder() {
        for main in ["../index.ts", "lib/../../index.ts", "/abs/index.ts", ""] {
            let mut m = manifest();
            m.main = main.into();
            assert_eq!(m.validate().len(), 1, "{main}");
        }
        let mut m = manifest();
        m.main = "./dist/index..min.js".into();
        assert_eq!(m.validate(), vec![]);
    }

    #[test]
    fn permissions() {
        assert_eq!(Permission::parse("network:API.github.com"), Ok(Permission::Network("api.github.com".into())));
        assert!(Permission::parse("network:").is_err());
        assert_eq!(Permission::parse("network:*.example.com"), Ok(Permission::Network("*.example.com".into())));
        assert_eq!(Permission::parse("network:[::1]"), Ok(Permission::Network("[::1]".into())));
        for unmatchable in ["network:https://api.github.com", "network:localhost:8080", "network:api.github.com/v3", "network:api.*.com"] {
            let err = Permission::parse(unmatchable).unwrap_err();
            assert!(err.contains("host name only"), "{unmatchable}: {err}");
        }
    }
}
