//! One module per group of commands.
//!
//! Exit codes: `0` success, `1` tests failed or the request failed, `2` usage or setup error.

pub mod plugin;
pub mod request;
pub mod run;
pub mod serve;
pub mod workspace;
