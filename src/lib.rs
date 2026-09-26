/// Application.
pub mod app;

/// Terminal events handler.
#[cfg(not(target_arch = "wasm32"))]
pub mod event;

/// Widget renderer.
pub mod ui;

/// Terminal user interface.
#[cfg(not(target_arch = "wasm32"))]
pub mod tui;

/// Event handler.
#[cfg(not(target_arch = "wasm32"))]
pub mod handler;

/// IRA business logic
pub mod services;

/// IRA Components
pub mod components;

/// IRA Domain types and structs
pub mod domain;

/// Semantic theme, terminal caps, and file-type icons.
pub mod theme;

pub mod utils;

/// Monotonic clock backed by `performance.now()` in browsers.
pub mod clock {
    #[cfg(not(target_arch = "wasm32"))]
    pub use std::time::{Instant, SystemTime, UNIX_EPOCH};
    #[cfg(target_arch = "wasm32")]
    pub use web_time::{Instant, SystemTime, UNIX_EPOCH};
}
