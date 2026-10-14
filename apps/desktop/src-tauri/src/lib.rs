//! The post-tester desktop and mobile app.
//!
//! The app runs the same local server as `pt serve` on a random port inside
//! the process, and shows its web UI in a native window. There is a single
//! UI and a single engine for the browser, desktop and mobile.

use post_tester_core::events::EventBus;
use post_tester_core::secrets::SecretStore;
use post_tester_core::{Engine, Workspace};
use post_tester_plugin_host::PluginHost;
use post_tester_server::{ServerConfig, serve};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

/// Remembers which workspace was open last.
const LAST_WORKSPACE_FILE: &str = "last-workspace.txt";

struct AppState {
    engine: Arc<Engine>,
    plugins: Arc<PluginHost>,
    /// Relays work started elsewhere (terminal, AI agents over stdio) for the open workspace.
    follower: std::sync::Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

/// Starts relaying events from other post-tester processes for `workspace`.
fn follow(engine: &Engine, workspace: &Workspace) -> Option<tauri::async_runtime::JoinHandle<()>> {
    let dir = workspace.local_dir().ok()?;
    Some(tauri::async_runtime::spawn(post_tester_core::history::follower(engine.bus(), &dir)))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_dialog::init());
    builder
        .setup(|app| {
            let workspace = open_initial_workspace(app.handle())?;
            let engine = Arc::new(Engine::new(EventBus::default(), Some(workspace.clone()), secret_store()));
            let plugins = PluginHost::attach(&engine);
            if let Ok(dir) = workspace.local_dir() {
                let _ = post_tester_core::history::attach(engine.bus(), &dir);
            }

            let server = tauri::async_runtime::block_on(async {
                plugins.load_workspace(&workspace).await;
                let config = ServerConfig { port: 0, ..Default::default() };
                serve(config, engine.clone(), plugins.clone()).await
            })?;
            tracing::info!("post-tester server on {}", server.addr);

            let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::External(server.url.parse()?));
            #[cfg(desktop)]
            let window = window.title("post-tester").inner_size(1440.0, 900.0).min_inner_size(900.0, 600.0);
            window.build()?;

            #[cfg(desktop)]
            desktop::install_menu(app)?;
            let follower = std::sync::Mutex::new(follow(&engine, &workspace));
            app.manage(AppState { engine, plugins, follower });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running post-tester");
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
fn open_initial_workspace(app: &tauri::AppHandle) -> Result<Workspace, Box<dyn std::error::Error>> {
    let remembered = last_workspace_file(app).and_then(|f| std::fs::read_to_string(f).ok()).map(|p| PathBuf::from(p.trim()));
    if let Some(path) = remembered
        && let Ok(workspace) = Workspace::open(&path)
    {
        return Ok(workspace);
    }
    let default_dir = app.path().document_dir().or_else(|_| app.path().app_data_dir())?.join("post-tester");
    let workspace = open_or_create(&default_dir)?;
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
    use super::{AppState, open_or_create, remember};
    use tauri::menu::{Menu, MenuItem, Submenu};
    use tauri::{App, AppHandle, Manager};
    use tauri_plugin_dialog::DialogExt;

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

    /// Asks for a folder; opens it as a workspace, creating one if it is empty.
    fn pick_workspace(app: AppHandle) {
        let dialog_app = app.clone();
        dialog_app.dialog().file().set_title("Open or create a workspace").pick_folder(move |folder| {
            let Some(path) = folder.and_then(|f| f.into_path().ok()) else { return };
            match open_or_create(&path) {
                Ok(workspace) => {
                    let state = app.state::<AppState>();
                    state.engine.set_workspace(Some(workspace.clone()));
                    let mut follower = state.follower.lock().expect("follower lock poisoned");
                    if let Some(old) = follower.take() {
                        old.abort();
                    }
                    *follower = super::follow(&state.engine, &workspace);
                    drop(follower);
                    let plugins = state.plugins.clone();
                    for plugin in plugins.plugins() {
                        plugins.unload(&plugin.manifest.name);
                    }
                    tauri::async_runtime::block_on(plugins.load_workspace(&workspace));
                    remember(&app, workspace.root());
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.eval("location.reload()");
                    }
                }
                Err(error) => {
                    app.dialog().message(format!("Could not open {}:\n{error}", path.display())).title("post-tester").show(|_| {});
                }
            }
        });
    }
}
