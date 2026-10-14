//! A workspace on disk.
//!
//! ```text
//! my-workspace/
//! ├── post-tester.yaml          workspace settings
//! ├── collections/
//! │   └── auth/
//! │       ├── _folder.yaml      settings shared by the folder
//! │       └── login.yaml        one request per file → id "auth/login"
//! ├── environments/
//! │   └── staging.yaml
//! ├── plugins/                  workspace plugins (optional)
//! └── .post-tester/             local data, never committed
//! ```
//!
//! Requests and folders are identified by their path inside `collections/`,
//! without the `.yaml` extension, always using `/`.

use crate::error::{Error, Result};
use crate::model::{Environment, FolderSettings, Request, WorkspaceSettings};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::path::{Path, PathBuf};
use ts_rs::TS;

pub const SETTINGS_FILE: &str = "post-tester.yaml";
pub const FOLDER_FILE: &str = "_folder.yaml";
const COLLECTIONS: &str = "collections";
const ENVIRONMENTS: &str = "environments";
const LOCAL_DIR: &str = ".post-tester";

/// A folder in the collection tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Folder {
    /// Path inside `collections/` (empty for the root).
    pub id: String,
    pub name: String,
    pub settings: FolderSettings,
    pub folders: Vec<Folder>,
    pub requests: Vec<RequestEntry>,
}

/// A request as listed in the tree (without loading every detail).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RequestEntry {
    pub id: String,
    pub name: String,
    pub method: String,
    pub url: String,
    /// Set when the file could not be read, so the UI can show the problem.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error: Option<String>,
}

impl Folder {
    /// Every request id in this folder and its sub-folders, in display order.
    pub fn request_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.requests.iter().map(|r| r.id.clone()).collect();
        for folder in &self.folders {
            ids.extend(folder.request_ids());
        }
        ids
    }

    /// Finds a sub-folder by id.
    pub fn find(&self, id: &str) -> Option<&Folder> {
        if self.id == id {
            return Some(self);
        }
        self.folders.iter().find_map(|f| f.find(id))
    }
}

/// A workspace folder on disk. Cheap to clone.
#[derive(Debug, Clone)]
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    /// Opens an existing workspace.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        if !root.join(SETTINGS_FILE).is_file() {
            return Err(Error::file(root.join(SETTINGS_FILE), "not a post-tester workspace. Run `pt init` to create one here"));
        }
        Ok(Self { root })
    }

    /// Creates a new workspace with a sample request. Fails if one already exists.
    pub fn init(root: impl Into<PathBuf>, name: &str) -> Result<Self> {
        let root = root.into();
        if root.join(SETTINGS_FILE).exists() {
            return Err(Error::file(root.join(SETTINGS_FILE), "a workspace already exists here"));
        }
        std::fs::create_dir_all(root.join(COLLECTIONS))?;
        std::fs::create_dir_all(root.join(ENVIRONMENTS))?;
        let workspace = Self { root };
        workspace.save_settings(&WorkspaceSettings {
            name: name.to_string(),
            default_environment: Some("local".into()),
            ..Default::default()
        })?;
        let mut local = Environment::default();
        local.variables.insert("baseUrl".into(), "https://httpbin.org".into());
        workspace.write_environment("local", &local)?;
        workspace.write_request(
            "examples/get-ip",
            &Request {
                name: "Get my IP".into(),
                description: "A sample request. Edit or delete it.".into(),
                url: "{{baseUrl}}/ip".into(),
                tests: vec!["status == 200".into(), "json($.origin) exists".into()],
                ..Default::default()
            },
        )?;
        std::fs::write(workspace.root.join(".gitignore"), format!("{LOCAL_DIR}/\n"))?;
        Ok(workspace)
    }

    /// Searches `start` and its parents for a workspace.
    pub fn discover(start: &Path) -> Option<PathBuf> {
        start.ancestors().find(|dir| dir.join(SETTINGS_FILE).is_file()).map(Path::to_path_buf)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The folder for local-only data (history, caches). Created on demand.
    pub fn local_dir(&self) -> Result<PathBuf> {
        let dir = self.root.join(LOCAL_DIR);
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    pub fn settings(&self) -> Result<WorkspaceSettings> {
        read_yaml(&self.root.join(SETTINGS_FILE))
    }

    pub fn save_settings(&self, settings: &WorkspaceSettings) -> Result<()> {
        write_yaml(&self.root.join(SETTINGS_FILE), settings)
    }

    // ── Requests and folders ────────────────────────────────────────────────

    /// The whole collection tree. Unreadable files are listed with an error
    /// instead of failing the whole tree.
    pub fn tree(&self) -> Result<Folder> {
        let dir = self.root.join(COLLECTIONS);
        std::fs::create_dir_all(&dir)?;
        read_folder(&dir, "")
    }

    pub fn read_request(&self, id: &str) -> Result<Request> {
        let path = self.request_path(id)?;
        if !path.is_file() {
            return Err(Error::NotFound(format!("request `{id}`")));
        }
        let mut request: Request = read_yaml(&path)?;
        if request.name.is_empty() {
            request.name = last_segment(id).to_string();
        }
        Ok(request)
    }

    pub fn write_request(&self, id: &str, request: &Request) -> Result<()> {
        write_yaml(&self.request_path(id)?, request)
    }

    pub fn delete_request(&self, id: &str) -> Result<()> {
        let path = self.request_path(id)?;
        std::fs::remove_file(&path).map_err(|e| Error::file(path, e))
    }

    pub fn request_exists(&self, id: &str) -> bool {
        self.request_path(id).is_ok_and(|p| p.is_file())
    }

    /// Folder settings from the root down to the request's own folder.
    pub fn folder_chain(&self, request_id: &str) -> Result<Vec<FolderSettings>> {
        validate_id(request_id)?;
        let mut chain = Vec::new();
        let mut dir = self.root.join(COLLECTIONS);
        let segments: Vec<&str> = request_id.split('/').collect();
        for segment in std::iter::once("").chain(segments[..segments.len() - 1].iter().copied()) {
            if !segment.is_empty() {
                dir.push(segment);
            }
            let file = dir.join(FOLDER_FILE);
            if file.is_file() {
                chain.push(read_yaml(&file)?);
            }
        }
        Ok(chain)
    }

    pub fn read_folder_settings(&self, folder_id: &str) -> Result<FolderSettings> {
        let path = self.folder_path(folder_id)?.join(FOLDER_FILE);
        if path.is_file() { read_yaml(&path) } else { Ok(FolderSettings::default()) }
    }

    pub fn write_folder_settings(&self, folder_id: &str, settings: &FolderSettings) -> Result<()> {
        write_yaml(&self.folder_path(folder_id)?.join(FOLDER_FILE), settings)
    }

    fn request_path(&self, id: &str) -> Result<PathBuf> {
        validate_id(id)?;
        Ok(self.root.join(COLLECTIONS).join(format!("{id}.yaml")))
    }

    fn folder_path(&self, id: &str) -> Result<PathBuf> {
        if id.is_empty() {
            return Ok(self.root.join(COLLECTIONS));
        }
        validate_id(id)?;
        Ok(self.root.join(COLLECTIONS).join(id))
    }

    // ── Environments ────────────────────────────────────────────────────────

    pub fn environments(&self) -> Result<Vec<String>> {
        let dir = self.root.join(ENVIRONMENTS);
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut names: Vec<String> = std::fs::read_dir(dir)?
            .filter_map(std::result::Result::ok)
            .filter_map(|e| e.file_name().to_string_lossy().strip_suffix(".yaml").map(str::to_string))
            .collect();
        names.sort();
        Ok(names)
    }

    pub fn read_environment(&self, name: &str) -> Result<Environment> {
        let path = self.environment_path(name)?;
        if !path.is_file() {
            let available = self.environments().unwrap_or_default().join(", ");
            return Err(Error::NotFound(format!("environment `{name}` (available: {available})")));
        }
        read_yaml(&path)
    }

    pub fn write_environment(&self, name: &str, environment: &Environment) -> Result<()> {
        write_yaml(&self.environment_path(name)?, environment)
    }

    pub fn delete_environment(&self, name: &str) -> Result<()> {
        let path = self.environment_path(name)?;
        std::fs::remove_file(&path).map_err(|e| Error::file(path, e))
    }

    fn environment_path(&self, name: &str) -> Result<PathBuf> {
        if name.contains('/') {
            return Err(Error::InvalidRequest(format!("environment name `{name}` may not contain `/`")));
        }
        validate_id(name)?;
        Ok(self.root.join(ENVIRONMENTS).join(format!("{name}.yaml")))
    }
}

/// Reads one folder of the collection tree, and its sub-folders.
fn read_folder(dir: &Path, id: &str) -> Result<Folder> {
    let settings_path = dir.join(FOLDER_FILE);
    let settings: FolderSettings = if settings_path.is_file() { read_yaml(&settings_path)? } else { FolderSettings::default() };
    let default_name = if id.is_empty() { "Collections".to_string() } else { last_segment(id).to_string() };
    let mut folder = Folder {
        id: id.to_string(),
        name: if settings.name.is_empty() { default_name } else { settings.name.clone() },
        settings,
        folders: Vec::new(),
        requests: Vec::new(),
    };
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(std::result::Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if file_name.starts_with('.') || file_name == FOLDER_FILE {
            continue;
        }
        let child_id = |name: &str| if id.is_empty() { name.to_string() } else { format!("{id}/{name}") };
        // The entry's type usually comes with the directory listing; only
        // symbolic links need another look at what they point to.
        let is_dir = match entry.file_type() {
            Ok(t) if !t.is_symlink() => t.is_dir(),
            _ => path.is_dir(),
        };
        if is_dir {
            folder.folders.push(read_folder(&path, &child_id(&file_name))?);
        } else if let Some(stem) = file_name.strip_suffix(".yaml").or_else(|| file_name.strip_suffix(".yml")) {
            let request_id = child_id(stem);
            folder.requests.push(match read_yaml::<Request>(&path) {
                Ok(r) => RequestEntry {
                    name: if r.name.is_empty() { stem.to_string() } else { r.name },
                    id: request_id,
                    method: r.method.0,
                    url: r.url,
                    error: None,
                },
                Err(e) => RequestEntry {
                    id: request_id,
                    name: stem.to_string(),
                    method: String::new(),
                    url: String::new(),
                    error: Some(e.to_string()),
                },
            });
        }
    }
    Ok(folder)
}

/// Ids are relative paths made of simple segments, so they can never escape
/// the workspace, and work as file names on every system (workspaces are
/// shared through git between macOS, Linux and Windows).
pub fn validate_id(id: &str) -> Result<()> {
    let valid_segment = |s: &str| {
        !s.is_empty()
            && !s.starts_with('.')
            // Windows silently drops a trailing dot or space, so `a.` would be `a`.
            && !s.ends_with(['.', ' '])
            && !s.starts_with(' ')
            && s.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' '))
            && !is_reserved_on_windows(s)
    };
    if id.is_empty() || !id.split('/').all(valid_segment) {
        return Err(Error::InvalidRequest(format!(
            "`{id}` is not a valid id. Use letters, numbers, spaces, `-`, `_` and `.` (not at the start or end of a name, \
             and not a Windows device name such as `con` or `nul`), separated by `/`"
        )));
    }
    Ok(())
}

/// Device names Windows reserves, with or without an extension (`con`, `nul.txt`).
fn is_reserved_on_windows(segment: &str) -> bool {
    let stem = segment.split('.').next().unwrap_or(segment).to_ascii_lowercase();
    matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
        || (stem.len() == 4
            && (stem.starts_with("com") || stem.starts_with("lpt"))
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0')
}

/// Turns a display name into an id segment: "Get User #1" → "get-user-1".
pub fn slugify(name: &str) -> String {
    let mut slug = String::new();
    for c in name.trim().chars() {
        if c.is_alphanumeric() {
            slug.extend(c.to_lowercase());
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-').to_string();
    if slug.is_empty() {
        "request".into()
    } else if is_reserved_on_windows(&slug) {
        format!("{slug}-1") // `con` is not a valid file name on Windows
    } else {
        slug
    }
}

fn last_segment(id: &str) -> &str {
    id.rsplit('/').next().unwrap_or(id)
}

pub(crate) fn read_yaml<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text = std::fs::read_to_string(path).map_err(|e| Error::file(path, e))?;
    if text.trim().is_empty() {
        return serde_yaml::from_str("{}").map_err(|e| Error::file(path, e));
    }
    serde_yaml::from_str(&text).map_err(|e| Error::file(path, e))
}

/// Makes temporary file names unique within this process.
static TMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub(crate) fn write_yaml<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_yaml::to_string(value).map_err(|e| Error::file(path, e))?;
    // Write to a temporary file first, so a crash never leaves a half-written
    // file. Its name is unique, so two writers (e.g. the app and `pt import`)
    // never write into the same temporary file; it starts with `.` so the
    // tree never lists it.
    let file_name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let unique = TMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_file_name(format!(".{file_name}.{}.{unique}.tmp", std::process::id()));
    std::fs::write(&tmp, text).map_err(|e| Error::file(&tmp, e))?;
    replace_file(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        Error::file(path, e)
    })
}

/// Moves `from` over `to`, replacing it atomically.
///
/// On Windows `rename` replaces existing files too (`MoveFileEx` with
/// `MOVEFILE_REPLACE_EXISTING`), but fails while another program (an editor,
/// a virus scanner, a sync client) briefly has the file open, so it is retried
/// for a moment before giving up.
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut attempts = 0;
    loop {
        match std::fs::rename(from, to) {
            Err(e) if cfg!(windows) && e.kind() == std::io::ErrorKind::PermissionDenied && attempts < 10 => {
                attempts += 1;
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            result => return result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_creates_a_usable_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::init(dir.path(), "Demo").unwrap();
        assert_eq!(ws.settings().unwrap().name, "Demo");
        assert_eq!(ws.environments().unwrap(), vec!["local"]);
        let tree = ws.tree().unwrap();
        assert_eq!(tree.request_ids(), vec!["examples/get-ip"]);
        assert_eq!(ws.read_request("examples/get-ip").unwrap().name, "Get my IP");
        assert!(Workspace::init(dir.path(), "Again").is_err());
    }

    #[test]
    fn folder_settings_are_inherited_from_the_root_down() {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::init(dir.path(), "Demo").unwrap();
        ws.write_folder_settings("", &FolderSettings { name: "root".into(), ..Default::default() }).unwrap();
        ws.write_folder_settings("a", &FolderSettings { name: "a".into(), ..Default::default() }).unwrap();
        ws.write_request("a/b/req", &Request::default()).unwrap();
        let chain: Vec<String> = ws.folder_chain("a/b/req").unwrap().into_iter().map(|f| f.name).collect();
        assert_eq!(chain, vec!["root", "a"]);
    }

    #[test]
    fn ids_cannot_escape_the_workspace() {
        for bad in ["../x", "a/../../x", "/etc/passwd", "", "a//b", ".hidden", "a/", "x\\y", "C:x"] {
            assert!(validate_id(bad).is_err(), "{bad} should be rejected");
        }
        assert!(validate_id("auth/login v2").is_ok());
    }

    #[test]
    fn ids_work_as_file_names_everywhere() {
        for bad in ["a.", "a ", " a", "con", "NUL", "aux.json", "com1", "lpt9/x", "x/prn"] {
            assert!(validate_id(bad).is_err(), "{bad} should be rejected");
        }
        for good in ["v1.2", "console", "com0", "com", "my request", "émoji-ok"] {
            assert!(validate_id(good).is_ok(), "{good} should be accepted");
        }
    }

    #[test]
    fn writes_replace_files_and_leave_no_temporary_files() {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::init(dir.path(), "Demo").unwrap();
        for name in ["first", "second"] {
            ws.write_request("a/req", &Request { name: name.into(), ..Default::default() }).unwrap();
        }
        assert_eq!(ws.read_request("a/req").unwrap().name, "second");
        let files: Vec<_> = std::fs::read_dir(dir.path().join("collections/a")).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(files, vec!["req.yaml"]);
    }

    #[test]
    fn broken_files_are_listed_with_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::init(dir.path(), "Demo").unwrap();
        std::fs::write(dir.path().join("collections/broken.yaml"), "method: [").unwrap();
        let tree = ws.tree().unwrap();
        let broken = tree.requests.iter().find(|r| r.id == "broken").unwrap();
        assert!(broken.error.is_some());
    }

    #[test]
    fn slugs() {
        assert_eq!(slugify("Get User #1"), "get-user-1");
        assert_eq!(slugify("  "), "request");
        assert_eq!(slugify("Con"), "con-1");
        assert!(validate_id(&slugify("Aux")).is_ok());
    }
}
