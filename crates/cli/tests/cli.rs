//! End-to-end tests of the `pt` binary: exit codes, output formats and the
//! long-running commands, against a temporary workspace and a local server.

use axum::Json;
use axum::Router;
use axum::extract::Path as UrlPath;
use axum::http::StatusCode;
use axum::routing::get;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// A local HTTP server for requests to hit, shared by all tests.
fn target_port() -> u16 {
    static PORT: OnceLock<u16> = OnceLock::new();
    *PORT.get_or_init(|| {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            runtime.block_on(async move {
                let app = Router::new().route("/ok", get(|| async { Json(serde_json::json!({ "ok": true, "name": "Ada" })) })).route(
                    "/status/{code}",
                    get(|UrlPath(code): UrlPath<u16>| async move { (StatusCode::from_u16(code).unwrap(), "status") }),
                );
                axum::serve(tokio::net::TcpListener::from_std(listener).unwrap(), app).await.unwrap();
            });
        });
        port
    })
}

/// A workspace whose `local` environment points at the test server.
struct Workspace {
    dir: tempfile::TempDir,
}

impl Workspace {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let ws = Self { dir };
        let output = ws.pt(&["init", "ws", "--name", "Demo"], ws.dir.path());
        assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
        std::fs::remove_dir_all(ws.root().join("collections/examples")).unwrap();
        std::fs::write(ws.root().join("environments/local.yaml"), format!("variables:\n  baseUrl: http://127.0.0.1:{}\n", target_port()))
            .unwrap();
        ws.write("collections/checks/01-ok.yaml", "url: '{{baseUrl}}/ok'\ntests:\n  - status == 200\n  - json($.name) == \"Ada\"\n");
        ws.write("collections/checks/02-fails.yaml", "url: '{{baseUrl}}/status/500'\ntests:\n  - status == 200\n");
        ws
    }

    fn root(&self) -> PathBuf {
        self.dir.path().join("ws")
    }

    fn write(&self, path: &str, content: &str) {
        let path = self.root().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn command(&self, args: &[&str], cwd: &Path) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_pt"));
        command.args(args).current_dir(cwd).env("NO_COLOR", "1").env_remove("PT_WORKSPACE").env_remove("PT_LOG");
        command
    }

    fn pt(&self, args: &[&str], cwd: &Path) -> Output {
        self.command(args, cwd).output().unwrap()
    }

    /// Runs `pt` inside the workspace.
    fn run(&self, args: &[&str]) -> Output {
        self.pt(args, &self.root())
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn code(output: &Output) -> i32 {
    output.status.code().unwrap_or(-1)
}

#[test]
fn init_creates_a_workspace() {
    let ws = Workspace::new();
    assert!(ws.root().join("post-tester.yaml").is_file());
    assert!(ws.root().join("environments/local.yaml").is_file());
    let output = ws.pt(&["init", "second"], ws.dir.path());
    assert_eq!(code(&output), 0);
    assert!(text(&output.stdout).contains("Created workspace \"second\""));
}

#[test]
fn send_exit_codes() {
    let ws = Workspace::new();
    let ok = ws.run(&["send", "checks/01-ok"]);
    assert_eq!(code(&ok), 0, "{}{}", text(&ok.stdout), text(&ok.stderr));
    let stdout = text(&ok.stdout);
    assert!(stdout.contains("200 OK") && stdout.contains("✓ status == 200"), "{stdout}");
    assert!(stdout.contains("\"name\": \"Ada\""), "the body is pretty-printed: {stdout}");

    let failed = ws.run(&["send", "checks/02-fails"]);
    assert_eq!(code(&failed), 1, "a failed test exits with 1");
    assert!(text(&failed.stdout).contains("✗ status == 200"));

    let unreachable = ws.run(&["send", "http://127.0.0.1:1/"]);
    assert_eq!(code(&unreachable), 1, "a request error exits with 1");

    let unknown = ws.run(&["send", "checks/missing"]);
    assert_eq!(code(&unknown), 2, "a usage error exits with 2");
    assert!(text(&unknown.stderr).contains("neither a saved request nor a URL"), "{}", text(&unknown.stderr));

    let port = target_port();
    let no_scheme = ws.run(&["send", &format!("127.0.0.1:{port}/ok"), "--no-body"]);
    assert_eq!(code(&no_scheme), 0, "{}", text(&no_scheme.stderr));
    assert!(!text(&no_scheme.stdout).contains("Ada"), "--no-body");
}

#[test]
fn history_is_written_before_exit() {
    let ws = Workspace::new();
    assert_eq!(code(&ws.run(&["send", "checks/01-ok", "--json"])), 0);
    let history = std::fs::read_to_string(ws.root().join(".post-tester/history.jsonl")).unwrap();
    let finished = history.lines().filter(|l| l.contains("\"type\":\"finished\"")).count();
    assert!(finished >= 1, "the request's last event reached the history file: {history}");
}

#[test]
fn load_reuses_or_opens_connections() {
    let ws = Workspace::new();
    for extra in [&[][..], &["--fresh-connections"][..]] {
        let mut args = vec!["load", "checks/01-ok", "-n", "6", "-c", "2", "--json"];
        args.extend_from_slice(extra);
        let output = ws.run(&args);
        assert_eq!(code(&output), 0, "{args:?}: {}", text(&output.stderr));
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["succeeded"], 6, "{report}");
    }
}

#[test]
fn send_json_prints_only_json() {
    let ws = Workspace::new();
    for (id, expected_code, outcome) in [("checks/01-ok", 0, "ok"), ("checks/02-fails", 1, "failed")] {
        let output = ws.run(&["send", id, "--json"]);
        assert_eq!(code(&output), expected_code);
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("stdout is exactly one JSON document");
        assert_eq!(json["outcome"], outcome);
        assert!(json["tests"].is_array());
    }
    let output = ws.run(&["send", "http://127.0.0.1:1/", "--json"]);
    assert_eq!(code(&output), 1);
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["outcome"], "error");
    assert!(json["error"].as_str().unwrap().contains("connect"), "{json}");
}

#[test]
fn run_reporters() {
    let ws = Workspace::new();
    let pretty = ws.run(&["run", "checks"]);
    assert_eq!(code(&pretty), 1, "one request fails");
    assert!(text(&pretty.stdout).contains("2 requests · 1 passed · 1 failed"), "{}", text(&pretty.stdout));

    let json = ws.run(&["run", "checks", "--reporter", "json"]);
    assert_eq!(code(&json), 1);
    let report: serde_json::Value = serde_json::from_slice(&json.stdout).expect("stdout is exactly the JSON report");
    assert_eq!(report["items"].as_array().unwrap().len(), 2);

    let junit = ws.run(&["run", "checks", "--reporter", "junit", "--output", "results.xml"]);
    assert_eq!(code(&junit), 1);
    let xml = std::fs::read_to_string(ws.root().join("results.xml")).unwrap();
    assert!(xml.starts_with("<?xml"));
    assert_eq!(xml.matches("<testcase ").count(), 2);
    assert!(xml.contains("<failure message=\"status == 200: "), "{xml}");

    let passing = ws.run(&["run", "-r", "checks/01-ok"]);
    assert_eq!(code(&passing), 0);

    let missing_output = ws.run(&["run", "checks", "--reporter", "junit"]);
    assert_eq!(code(&missing_output), 2);
    assert!(text(&missing_output.stderr).contains("--output"));

    let missing_folder = ws.run(&["run", "nope"]);
    assert_eq!(code(&missing_folder), 2);
}

#[test]
fn plugin_new_and_test() {
    let ws = Workspace::new();
    let created = ws.run(&["plugin", "new", "template-function", "shouty", "--dir", "plugins/shouty"]);
    assert_eq!(code(&created), 0, "{}", text(&created.stderr));
    let tested = ws.run(&["plugin", "test", "plugins/shouty"]);
    assert_eq!(code(&tested), 0, "{}{}", text(&tested.stdout), text(&tested.stderr));
    assert!(text(&tested.stdout).contains("0 failed"));
    let installed = ws.run(&["plugin", "install", "plugins/shouty", "--yes"]);
    assert_eq!(code(&installed), 0, "{}", text(&installed.stderr));
    let settings = std::fs::read_to_string(ws.root().join("post-tester.yaml")).unwrap();
    assert!(settings.contains("plugins/shouty"), "stored relative, with `/`: {settings}");
}

#[test]
fn import_and_export() {
    let ws = Workspace::new();
    ws.write("curl.txt", "curl -X POST https://api.example.com/users -d '{\"name\":\"Ada\"}'");
    let imported = ws.run(&["import", "curl.txt", "--into", "imported"]);
    assert_eq!(code(&imported), 0, "{}", text(&imported.stderr));
    assert!(ws.root().join("collections/imported/post-users.yaml").is_file());

    let unknown = ws.run(&["import", "missing.json"]);
    assert_eq!(code(&unknown), 2);
    assert!(text(&unknown.stderr).contains("cannot read missing.json"));

    let export = ws.run(&["export", "--to", "markdown/markdown"]);
    assert_eq!(code(&export), 2);
    let stderr = text(&export.stderr);
    assert!(stderr.contains("no exporter plugins are installed") && stderr.contains("pt plugin new exporter"), "{stderr}");
    assert!(export.stdout.is_empty());
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// A raw HTTP/1.1 GET; returns the status code and the body.
fn http_get(port: u16, path: &str) -> std::io::Result<(u16, String)> {
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    write!(stream, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n")?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let status = response.split(' ').nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let body = response.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default();
    Ok((status, body))
}

fn wait_for_exit(child: &mut std::process::Child, limit: Duration) -> std::process::ExitStatus {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if started.elapsed() > limit {
            child.kill().unwrap();
            panic!("pt did not exit within {limit:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn mock_serves_examples_and_stops_on_ctrl_c() {
    let ws = Workspace::new();
    ws.write(
        "collections/users/get.yaml",
        "url: '{{baseUrl}}/users/{{id}}'\nexamples:\n  - name: found\n    body: '{\"id\": \"{{id}}\"}'\n",
    );
    let port = free_port();
    let mut child = ws
        .command(&["mock", "users", "--port", &port.to_string()], &ws.root())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let started = Instant::now();
    let (status, body) = loop {
        match http_get(port, "/users/42") {
            Ok(result) => break result,
            Err(_) if started.elapsed() < Duration::from_secs(20) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => {
                child.kill().unwrap();
                panic!("the mock never answered: {e}");
            }
        }
    };
    assert_eq!(status, 200);
    assert!(body.contains("\"id\": \"42\""), "{body}");

    #[cfg(unix)]
    {
        let interrupted = Command::new("kill").args(["-INT", &child.id().to_string()]).status().unwrap();
        assert!(interrupted.success());
        let status = wait_for_exit(&mut child, Duration::from_secs(10));
        let mut stdout = String::new();
        child.stdout.take().unwrap().read_to_string(&mut stdout).unwrap();
        assert!(status.success(), "Ctrl+C is a normal way to stop: {status:?}\n{stdout}");
        assert!(stdout.contains("GET     /users/{{id}}") && stdout.contains("Stopped."), "{stdout}");
        assert!(stdout.contains("Mock /users/42"), "the hit is shown live: {stdout}");
    }
    #[cfg(not(unix))]
    child.kill().unwrap();
}

#[test]
fn mock_without_examples_is_a_clear_error() {
    let ws = Workspace::new();
    let output = ws.run(&["mock", "checks", "--port", &free_port().to_string()]);
    assert_eq!(code(&output), 2);
    assert!(text(&output.stderr).contains("no request in this folder has examples"));
}

/// `pt mcp` speaks JSON-RPC on stdout; warnings must go to stderr.
#[test]
fn mcp_writes_only_protocol_to_stdout() {
    let ws = Workspace::new();
    let settings = std::fs::read_to_string(ws.root().join("post-tester.yaml")).unwrap();
    std::fs::write(ws.root().join("post-tester.yaml"), format!("{settings}plugins:\n- plugins/does-not-exist\n")).unwrap();
    let mut child = ws.command(&["mcp"], &ws.root()).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let initialize = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "1" } }
    });
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, "{initialize}").unwrap();
    stdin.flush().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    let response: serde_json::Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("not JSON-RPC: {line:?} ({e})"));
    assert_eq!(response["result"]["serverInfo"]["name"], "post-tester", "{response}");

    drop(stdin); // the agent disconnects
    wait_for_exit(&mut child, Duration::from_secs(10));
    let mut rest = String::new();
    stdout.read_to_string(&mut rest).unwrap();
    assert!(rest.lines().all(|l| serde_json::from_str::<serde_json::Value>(l).is_ok()), "only JSON-RPC on stdout: {rest:?}");
    let mut stderr = String::new();
    child.stderr.take().unwrap().read_to_string(&mut stderr).unwrap();
    assert!(stderr.contains("warning: plugin"), "the plugin warning goes to stderr: {stderr}");
}
