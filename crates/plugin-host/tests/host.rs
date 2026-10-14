//! End-to-end tests: plugins and request scripts running inside a real engine,
//! against a local HTTP server.

mod common;

use axum::Router;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use post_tester_core::events::{Outcome, Source, Span, SpanKind, Summary};
use post_tester_core::extensions::{Extensions, ScriptInput, ScriptPhase, ScriptRequest};
use post_tester_core::model::Request;
use post_tester_core::secrets::MemorySecrets;
use post_tester_core::{Engine, SendOptions, Workspace};
use post_tester_plugin_host::PluginHost;
use post_tester_plugin_host::manifest::Problem;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

// ── Helpers ─────────────────────────────────────────────────────────────────

async fn start_server() -> String {
    let app = Router::new()
        .route("/json", get(|| async { axum::Json(json!({ "token": "abc", "user": { "id": 7 } })) }))
        .route(
            "/echo-headers",
            get(|headers: HeaderMap| async move {
                let pairs: serde_json::Map<String, serde_json::Value> =
                    headers.iter().map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().into())).collect();
                axum::Json(pairs)
            }),
        )
        .route(
            "/status/{code}",
            get(|axum::extract::Path(code): axum::extract::Path<u16>| async move { StatusCode::from_u16(code).unwrap().into_response() }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

/// Writes a plugin with an `index.ts` into `parent/name` and returns its folder.
fn write_plugin(parent: &Path, name: &str, provides: &[&str], permissions: &[&str], code: &str) -> PathBuf {
    let dir = parent.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = json!({
        "name": name,
        "description": "A plugin used in tests.",
        "version": "1.0.0",
        "engine": format!("^{}", post_tester_core::VERSION),
        "main": "index.ts",
        "provides": provides,
        "permissions": permissions,
    });
    std::fs::write(dir.join("plugin.json"), manifest.to_string()).unwrap();
    std::fs::write(dir.join("index.ts"), code).unwrap();
    dir
}

async fn load(host: &PluginHost, dir: &Path, granted: &[&str]) -> Arc<post_tester_plugin_host::Plugin> {
    host.load(dir, granted.iter().map(ToString::to_string).collect()).await.unwrap_or_else(|p| panic!("{}", show(&p)))
}

fn example(name: &str) -> PathBuf {
    common::repo_root().join("examples/plugins").join(name)
}

/// A request from its JSON form (the same fields as a request YAML file).
fn request(value: serde_json::Value) -> Request {
    serde_json::from_value(value).unwrap()
}

fn body_json(execution: &post_tester_core::Execution) -> serde_json::Value {
    serde_json::from_slice(&execution.response.as_ref().expect("a response").body).unwrap()
}

fn show(problems: &[Problem]) -> String {
    problems.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n")
}

fn span(engine: &Engine) -> Span {
    Span::start(engine.bus(), Source::User, None, SpanKind::Request, "test", Summary::default())
}

/// A plugin with one MCP tool that runs `body` (with `input` and `ctx` in scope).
fn tool_plugin(parent: &Path, name: &str, permissions: &[&str], body: &str) -> PathBuf {
    let code = format!(
        r#"import {{ definePlugin }} from "@post-tester/plugin-sdk";
export default definePlugin({{
  mcpTools: {{
    run: {{ description: "Runs test code.", run: async (input, ctx) => {{ {body} }} }},
  }},
}});
"#
    );
    write_plugin(parent, name, &["mcp-tool"], permissions, &code)
}

async fn call_tool(plugin: &post_tester_plugin_host::Plugin, input: serde_json::Value) -> Result<serde_json::Value, String> {
    plugin.call(None, "mcpTool", "run", json!({ "input": input })).await.map_err(|e| e.to_string())
}

// ── Template functions, auth and hooks in real requests ────────────────────

#[tokio::test]
async fn template_functions_are_used_in_request_urls() {
    let base = start_server().await;
    let (engine, host) = common::host();
    let dir = tempfile::tempdir().unwrap();
    let plugin = write_plugin(
        dir.path(),
        "paths",
        &["template-function"],
        &[],
        r#"import { definePlugin } from "@post-tester/plugin-sdk";
export default definePlugin({
  templateFunctions: {
    route: {
      description: "Returns a route by name.",
      args: [{ name: "name", type: "string", description: "Route name" }],
      run: ({ name }) => ({ user: "json", headers: "echo-headers" })[name],
    },
  },
});
"#,
    );
    load(&host, &plugin, &[]).await;

    let execution = engine
        .send(
            &request(json!({ "url": format!("{base}/{{{{route(\"user\")}}}}"), "tests": ["json($.user.id) == 7"] })),
            SendOptions::new(Source::User),
        )
        .await;
    assert_eq!(execution.outcome, Outcome::Ok, "{:?}", execution.error);
    assert!(execution.request.unwrap().url.as_str().ends_with("/json"));

    // Named arguments, and an example plugin (TOTP) in a header.
    load(&host, &example("totp-generator"), &[]).await;
    let execution = engine
        .send(
            &request(json!({
                "url": format!("{base}/{{{{route(name: \"headers\")}}}}"),
                "headers": [{ "key": "X-OTP", "value": "{{totp(\"GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ\")}}" }],
            })),
            SendOptions::new(Source::User),
        )
        .await;
    assert_eq!(execution.outcome, Outcome::Ok, "{:?}", execution.error);
    let otp = body_json(&execution)["x-otp"].as_str().unwrap().to_string();
    assert!(otp.len() == 6 && otp.chars().all(|c| c.is_ascii_digit()), "{otp}");
}

#[tokio::test]
async fn plugin_errors_fail_the_request_with_the_plugin_name() {
    let base = start_server().await;
    let (engine, host) = common::host();
    load(&host, &example("totp-generator"), &[]).await;
    let execution =
        engine.send(&request(json!({ "url": format!("{base}/json?code={{{{totp(\"!!\")}}}}") })), SendOptions::new(Source::User)).await;
    let error = execution.error.expect("an error");
    assert!(error.contains("totp-generator") && error.contains("not valid base32"), "{error}");
}

#[tokio::test]
async fn plugin_auth_is_applied() {
    let base = start_server().await;
    let (engine, host) = common::host();
    load(&host, &example("hmac-signature"), &[]).await;

    for name in ["hmac-signature/hmac", "hmac"] {
        let r = request(json!({
            "url": format!("{base}/echo-headers"),
            "auth": { "type": "plugin", "name": name, "config": { "keyId": "key-1", "secret": "{{secret:signing-key}}" } },
        }));
        let execution = engine.send(&r, SendOptions::new(Source::User)).await;
        assert_eq!(execution.outcome, Outcome::Ok, "{name}: {:?}", execution.error);
        let headers = body_json(&execution);
        assert_eq!(headers["x-key-id"], "key-1");
        assert_eq!(headers["x-signature"].as_str().unwrap().len(), 64, "{headers}");
    }

    let r = request(json!({ "url": format!("{base}/json"), "auth": { "type": "plugin", "name": "nobody/nothing" } }));
    let error = engine.send(&r, SendOptions::new(Source::User)).await.error.unwrap();
    assert!(error.contains("no installed plugin provides `nobody/nothing`"), "{error}");
}

#[tokio::test]
async fn hooks_run_before_and_after_each_request() {
    let base = start_server().await;
    let (engine, host) = common::host();
    load(&host, &example("request-id"), &[]).await;
    let dir = tempfile::tempdir().unwrap();
    let rewriter = write_plugin(
        dir.path(),
        "rewriter",
        &["hook"],
        &[],
        r#"import { definePlugin } from "@post-tester/plugin-sdk";
export default definePlugin({
  hooks: {
    afterResponse: (response) => {
      response.headers.set("X-Hooked", "yes");
      response.body = JSON.stringify({ wrapped: response.json() });
      return response;
    },
  },
});
"#,
    );
    load(&host, &rewriter, &[]).await;

    let r = request(json!({ "url": format!("{base}/echo-headers"), "tests": ["header(x-hooked) == yes", "json($.wrapped) exists"] }));
    let execution = engine.send(&r, SendOptions::new(Source::User)).await;
    assert_eq!(execution.outcome, Outcome::Ok, "{:?} {:?}", execution.error, execution.tests);
    let id = body_json(&execution)["wrapped"]["x-request-id"].as_str().unwrap().to_string();
    assert_eq!(id.len(), 36, "{id}");
    assert_eq!(execution.request.unwrap().header("X-Request-Id"), Some(id.as_str()));
}

#[tokio::test]
async fn importer_results_become_workspace_requests() {
    let (_engine, host) = common::host();
    load(&host, &example("har-importer"), &[]).await;
    assert!(host.import("not a HAR file").await.is_none());

    let har = json!({ "log": { "entries": [
        { "request": { "method": "GET", "url": "https://api.example.com/users?page=2", "headers": [{ "name": "Accept", "value": "application/json" }] } },
        { "request": { "method": "POST", "url": "https://api.example.com/users", "headers": [],
                       "postData": { "mimeType": "application/json", "text": "{\"name\":\"Ada\"}" } } },
    ] } });
    let imported = host.import(&har.to_string()).await.expect("detected as HAR").unwrap();
    assert_eq!(imported.name, "HAR import");
    let ids: Vec<&str> = imported.requests.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["api.example.com/get-users", "api.example.com/post-users"]);
    let (_, get) = &imported.requests[0];
    assert_eq!(get.url, "https://api.example.com/users");
    assert_eq!((get.query[0].key.as_str(), get.query[0].value.as_str()), ("page", "2"));
    assert_eq!(get.headers[0].key, "Accept");
    let (_, post) = &imported.requests[1];
    assert_eq!(post.body, post_tester_core::model::Body::Json { content: r#"{"name":"Ada"}"#.into() });
}

#[tokio::test]
async fn mcp_tools_are_named_for_agents() {
    let (engine, host) = common::host();
    load(&host, &example("jwt-tools"), &[]).await;
    let tools = host.mcp_tools();
    assert_eq!(tools.len(), 1);
    let (name, plugin, info) = &tools[0];
    assert_eq!(name, "jwt_tools__decode_jwt");
    assert_eq!(info.input_schema["required"], json!(["token"]));
    let token = "eyJhbGciOiJIUzI1NiJ9.eyJleHAiOjEwMH0.c2ln";
    let result = host.call_mcp_tool(plugin, &info.name, json!({ "token": token, "now": 160 }), &span(&engine)).await.unwrap();
    assert_eq!(result["expired"], true);
    assert_eq!(result["expiresInSeconds"], -60);
}

// ── Permissions and sandbox limits ──────────────────────────────────────────

#[tokio::test]
async fn http_needs_a_declared_and_approved_permission() {
    let base = start_server().await;
    let (_engine, host) = common::host();
    let dir = tempfile::tempdir().unwrap();
    let fetch = "const res = await ctx.http.get(input.url); return { status: res.status, token: res.json().token };";
    let input = json!({ "url": format!("{base}/json") });

    let undeclared = load(&host, &tool_plugin(dir.path(), "undeclared", &[], fetch), &[]).await;
    let error = call_tool(&undeclared, input.clone()).await.unwrap_err();
    assert!(
        error.contains("did not declare `network:127.0.0.1`") && error.contains(r#"Add "network:127.0.0.1" to "permissions""#),
        "{error}"
    );

    let unapproved = load(&host, &tool_plugin(dir.path(), "unapproved", &["network:127.0.0.1"], fetch), &[]).await;
    assert_eq!(unapproved.info().pending_permissions, ["network:127.0.0.1"]);
    let error = call_tool(&unapproved, input.clone()).await.unwrap_err();
    assert!(error.contains("was not approved") && error.contains("pt plugin approve unapproved"), "{error}");

    let approved = load(&host, &tool_plugin(dir.path(), "approved", &["network:127.0.0.1"], fetch), &["network:127.0.0.1"]).await;
    assert!(approved.info().pending_permissions.is_empty());
    assert_eq!(call_tool(&approved, input.clone()).await.unwrap(), json!({ "status": 200, "token": "abc" }));

    // A wildcard only covers sub-domains of the given domain.
    let wildcard = load(&host, &tool_plugin(dir.path(), "wildcard", &["network:*.example.com"], fetch), &["network:*.example.com"]).await;
    let error = call_tool(&wildcard, input).await.unwrap_err();
    assert!(error.contains("did not declare `network:127.0.0.1`"), "{error}");
}

#[tokio::test]
async fn invalid_network_permissions_are_reported() {
    let (_engine, host) = common::host();
    let dir = tempfile::tempdir().unwrap();
    let plugin = tool_plugin(dir.path(), "bad-permission", &["network:https://api.example.com"], "return 1;");
    let problems = host.validate(&plugin).await;
    assert_eq!(problems.len(), 1, "{}", show(&problems));
    assert_eq!(problems[0].location, "plugin.json: permissions");
    assert!(problems[0].message.contains("host name only"), "{}", problems[0]);
}

#[tokio::test]
async fn secrets_need_permission() {
    let (_engine, host) = common::host();
    let dir = tempfile::tempdir().unwrap();
    let read = "return await ctx.secrets.get(input.name) ?? null;";
    let denied = load(&host, &tool_plugin(dir.path(), "no-secrets", &[], read), &[]).await;
    let error = call_tool(&denied, json!({ "name": "signing-key" })).await.unwrap_err();
    assert!(error.contains("did not declare `secrets:read`"), "{error}");

    let allowed = load(&host, &tool_plugin(dir.path(), "secrets", &["secrets:read"], read), &["secrets:read"]).await;
    assert_eq!(call_tool(&allowed, json!({ "name": "signing-key" })).await.unwrap(), "s3cret");
    assert_eq!(call_tool(&allowed, json!({ "name": "missing" })).await.unwrap(), serde_json::Value::Null);
}

#[tokio::test]
async fn fs_stays_inside_the_workspace() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::init(root.path(), "Test").unwrap();
    let engine = Arc::new(Engine::new(Default::default(), Some(workspace), Arc::new(MemorySecrets::with(&[]))));
    let host = PluginHost::attach(&engine);
    let plugins = tempfile::tempdir().unwrap();
    let code = r#"
      if (input.write !== undefined) { ctx.fs.write(input.path, input.write); return "written"; }
      return ctx.fs.read(input.path);
    "#;
    let fs = load(&host, &tool_plugin(plugins.path(), "files", &["fs:read", "fs:write"], code), &["fs:read", "fs:write"]).await;

    assert_eq!(call_tool(&fs, json!({ "path": "notes/a.txt", "write": "hello" })).await.unwrap(), "written");
    assert_eq!(std::fs::read_to_string(root.path().join("notes/a.txt")).unwrap(), "hello");
    assert_eq!(call_tool(&fs, json!({ "path": "notes/a.txt" })).await.unwrap(), "hello");

    let outside = root.path().parent().unwrap().join("outside.txt");
    for path in ["../outside.txt", "notes/../../outside.txt", outside.to_str().unwrap()] {
        let error = call_tool(&fs, json!({ "path": path, "write": "x" })).await.unwrap_err();
        assert!(error.contains("is outside the workspace"), "{path}: {error}");
        let error = call_tool(&fs, json!({ "path": path })).await.unwrap_err();
        assert!(error.contains("is outside the workspace"), "{path}: {error}");
    }
    assert!(!outside.exists());

    // A symbolic link inside the workspace must not lead outside it.
    #[cfg(unix)]
    {
        let elsewhere = tempfile::tempdir().unwrap();
        std::fs::write(elsewhere.path().join("secret.txt"), "private").unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), root.path().join("link")).unwrap();
        let error = call_tool(&fs, json!({ "path": "link/secret.txt" })).await.unwrap_err();
        assert!(error.contains("is outside the workspace"), "{error}");
        let error = call_tool(&fs, json!({ "path": "link/new.txt", "write": "x" })).await.unwrap_err();
        assert!(error.contains("is outside the workspace"), "{error}");
        assert!(!elsewhere.path().join("new.txt").exists());
    }

    let no_permission = load(&host, &tool_plugin(plugins.path(), "no-files", &[], code), &[]).await;
    let error = call_tool(&no_permission, json!({ "path": "notes/a.txt" })).await.unwrap_err();
    assert!(error.contains("did not declare `fs:read`"), "{error}");
}

#[tokio::test]
async fn runaway_code_is_stopped_by_the_time_limit() {
    let engine = common::engine();
    let host = PluginHost::attach_with_timeout(&engine, Duration::from_millis(300));
    let dir = tempfile::tempdir().unwrap();
    let plugin = write_plugin(
        dir.path(),
        "spinner",
        &["template-function"],
        &[],
        r#"import { definePlugin } from "@post-tester/plugin-sdk";
export default definePlugin({
  templateFunctions: {
    spin: { description: "Never returns.", run: () => { while (true) {} } },
    wait: { description: "Waits forever.", run: () => new Promise(() => {}) },
    ok: { description: "Returns ok.", run: () => "ok" },
  },
});
"#,
    );
    let plugin = load(&host, &plugin, &[]).await;
    for name in ["spin", "wait"] {
        let started = std::time::Instant::now();
        let error = plugin.call(None, "templateFunction", name, json!({ "args": {} })).await.unwrap_err().to_string();
        assert!(error.contains("took longer than 300 ms and was stopped"), "{name}: {error}");
        assert!(started.elapsed() < Duration::from_secs(5), "{name} took {:?}", started.elapsed());
    }
    // The sandbox is still usable afterwards.
    assert_eq!(plugin.call(None, "templateFunction", "ok", json!({ "args": {} })).await.unwrap(), "ok");
}

// ── Actionable problems for invalid plugins ─────────────────────────────────

#[tokio::test]
async fn invalid_plugins_produce_actionable_problems() {
    let (_engine, host) = common::host();
    let dir = tempfile::tempdir().unwrap();
    let valid_code = r#"import { definePlugin } from "@post-tester/plugin-sdk";
export default definePlugin({ hooks: { beforeRequest: (r) => r } });
"#;
    let problems_for = async |dir: &Path| {
        let problems = host.validate(dir).await;
        assert!(!problems.is_empty(), "{} should not validate", dir.display());
        assert!(problems.iter().all(|p| !p.fix.is_empty()), "{}", show(&problems));
        problems
    };

    // Bad manifest: every field problem is reported at once.
    let bad = write_plugin(dir.path(), "bad-manifest", &["hook"], &["everything"], valid_code);
    let mut manifest: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(bad.join("plugin.json")).unwrap()).unwrap();
    manifest["name"] = "Bad Name".into();
    manifest["version"] = "1".into();
    manifest["provides"] = json!(["widget"]);
    std::fs::write(bad.join("plugin.json"), manifest.to_string()).unwrap();
    let problems = problems_for(&bad).await;
    let locations: Vec<&str> = problems.iter().map(|p| p.location.as_str()).collect();
    assert_eq!(
        locations,
        ["plugin.json: name", "plugin.json: version", "plugin.json: provides", "plugin.json: permissions"],
        "{}",
        show(&problems)
    );

    // Not JSON / unknown field.
    let broken = write_plugin(dir.path(), "broken-json", &["hook"], &[], valid_code);
    std::fs::write(broken.join("plugin.json"), "{ \"name\": \"x\",\n  \"nmae\": 1 }").unwrap();
    let problems = problems_for(&broken).await;
    assert!(problems[0].location.starts_with("plugin.json:2:"), "{}", show(&problems));
    assert!(problems[0].message.contains("unknown field `nmae`"), "{}", show(&problems));

    // `provides` and definePlugin disagree.
    let mismatch = write_plugin(
        dir.path(),
        "mismatch",
        &["hook"],
        &[],
        r#"import { definePlugin } from "@post-tester/plugin-sdk";
export default definePlugin({ templateFunctions: { a: { description: "A.", run: () => "a" } } });
"#,
    );
    let problems = problems_for(&mismatch).await;
    assert_eq!(problems.len(), 2, "{}", show(&problems));
    let has = |text: &str| problems.iter().any(|p| p.message.contains(text));
    assert!(has(r#"lists "hook" in provides, but definePlugin has no `hooks`"#), "{}", show(&problems));
    assert!(has(r#"definePlugin has `templateFunctions`, but provides does not list "template-function""#), "{}", show(&problems));

    // No default export.
    let no_default = write_plugin(dir.path(), "no-default", &["hook"], &[], "export const hooks = {};\n");
    let problems = problems_for(&no_default).await;
    assert!(problems[0].message.contains("must `export default definePlugin({ ... })`"), "{}", show(&problems));

    // TypeScript syntax error, with file:line:column.
    let syntax = write_plugin(
        dir.path(),
        "syntax",
        &["hook"],
        &[],
        "import { definePlugin } from \"@post-tester/plugin-sdk\";\nexport default definePlugin({ hooks: { beforeRequest: (r => r } });\n",
    );
    let problems = problems_for(&syntax).await;
    assert!(problems[0].message.contains("index.ts:2:") && problems[0].message.contains("syntax error"), "{}", show(&problems));

    // JavaScript syntax error, also with the file and line.
    let js = write_plugin(dir.path(), "syntax-js", &["hook"], &[], "");
    std::fs::rename(js.join("index.ts"), js.join("index.js")).unwrap();
    let mut manifest: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(js.join("plugin.json")).unwrap()).unwrap();
    manifest["main"] = "index.js".into();
    std::fs::write(js.join("plugin.json"), manifest.to_string()).unwrap();
    std::fs::write(js.join("index.js"), "export default {\n  hooks: { beforeRequest(r) { return r; }\n};\n").unwrap();
    let problems = problems_for(&js).await;
    assert!(problems[0].message.contains("index.js:"), "{}", show(&problems));

    // Unknown definePlugin key, misspelled hook, missing descriptions.
    let typo = write_plugin(
        dir.path(),
        "typo",
        &["hook"],
        &[],
        "import { definePlugin } from \"@post-tester/plugin-sdk\";\nexport default definePlugin({ hook: {} });\n",
    );
    let problems = problems_for(&typo).await;
    assert!(problems[0].message.contains(r#"unknown key "hook""#), "{}", show(&problems));

    let hook_typo = write_plugin(dir.path(), "hook-typo", &["hook"], &[], "export default { hooks: { beforeRequset: (r) => r } };\n");
    let problems = problems_for(&hook_typo).await;
    assert!(
        problems[0].message.contains("unknown hook `beforeRequset`") && problems[0].fix.contains("beforeRequest"),
        "{}",
        show(&problems)
    );

    let undocumented =
        write_plugin(dir.path(), "undocumented", &["mcp-tool"], &[], "export default { mcpTools: { t: { run: () => 1 } } };\n");
    let problems = problems_for(&undocumented).await;
    assert!(problems[0].message.contains("MCP tool `t` has no description"), "{}", show(&problems));

    // npm imports must be bundled.
    let npm = write_plugin(
        dir.path(),
        "npm-import",
        &["hook"],
        &[],
        "import _ from \"lodash\";\nexport default { hooks: { beforeRequest: (r) => _.identity(r) } };\n",
    );
    let problems = problems_for(&npm).await;
    assert!(problems[0].message.starts_with("Error resolving module 'lodash'"), "{}", show(&problems));
    assert!(problems[0].message.contains("bundle npm packages"), "{}", show(&problems));

    // Missing folder.
    let problems = problems_for(&dir.path().join("does-not-exist")).await;
    assert!(problems[0].message.contains("cannot open the plugin folder"), "{}", show(&problems));
}

#[tokio::test]
async fn plugins_load_from_relative_folders() {
    let (_engine, host) = common::host();
    // Tests run in the crate folder; create the plugin below it and refer to it
    // with a plain relative path (no `./`), as `pt plugin install plugins/x` does.
    let parent = tempfile::Builder::new().prefix("relative-plugin-").tempdir_in(".").unwrap();
    let name = parent.path().file_name().unwrap().to_string_lossy().into_owned();
    post_tester_plugin_host::scaffold::create(&parent.path().join("shout"), "template-function", "shout").unwrap();
    let relative = Path::new(&name).join("shout");
    assert!(relative.is_relative());

    assert!(host.validate(&relative).await.is_empty(), "{}", show(&host.validate(&relative).await));
    let report = host.test(&relative).await.unwrap_or_else(|p| panic!("{}", show(&p)));
    assert!(report.passed(), "{}", common::failures(&report.tests));
    let plugin = load(&host, &relative, &[]).await;
    assert!(plugin.dir.is_absolute());
}

#[tokio::test]
async fn failing_plugin_tests_are_reported() {
    let (_engine, host) = common::host();
    let dir = tempfile::tempdir().unwrap();
    let plugin = write_plugin(
        dir.path(),
        "failing",
        &["template-function"],
        &[],
        "export default { templateFunctions: { a: { description: \"A.\", run: () => \"a\" } } };\n",
    );
    std::fs::write(
        plugin.join("index.test.ts"),
        r#"import { testPlugin } from "@post-tester/plugin-sdk/testing";
import plugin from "./index.ts";
test("passes", async () => expect(await testPlugin(plugin).templateFunction("a")).toBe("a"));
test("fails", async () => expect(await testPlugin(plugin).templateFunction("a")).toBe("b"));
test("unknown function", async () => { await testPlugin(plugin).templateFunction("nope"); });
"#,
    )
    .unwrap();
    let report = host.test(&plugin).await.unwrap();
    assert!(!report.passed());
    let outcomes: Vec<(&str, bool)> = report.tests.iter().map(|t| (t.name.as_str(), t.passed)).collect();
    assert_eq!(outcomes, [("passes", true), ("fails", false), ("unknown function", false)]);
    assert_eq!(report.tests[1].detail.as_deref(), Some(r#"expected "a" to be "b""#));
    assert!(report.tests[2].detail.as_deref().unwrap().contains(r#"no template function named "nope" (available: a)"#));

    std::fs::remove_file(plugin.join("index.test.ts")).unwrap();
    let problems = host.test(&plugin).await.unwrap_err();
    assert!(problems[0].message.contains("no test files found"), "{}", show(&problems));
}

// ── Request scripts ─────────────────────────────────────────────────────────

#[tokio::test]
async fn after_response_scripts_test_and_set_variables() {
    let base = start_server().await;
    let (engine, _host) = common::host();
    let script = r#"
pt.test("pt: status is 200", () => pt.expect(pt.response.status).toBe(200));
pt.variables.set("userId", pt.response.json().user.id);
pm.test("pm: token is abc", () => {
  pm.expect(pm.response.json().token).to.equal("abc");
  pm.response.to.have.status(200);
  pm.expect(pm.response.json().user).to.have.property("id", 7);
});
pm.test("pm: a failing assertion", () => pm.expect(1).to.equal(2));
pm.environment.set("token", pm.response.json().token);
"#;
    let mut first = request(json!({ "url": format!("{base}/json") }));
    first.scripts.after_response = script.into();
    let execution = engine.send(&first, SendOptions::new(Source::User)).await;
    let tests: Vec<(&str, bool)> = execution.tests.iter().map(|t| (t.name.as_str(), t.passed)).collect();
    assert_eq!(tests, [("pt: status is 200", true), ("pm: token is abc", true), ("pm: a failing assertion", false)]);
    assert_eq!(execution.tests[2].detail.as_deref(), Some("expected 1 to equal 2"));
    assert_eq!(execution.outcome, Outcome::Failed);

    // Variables set by the script are visible to the next request.
    assert_eq!(engine.runtime_variables()["token"], "abc");
    assert_eq!(engine.runtime_variables()["userId"], "7");
    let second =
        request(json!({ "url": format!("{base}/echo-headers"), "headers": [{ "key": "X-Token", "value": "{{token}}-{{userId}}" }] }));
    let execution = engine.send(&second, SendOptions::new(Source::User)).await;
    assert_eq!(body_json(&execution)["x-token"], "abc-7");
}

#[tokio::test]
async fn before_request_scripts_set_variables_for_the_request() {
    let base = start_server().await;
    let (engine, _host) = common::host();
    let mut r = request(json!({ "url": format!("{base}/echo-headers"), "headers": [{ "key": "X-Run", "value": "{{run}}" }] }));
    r.scripts.before_request = r#"pm.variables.set("run", "first-" + pm.info.eventName);"#.into();
    let execution = engine.send(&r, SendOptions::new(Source::User)).await;
    assert_eq!(execution.outcome, Outcome::Ok, "{:?}", execution.error);
    assert_eq!(body_json(&execution)["x-run"], "first-prerequest");
}

#[tokio::test]
async fn before_request_scripts_can_change_headers() {
    let (engine, host) = common::host();
    let input = ScriptInput {
        request: ScriptRequest {
            method: "GET".into(),
            url: "https://example.com/".into(),
            headers: vec![("X-Old".into(), "1".into())],
            body: String::new(),
        },
        response: None,
        variables: vec![],
        environment: None,
    };
    let code = r#"
pt.request.headers.set("X-Pt", "1");
pm.request.headers.add({ key: "X-Pm", value: "2" });
pm.request.headers.remove("x-old");
"#;
    let output = host.run_script(ScriptPhase::BeforeRequest, code, input, &span(&engine)).await.unwrap();
    let headers = output.request.expect("the changed request").headers;
    assert_eq!(headers, [("X-Pt".to_string(), "1".to_string()), ("X-Pm".to_string(), "2".to_string())]);
}

/// Header changes made by a before-request script reach the server.
#[tokio::test]
async fn before_request_script_header_changes_are_sent() {
    let base = start_server().await;
    let (engine, _host) = common::host();
    let mut r = request(json!({ "url": format!("{base}/echo-headers") }));
    r.scripts.before_request = r#"pm.request.headers.add({ key: "X-From-Script", value: "yes" });"#.into();
    let execution = engine.send(&r, SendOptions::new(Source::User)).await;
    assert_eq!(body_json(&execution)["x-from-script"], "yes");
}

#[tokio::test]
async fn script_errors_are_reported_clearly() {
    let base = start_server().await;
    let (engine, _host) = common::host();

    let mut r = request(json!({ "url": format!("{base}/json") }));
    r.scripts.after_response = "const data = pm.response.json();\nthrow new Error(`unexpected user ${data.user.id}`);".into();
    let execution = engine.send(&r, SendOptions::new(Source::User)).await;
    assert_eq!(execution.outcome, Outcome::Error);
    let error = execution.error.unwrap();
    assert!(error.ends_with("script error: unexpected user 7"), "{error}");

    let mut r = request(json!({ "url": format!("{base}/json") }));
    r.scripts.before_request = "notDefined.call();".into();
    let execution = engine.send(&r, SendOptions::new(Source::User)).await;
    let error = execution.error.unwrap();
    assert!(error.contains("script error") && error.contains("notDefined"), "{error}");
    assert!(execution.response.is_none(), "the request must not be sent after a before-request script fails");

    let mut r = request(json!({ "url": format!("{base}/status/500") }));
    r.scripts.after_response = "pm.response.to.have.status(200);".into();
    let error = engine.send(&r, SendOptions::new(Source::User)).await.error.unwrap();
    assert!(error.contains("expected response to have status 200, got 500"), "{error}");

    let mut r = request(json!({ "url": format!("{base}/json") }));
    r.scripts.after_response = "pm.test('broken syntax', () => {".into();
    let error = engine.send(&r, SendOptions::new(Source::User)).await.error.unwrap();
    assert!(error.contains("script error: syntax error in the script"), "{error}");
    assert!(!error.contains("eval_script"), "internal frames must not leak: {error}");
}
