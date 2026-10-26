/// Application.
pub mod app;

/// Terminal events handler.
pub mod event;

/// Widget renderer.
pub mod ui;

/// Terminal user interface.
pub mod tui;

/// Event handler.
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

// Hooked exit fixtures use the natural library App/component type graph and
// the same private helper source as production main.
#[cfg(test)]
mod terminal_exit;
#[cfg(test)]
use app::AppResult;
#[cfg(test)]
use std::io;
#[cfg(test)]
use terminal_exit::{finish_exit, ExitApp};
#[cfg(test)]
#[path = "main_exit_settlement_tests.rs"]
mod main_exit_settlement_tests;
