//! The post-tester desktop and mobile app.
//!
//! The app runs the same local server as `pt serve` on a random port inside
//! the process, and shows its web UI in a native window. There is a single
//! UI and a single engine for the browser, desktop and mobile.
//!
//! Each open workspace is a [`Session`]: its own engine, plugins, history and
//! server (with a fresh session token). Opening another workspace starts a new
//! session, points the window at it, then stops the old one, so nothing from
//! the previous workspace (history file, plugins, variables, MCP sessions)
//! carries over.

use post_tester_core::events::EventBus;
use post_tester_core::secrets::SecretStore;
use post_tester_core::{Engine, Workspace};
use post_tester_plugin_host::PluginHost;
use post_tester_server::{RunningServer, ServerConfig, serve};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

/// Remembers which workspace was open last.
const LAST_WORKSPACE_FILE: &str = "last-workspace.txt";
/// How long requests of the previous workspace may take to finish after a switch.
const SWITCH_GRACE: Duration = Duration::from_secs(2);

/// The engine and server of the open workspace.
struct Session {
    server: RunningServer,
    /// Relays work started elsewhere (terminal, AI agents over stdio) for this workspace.
    follower: Option<tauri::async_runtime::JoinHandle<()>>,
}

impl Session {
    /// Loads the workspace's history and plugins, and starts its server.
    async fn start(workspace: &Workspace) -> Result<Self, String> {
        let engine = Arc::new(Engine::new(EventBus::default(), Some(workspace.clone()), secret_store()));
        let plugins = PluginHost::attach(&engine);
        let local_dir = workspace.local_dir().ok();
        if let Some(dir) = &local_dir
            && let Err(e) = post_tester_core::history::attach(engine.bus(), dir)
        {
            tracing::warn!("history is not saved: {e}");
        }
        for (path, problems) in plugins.load_workspace(workspace).await {
            tracing::warn!("plugin {path} was not loaded: {problems:?}");
        }
        let config = ServerConfig { port: 0, ..Default::default() };
        let server = serve(config, engine.clone(), plugins).await.map_err(|e| format!("could not start the local server: {e}"))?;
        tracing::info!("post-tester server for {} on {}", workspace.root().display(), server.addr);
        let follower = local_dir.map(|dir| tauri::async_runtime::spawn(post_tester_core::history::follower(engine.bus(), &dir)));
        Ok(Self { server, follower })
    }

    async fn stop(self) {
        if let Some(follower) = self.follower {
            follower.abort();
        }
        if let Err(e) = self.server.shutdown(SWITCH_GRACE).await {
            tracing::warn!("the previous workspace's server did not stop cleanly: {e}");
        }
    }
}

struct AppState {
    /// Locked for the whole of a workspace switch, so two switches never interleave.
    session: tokio::sync::Mutex<Option<Session>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_dialog::init());
    builder
        .setup(|app| {
            if let Err(error) = setup(app) {
                // Explain the problem in a dialog instead of crashing without a word.
                #[cfg(desktop)]
                desktop::fatal(app.handle(), &error);
                #[cfg(mobile)]
                return Err(error.into());
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running post-tester");
}

fn setup(app: &mut tauri::App) -> Result<(), String> {
    let workspace = open_initial_workspace(app.handle())?;
    let session = tauri::async_runtime::block_on(Session::start(&workspace))?;
    let url = session.server.url.parse().map_err(|e| format!("invalid server address: {e}"))?;
    app.manage(AppState { session: tokio::sync::Mutex::new(Some(session)) });

    let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url));
    #[cfg(desktop)]
    let window = window.title("post-tester").inner_size(1440.0, 900.0).min_inner_size(900.0, 600.0);
    window.build().map_err(|e| format!("could not open the window: {e}"))?;

    #[cfg(desktop)]
    desktop::install_menu(app).map_err(|e| format!("could not create the menu: {e}"))?;
    Ok(())
}

#[cfg(desktop)]
fn secret_store() -> Arc<dyn SecretStore> {
    post_tester_core::secrets::default_store()
}

#[cfg(mobile)]
fn secret_store() -> Arc<dyn SecretStore> {
    // Mobile keychains are not wired up yet; secrets last until the app closes.
    Arc::new(post_tester_core::secrets::MemorySecrets::default())
}

/// The last workspace, or a default one in the user's documents (created if needed).
fn open_initial_workspace(app: &tauri::AppHandle) -> Result<Workspace, String> {
    let remembered = last_workspace_file(app).and_then(|f| std::fs::read_to_string(f).ok()).map(|p| PathBuf::from(p.trim()));
    if let Some(path) = remembered
        && let Ok(workspace) = Workspace::open(&path)
    {
        return Ok(workspace);
    }
    let base = app.path().document_dir().or_else(|_| app.path().app_data_dir()).map_err(|e| format!("no folder for a workspace: {e}"))?;
    let default_dir = base.join("post-tester");
    let workspace = open_or_create(&default_dir).map_err(|e| format!("could not open or create {}: {e}", default_dir.display()))?;
    remember(app, workspace.root());
    Ok(workspace)
}

fn open_or_create(dir: &Path) -> post_tester_core::Result<Workspace> {
    match Workspace::open(dir) {
        Ok(workspace) => Ok(workspace),
        Err(_) => {
            std::fs::create_dir_all(dir)?;
            let name = dir.file_name().map_or("Workspace".into(), |n| n.to_string_lossy().into_owned());
            Workspace::init(dir, &name)
        }
    }
}

fn last_workspace_file(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|dir| dir.join(LAST_WORKSPACE_FILE))
}

fn remember(app: &tauri::AppHandle, root: &Path) {
    if let Some(file) = last_workspace_file(app) {
        let _ = file.parent().map(std::fs::create_dir_all);
        let _ = std::fs::write(file, root.to_string_lossy().as_bytes());
    }
}

#[cfg(desktop)]
mod desktop {
    use super::{AppState, Session, open_or_create, remember};
    use std::path::PathBuf;
    use tauri::menu::{Menu, MenuItem, Submenu};
    use tauri::{App, AppHandle, Manager};
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

    const OPEN_WORKSPACE: &str = "open-workspace";

    /// The default menu (keeps copy/paste shortcuts) plus a Workspace menu.
    pub fn install_menu(app: &App) -> tauri::Result<()> {
        let menu = Menu::default(app.handle())?;
        let open = MenuItem::with_id(app, OPEN_WORKSPACE, "Open Workspace…", true, Some("CmdOrCtrl+O"))?;
        menu.append(&Submenu::with_items(app, "Workspace", true, &[&open])?)?;
        app.set_menu(menu)?;
        app.on_menu_event(|app, event| {
            if event.id() == OPEN_WORKSPACE {
                pick_workspace(app.clone());
            }
        });
        Ok(())
    }

    /// Shows a startup error, then quits when it is dismissed.
    pub fn fatal(app: &AppHandle, error: &str) {
        let handle = app.clone();
        app.dialog()
            .message(format!("post-tester could not start:\n\n{error}"))
            .title("post-tester")
            .kind(MessageDialogKind::Error)
            .show(move |_| handle.exit(1));
    }

    fn error(app: &AppHandle, message: String) {
        app.dialog().message(message).title("post-tester").kind(MessageDialogKind::Error).show(|_| {});
    }

    /// Asks for a folder; opens it as a workspace, creating one if it is empty.
    fn pick_workspace(app: AppHandle) {
        let dialog_app = app.clone();
        dialog_app.dialog().file().set_title("Open or create a workspace").pick_folder(move |folder| {
            let Some(path) = folder.and_then(|f| f.into_path().ok()) else { return };
            tauri::async_runtime::spawn(switch_workspace(app, path));
        });
    }

    async fn switch_workspace(app: AppHandle, path: PathBuf) {
        let state = app.state::<AppState>();
        // A second switch waits until this one is complete.
        let mut current = state.session.lock().await;
        let workspace = match open_or_create(&path) {
            Ok(workspace) => workspace,
            Err(e) => return error(&app, format!("Could not open {}:\n{e}", path.display())),
        };
        let session = match Session::start(&workspace).await {
            Ok(session) => session,
            Err(e) => return error(&app, format!("Could not open {}:\n{e}", path.display())),
        };
        let url = session.server.url.clone();
        let previous = current.replace(session);
        remember(&app, workspace.root());
        if let Some(window) = app.get_webview_window("main")
            && let Ok(url) = url.parse()
            && let Err(e) = window.navigate(url)
        {
            tracing::warn!("could not show the new workspace: {e}");
        }
        if let Some(previous) = previous {
            previous.stop().await;
        }
    }
}
