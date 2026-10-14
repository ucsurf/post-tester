//! # post-tester MCP
//!
//! The [Model Context Protocol](https://modelcontextprotocol.io) in both directions:
//!
//! | Module     | Direction                                                      |
//! |------------|----------------------------------------------------------------|
//! | [`server`] | post-tester as an MCP server: AI agents use the workspace      |
//! | [`client`] | post-tester as an MCP client: test any MCP server like an API  |
//!
//! Both report everything live on the engine's event bus: agent actions are
//! spans with `Source::Agent`, and every MCP message is a `Message` event.

pub mod client;
pub mod server;

pub use client::{ConnectOptions, McpClients, SessionInfo, Transport};
pub use server::{McpServer, ServerOptions, http_service, serve_stdio};
