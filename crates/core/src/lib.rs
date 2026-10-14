//! # post-tester core
//!
//! The engine behind every post-tester front-end (web UI, desktop app, CLI, MCP).
//!
//! | Module         | Responsibility                                               |
//! |----------------|--------------------------------------------------------------|
//! | [`events`]     | The live event bus: everything that happens is an event      |
//! | [`model`]      | Requests, environments and settings, as stored in YAML       |
//! | [`workspace`]  | Reading and writing a workspace folder                       |
//! | [`template`]   | `{{variables}}`, `{{secret:x}}`, `{{$uuid}}`, `{{fn()}}`     |
//! | [`prepare`]    | Turning a saved request into exactly what will be sent       |
//! | [`http`]       | Sending HTTP requests with per-phase timing                  |
//! | [`expr`]       | Tests (`status == 200`) and extraction (`json($.token)`)     |
//! | [`engine`]     | The full request lifecycle                                   |
//! | [`runner`]     | Running folders and collections                              |
//! | [`loadtest`]   | Sending a request many times concurrently                    |
//! | [`ws`]         | WebSocket sessions                                           |
//! | [`extensions`] | The interface plugins implement                              |
//! | [`secrets`]    | Where secret values come from                                |
//! | [`import`]     | Importing cURL, Postman and OpenAPI                          |
//! | [`codegen`]    | Generating code (cURL, JavaScript, Python) from requests     |

pub mod codegen;
pub mod engine;
pub mod error;
pub mod events;
pub mod expr;
pub mod extensions;
pub mod http;
pub mod import;
pub mod loadtest;
pub mod model;
pub mod prepare;
pub mod runner;
pub mod secrets;
pub mod template;
pub mod workspace;
pub mod ws;

pub use engine::{Engine, Execution, SendOptions};
pub use error::{Error, Result};
pub use workspace::Workspace;

/// The post-tester version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
