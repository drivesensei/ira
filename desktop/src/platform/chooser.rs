//! Value-only native chooser support. The actor supplies ticket authority; the
//! session owns the lease until its retained receiver settles.
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChooserKind {
    File,
    Folder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChooserTicket {
    pub request_id: u64,
    pub kind: ChooserKind,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ChooserOutcome {
    Selected(PathBuf),
    /// The API returned no selection; this does not establish a native reason.
    Canceled,
    Error(String),
}

pub fn options(kind: ChooserKind) -> gpui::PathPromptOptions {
    gpui::PathPromptOptions {
        files: kind == ChooserKind::File,
        directories: kind == ChooserKind::Folder,
        multiple: false,
        prompt: Some("Select".into()),
    }
}

/// Normalize the adapter's receiver and platform results without filesystem
/// access or changing the selected native spelling. The adapter retains error
/// classification when converting its actual error objects to strings.
pub fn normalize_received(
    received: Result<Result<Option<Vec<PathBuf>>, String>, String>,
) -> ChooserOutcome {
    let paths = match received {
        Err(error) => return ChooserOutcome::Error(format!("Chooser receiver failed: {error}")),
        Ok(Err(error)) => return ChooserOutcome::Error(format!("Chooser prompt failed: {error}")),
        Ok(Ok(None)) => return ChooserOutcome::Canceled,
        Ok(Ok(Some(paths))) => paths,
    };
    if paths.len() != 1 {
        return ChooserOutcome::Error("Chooser must return exactly one selected path".into());
    }
    // Cardinality was checked above; moving the sole value preserves OsString.
    let Some(path) = paths.into_iter().next() else {
        return ChooserOutcome::Error("Chooser returned no selected path".into());
    };
    if path.as_os_str().is_empty() || !path.is_absolute() {
        return ChooserOutcome::Error("Chooser selected path must be nonempty and absolute".into());
    }
    ChooserOutcome::Selected(path)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Busy;

/// Bookkeeping only: no lifecycle gate, window ownership or request-id minting.
#[derive(Debug, Default)]
pub struct SingleFlight {
    active: Option<ChooserTicket>,
}

impl SingleFlight {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn try_acquire(&mut self, ticket: ChooserTicket) -> Result<(), Busy> {
        if self.active.is_some() {
            return Err(Busy);
        }
        self.active = Some(ticket);
        Ok(())
    }

    pub fn active(&self) -> Option<ChooserTicket> {
        self.active
    }

    pub fn release(&mut self, ticket: ChooserTicket) -> bool {
        if self.active != Some(ticket) {
            return false;
        }
        self.active = None;
        true
    }
}

#[cfg(test)]
#[path = "chooser_tests.rs"]
mod tests;
