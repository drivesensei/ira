//! Exercise the actual TUI transfer source through the same pinned core publication API.
#[allow(dead_code)] // This source-inclusion probe compiles only transfer behavior.
#[path = "../../../src/services/transfer.rs"]
mod terminal_transfer;
