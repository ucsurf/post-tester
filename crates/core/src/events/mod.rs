//! The live event system.
//!
//! Everything that happens in post-tester is reported as an [`Event`] on the
//! [`EventBus`]. Front-ends (web UI, desktop app, CLI, MCP) are live views of
//! this one stream.

mod blob;
mod bus;
mod event;
mod span;

pub use blob::{Blob, BlobStore, PREVIEW_BYTES, preview_text};
pub use bus::{BusConfig, EventBus, EventSink};
pub use event::*;
pub use span::{Span, millis};
