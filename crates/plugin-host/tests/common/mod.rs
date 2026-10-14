//! Helpers shared by the integration tests.
#![allow(dead_code)] // Each test binary uses a different subset.

use post_tester_core::Engine;
use post_tester_core::secrets::MemorySecrets;
use post_tester_plugin_host::PluginHost;
use post_tester_plugin_host::sandbox::TestOutcome;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// An engine without a workspace, with one in-memory secret.
pub fn engine() -> Arc<Engine> {
    Arc::new(Engine::new(Default::default(), None, Arc::new(MemorySecrets::with(&[("signing-key", "s3cret")]))))
}

/// An engine with a plugin host attached.
pub fn host() -> (Arc<Engine>, Arc<PluginHost>) {
    let engine = engine();
    let host = PluginHost::attach(&engine);
    (engine, host)
}

/// The repository root (two levels above this crate).
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("repository root")
}

/// Formats failed plugin tests for an assertion message.
pub fn failures(tests: &[TestOutcome]) -> String {
    tests
        .iter()
        .filter(|t| !t.passed)
        .map(|t| format!("  ✗ {}: {}", t.name, t.detail.as_deref().unwrap_or("")))
        .collect::<Vec<_>>()
        .join("\n")
}
