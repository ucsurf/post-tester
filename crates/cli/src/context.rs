//! Builds the engine, workspace and plugin host that every command uses.

use anyhow::{Context as _, Result};
use post_tester_core::events::EventBus;
use post_tester_core::secrets;
use post_tester_core::{Engine, Workspace};
use post_tester_plugin_host::PluginHost;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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
        OPENED.lock().expect("opened lock poisoned").push(engine.bus().clone());
        let plugins = PluginHost::attach(&engine);
        if let Some(workspace) = &workspace {
            // History is a convenience: if it can't be written, carry on without it.
            if let Err(e) = workspace.local_dir().and_then(|dir| Ok(post_tester_core::history::attach(engine.bus(), &dir)?)) {
                eprintln!("{} history is not saved: {e}", crate::style::warning_label());
            }
            for (path, problems) in plugins.load_workspace(workspace).await {
                eprintln!("{} plugin {path} was not loaded:", crate::style::warning_label());
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

/// Every bus a command opened, so [`flush_history`] can reach them on exit.
static OPENED: Mutex<Vec<EventBus>> = Mutex::new(Vec::new());

/// Waits until the last events are written to the history file. History is
/// written in the background, so this must run before the process exits.
pub fn flush_history() {
    for bus in OPENED.lock().expect("opened lock poisoned").drain(..) {
        bus.flush();
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
