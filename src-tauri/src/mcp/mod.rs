#![allow(dead_code, unused_imports)] // wired into Tauri runtime in Step 4.

pub mod handler;
pub mod server;
pub mod tools;
pub mod transport;

pub use handler::McpHandler;
