//! Secrets: values that must never be written to workspace files.
//!
//! Requests reference them as `{{secret:name}}`. Where the value comes from is
//! pluggable: the OS keychain (desktop), environment variables (CI), or memory (tests).

use crate::error::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[async_trait]
pub trait SecretStore: Send + Sync {
    /// Short name shown to users, e.g. "OS keychain".
    fn name(&self) -> &str;
    async fn get(&self, name: &str) -> Result<Option<String>>;
    async fn set(&self, name: &str, value: &str) -> Result<()>;
    async fn delete(&self, name: &str) -> Result<()>;
}

/// The environment variable that holds a secret: `api-key` → `PT_SECRET_API_KEY`.
pub fn env_var_name(name: &str) -> String {
    let upper: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' }).collect();
    format!("PT_SECRET_{upper}")
}

/// Reads secrets from `PT_SECRET_*` environment variables. Read-only. Ideal for CI.
#[derive(Debug, Default)]
pub struct EnvSecrets;

#[async_trait]
impl SecretStore for EnvSecrets {
    fn name(&self) -> &str {
        "environment variables"
    }
    async fn get(&self, name: &str) -> Result<Option<String>> {
        Ok(std::env::var(env_var_name(name)).ok())
    }
    async fn set(&self, name: &str, _value: &str) -> Result<()> {
        Err(std::io::Error::other(format!("environment variables are read-only; export {} instead", env_var_name(name))).into())
    }
    async fn delete(&self, name: &str) -> Result<()> {
        self.set(name, "").await
    }
}

/// Keeps secrets in memory. Used in tests and as a fallback.
#[derive(Debug, Default)]
pub struct MemorySecrets(Mutex<HashMap<String, String>>);

impl MemorySecrets {
    pub fn with(pairs: &[(&str, &str)]) -> Self {
        Self(Mutex::new(pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()))
    }
}

#[async_trait]
impl SecretStore for MemorySecrets {
    fn name(&self) -> &str {
        "memory"
    }
    async fn get(&self, name: &str) -> Result<Option<String>> {
        Ok(self.0.lock().expect("secret lock poisoned").get(name).cloned())
    }
    async fn set(&self, name: &str, value: &str) -> Result<()> {
        self.0.lock().expect("secret lock poisoned").insert(name.into(), value.into());
        Ok(())
    }
    async fn delete(&self, name: &str) -> Result<()> {
        self.0.lock().expect("secret lock poisoned").remove(name);
        Ok(())
    }
}

/// Tries several stores in order. Writes go to the first store.
pub struct ChainedSecrets(pub Vec<Arc<dyn SecretStore>>);

impl std::fmt::Debug for ChainedSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.0.iter().map(|s| s.name())).finish()
    }
}

#[async_trait]
impl SecretStore for ChainedSecrets {
    fn name(&self) -> &str {
        self.0.first().map_or("none", |s| s.name())
    }
    async fn get(&self, name: &str) -> Result<Option<String>> {
        for store in &self.0 {
            // A failing store (e.g. a locked keychain) should not hide the others.
            if let Ok(Some(value)) = store.get(name).await {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }
    async fn set(&self, name: &str, value: &str) -> Result<()> {
        match self.0.first() {
            Some(store) => store.set(name, value).await,
            None => Err(std::io::Error::other("no secret store configured").into()),
        }
    }
    async fn delete(&self, name: &str) -> Result<()> {
        for store in &self.0 {
            let _ = store.delete(name).await;
        }
        Ok(())
    }
}

/// The default store for people: `PT_SECRET_*` variables first (so CI and
/// one-off overrides never touch the keychain), then the OS keychain.
#[cfg(feature = "keyring")]
pub fn default_store() -> Arc<dyn SecretStore> {
    Arc::new(EnvFirst(Arc::new(KeyringSecrets)))
}

/// Reads environment variables before another store; writes go to that store.
pub struct EnvFirst(pub Arc<dyn SecretStore>);

impl std::fmt::Debug for EnvFirst {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("EnvFirst").field(&self.0.name()).finish()
    }
}

#[async_trait]
impl SecretStore for EnvFirst {
    fn name(&self) -> &str {
        self.0.name()
    }
    async fn get(&self, name: &str) -> Result<Option<String>> {
        match EnvSecrets.get(name).await? {
            Some(value) => Ok(Some(value)),
            None => self.0.get(name).await,
        }
    }
    async fn set(&self, name: &str, value: &str) -> Result<()> {
        self.0.set(name, value).await
    }
    async fn delete(&self, name: &str) -> Result<()> {
        self.0.delete(name).await
    }
}

/// Secrets in the operating system's keychain.
#[cfg(feature = "keyring")]
#[derive(Debug, Default)]
pub struct KeyringSecrets;

#[cfg(feature = "keyring")]
impl KeyringSecrets {
    const SERVICE: &str = "post-tester";

    fn entry(name: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(Self::SERVICE, name).map_err(keychain_error)
    }
}

#[cfg(feature = "keyring")]
fn keychain_error(e: keyring::Error) -> crate::Error {
    std::io::Error::other(format!("keychain: {e}")).into()
}

#[cfg(feature = "keyring")]
#[async_trait]
impl SecretStore for KeyringSecrets {
    fn name(&self) -> &str {
        "OS keychain"
    }
    async fn get(&self, name: &str) -> Result<Option<String>> {
        match Self::entry(name)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(keychain_error(e)),
        }
    }
    async fn set(&self, name: &str, value: &str) -> Result<()> {
        Self::entry(name)?.set_password(value).map_err(keychain_error)
    }
    async fn delete(&self, name: &str) -> Result<()> {
        match Self::entry(name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(keychain_error(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_var_names() {
        assert_eq!(env_var_name("api-key"), "PT_SECRET_API_KEY");
        assert_eq!(env_var_name("db.pass"), "PT_SECRET_DB_PASS");
    }

    #[tokio::test]
    async fn chain_falls_through() {
        let chain = ChainedSecrets(vec![Arc::new(MemorySecrets::default()), Arc::new(MemorySecrets::with(&[("a", "1")]))]);
        assert_eq!(chain.get("a").await.unwrap().as_deref(), Some("1"));
        chain.set("b", "2").await.unwrap();
        assert_eq!(chain.get("b").await.unwrap().as_deref(), Some("2"));
    }
}
