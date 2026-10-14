//! Keeps secret values out of what agents see.
//!
//! Requests reference secrets as `{{secret:name}}`, directly or through
//! variables (an environment variable `apiKey: "{{secret:key}}"` used as
//! `?key={{apiKey}}`). Once a request is sent, the resolved value can appear
//! in the final URL, in error messages, test details or a response that
//! echoes the request. So before a result goes to the agent, every value of
//! a secret the request could have used is masked, also where it appears
//! URL-encoded (`%20`, `+`, `%2F`...), whichever characters the encoder chose.

use post_tester_core::Engine;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;

/// What masked values are replaced with.
const MASK: &str = "•••";
/// Shorter values are not masked: they would garble unrelated text.
const MIN_LENGTH: usize = 4;

/// Secret values to mask.
#[derive(Debug, Default)]
pub(super) struct Secrets {
    values: Vec<String>,
}

/// Collects the names of secrets referenced in templates.
#[derive(Debug, Default)]
pub(super) struct References {
    names: BTreeSet<String>,
}

impl References {
    /// Secrets referenced by the workspace's environments and settings, which
    /// any request may use through variables.
    pub(super) fn workspace(engine: &Engine) -> Self {
        let mut references = Self::default();
        if let Some(workspace) = engine.workspace() {
            if let Ok(settings) = workspace.settings() {
                references.add(&settings);
            }
            for name in workspace.environments().unwrap_or_default() {
                if let Ok(environment) = workspace.read_environment(&name) {
                    references.add(&environment);
                }
            }
        }
        references
    }

    /// Adds the secrets a saved request and its folders reference.
    pub(super) fn add_saved_request(&mut self, engine: &Engine, id: &str) {
        let Some(workspace) = engine.workspace() else { return };
        if let Ok(request) = workspace.read_request(id) {
            self.add(&request);
        }
        if let Ok(folders) = workspace.folder_chain(id) {
            self.add(&folders);
        }
    }

    /// Adds every `{{secret:name}}` in `value` (serialized), including nested ones
    /// such as `{{totp(secret: "{{secret:otp}}")}}`.
    pub(super) fn add(&mut self, value: &impl Serialize) {
        let Ok(text) = serde_json::to_string(value) else { return };
        let mut rest = text.as_str();
        while let Some(start) = rest.find("{{") {
            rest = &rest[start + 2..];
            let Some(name) = rest.trim_start().strip_prefix("secret:") else { continue };
            if let Some(end) = name.find("}}") {
                let name = name[..end].trim();
                if !name.is_empty() {
                    self.names.insert(name.to_string());
                }
            }
        }
    }

    /// Looks up the values. Secrets that cannot be read are skipped: they
    /// were not used either.
    pub(super) async fn resolve(&self, engine: &Engine) -> Secrets {
        let mut values = BTreeSet::new();
        for name in &self.names {
            if let Ok(Some(value)) = engine.secrets().get(name).await
                && value.chars().count() >= MIN_LENGTH
            {
                values.insert(value);
            }
        }
        // Longest first, so a secret that contains another is masked whole.
        let mut values: Vec<String> = values.into_iter().collect();
        values.sort_by_key(|v| std::cmp::Reverse(v.len()));
        Secrets { values }
    }
}

impl Secrets {
    /// Masks every secret value in every string of `value`, keys included.
    pub(super) fn redact(&self, value: &mut Value) {
        if self.values.is_empty() {
            return;
        }
        match value {
            Value::String(text) => self.redact_text(text),
            Value::Array(items) => items.iter_mut().for_each(|item| self.redact(item)),
            Value::Object(map) => {
                let entries = std::mem::take(map);
                for (mut key, mut item) in entries {
                    self.redact_text(&mut key);
                    self.redact(&mut item);
                    map.insert(key, item);
                }
            }
            _ => {}
        }
    }

    fn redact_text(&self, text: &mut String) {
        for secret in &self.values {
            if let Some(masked) = mask(text, secret.as_bytes()) {
                *text = masked;
            }
        }
    }
}

/// `text` with every occurrence of `secret` masked, raw or URL-encoded;
/// `None` if there is none.
fn mask(text: &str, secret: &[u8]) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out: Option<String> = None;
    let (mut copied, mut i) = (0, 0);
    while i < bytes.len() {
        match match_at(bytes, i, secret) {
            Some(end) => {
                let out = out.get_or_insert_with(|| String::with_capacity(text.len()));
                // Matches start and end on ASCII or on whole encoded sequences, so these are character boundaries.
                out.push_str(&text[copied..i]);
                out.push_str(MASK);
                copied = end;
                i = end;
            }
            None => i += 1,
        }
    }
    let mut out = out?;
    out.push_str(&text[copied..]);
    Some(out)
}

/// Where a match of `secret` starting at `start` ends. Each byte of the
/// secret may appear as itself, as `%XX`, or (a space) as `+`.
fn match_at(text: &[u8], start: usize, secret: &[u8]) -> Option<usize> {
    let hex = |b: u8| (b as char).to_digit(16);
    let mut j = start;
    for &wanted in secret {
        match text.get(j..)? {
            [b, ..] if *b == wanted => j += 1,
            [b'%', high, low, ..] if hex(*high)? * 16 + hex(*low)? == u32::from(wanted) => j += 3,
            [b'+', ..] if wanted == b' ' => j += 1,
            _ => return None,
        }
    }
    // A match that starts or ends inside a multi-byte character cannot be the secret.
    let boundary = |k: usize| k == text.len() || (text[k] as i8) >= -0x40;
    (j > start && boundary(start) && boundary(j)).then_some(j)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn finds_references_including_nested_and_spaced_ones() {
        let mut references = References::default();
        references.add(&json!({
            "url": "{{base}}?k={{ secret:api-key }}",
            "headers": [{ "key": "X-OTP", "value": "{{totp(secret: \"{{secret:otp}}\")}}" }],
            "other": "{{secret}} {{secret:}} {{secret:unterminated",
        }));
        assert_eq!(references.names.iter().collect::<Vec<_>>(), ["api-key", "otp"]);
    }

    #[test]
    fn masks_plain_and_url_encoded_values_everywhere() {
        let secrets = Secrets { values: vec!["p@ss wörd/1".into(), "abcd".into()] };
        let mut value = json!({
            "url": "https://x.dev/?key=p%40ss%20w%C3%B6rd/1&b=p@ss+w%c3%b6rd%2F1",
            "error": "invalid token p@ss wörd/1!",
            "list": ["abcd-1", 5, "ab%63d"],
            "abcd": true,
            "safe": "p@ss wörd/2 abc",
        });
        secrets.redact(&mut value);
        assert_eq!(
            value,
            json!({
                "url": "https://x.dev/?key=•••&b=•••",
                "error": "invalid token •••!",
                "list": ["•••-1", 5, "•••"],
                "•••": true,
                "safe": "p@ss wörd/2 abc",
            })
        );
    }
}
