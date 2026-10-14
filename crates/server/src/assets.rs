//! The web UI, embedded in the binary at build time from `apps/web/dist`.

use axum::body::Body;
use axum::http::{StatusCode, Uri, header};
use axum::response::{Html, IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../apps/web/dist"]
#[allow_missing = true]
struct Ui;

/// Serves a UI file, falling back to `index.html` so client-side routes work.
pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path == "api" || path.starts_with("api/") {
        return crate::error::ApiError::not_found(format!("no API endpoint `/{path}`; see API.md for the list")).into_response();
    }
    let path = if path.is_empty() { "index.html" } else { path };
    if let Some(file) = Ui::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        let cache = if path == "index.html" { "no-cache" } else { "public, max-age=31536000, immutable" };
        return ([(header::CONTENT_TYPE, mime.as_ref().to_string()), (header::CACHE_CONTROL, cache.to_string())], Body::from(file.data))
            .into_response();
    }
    match Ui::get("index.html") {
        Some(index) => Html(index.data).into_response(),
        None => (StatusCode::OK, Html(NOT_BUILT)).into_response(),
    }
}

const NOT_BUILT: &str = r#"<!doctype html><meta charset="utf-8"><title>post-tester</title>
<body style="font-family: system-ui; max-width: 40rem; margin: 4rem auto; line-height: 1.5">
<h1>The web UI is not built</h1>
<p>This binary was compiled without the web UI. Build it, then rebuild post-tester:</p>
<pre>pnpm install
pnpm --filter web build
cargo build --release</pre>
<p>The API and the MCP endpoint work without it.</p>
</body>"#;
