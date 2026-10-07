pub mod actions;
pub mod components;
pub mod focus;
pub mod keymap;
pub mod platform;
pub mod runtime;
pub mod views;

/// Opt-in, text-free lifecycle diagnostics for native window investigations.
/// Never records paths, drafts, selections, or clipboard contents.
pub fn lifecycle_trace(event: &str) {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *ENABLED.get_or_init(|| std::env::var_os("IRA_RUNTIME_TRACE").is_some()) {
        eprintln!("IRA lifecycle {:?}: {event}", std::time::SystemTime::now());
    }
}

#[cfg(test)]
pub(crate) mod test_support;
