//! Builds the engine, workspace and plugin host that every command uses.

use anyhow::{Context as _, Result};
use post_tester_core::events::EventBus;
use post_tester_core::secrets;
use post_tester_core::{Engine, Workspace};
use post_tester_plugin_host::PluginHost;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug)]
pub struct Context {
    pub engine: Arc<Engine>,
    pub plugins: Arc<PluginHost>,
}

impl Context {
    /// Opens the workspace at `path`, or the one containing the current folder.
    /// With `require`, a missing workspace is an error that explains `pt init`.
    pub async fn open(path: Option<&Path>, require: bool) -> Result<Self> {
        let workspace = find_workspace(path)?;
        if require && workspace.is_none() {
            anyhow::bail!("no workspace found here or in any parent folder. Create one with `pt init`, or pass --workspace <dir>");
        }
        let engine = Arc::new(Engine::new(EventBus::default(), workspace.clone(), secrets::default_store()));
        let plugins = PluginHost::attach(&engine);
        if let Some(workspace) = &workspace {
            for (path, problems) in plugins.load_workspace(workspace).await {
                eprintln!("{} plugin {path} was not loaded:", crate::style::yellow("warning:"));
                for problem in problems {
                    eprintln!("  {problem}");
                }
            }
        }
        Ok(Self { engine, plugins })
    }

    pub fn workspace(&self) -> Result<Workspace> {
        Ok(self.engine.require_workspace()?)
    }
}

fn find_workspace(path: Option<&Path>) -> Result<Option<Workspace>> {
    match path {
        Some(path) => Ok(Some(Workspace::open(path).with_context(|| format!("cannot open workspace {}", path.display()))?)),
        None => {
            let cwd: PathBuf = std::env::current_dir()?;
            Workspace::discover(&cwd).map(Workspace::open).transpose().map_err(Into::into)
        }
    }
}
