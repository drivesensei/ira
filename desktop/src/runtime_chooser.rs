use super::*;
use crate::platform::chooser::{ChooserKind, ChooserOutcome, ChooserTicket};
use ira_core::application::{ExistingPathKind, ExistingPathScope};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdmittedChooser {
    pub ticket: ChooserTicket,
    pub context: ExistingPathScope,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChooserFocusPermit {
    pub window: u64,
    pub document: u64,
    pub focus: u64,
    pub pane: usize,
    pub context: ira_core::input::InputContext,
    pub listing: u64,
    pub folder: Option<String>,
}
impl ChooserFocusPermit {
    pub(super) fn capture(app: &App) -> Self {
        Self {
            window: app.window_generation,
            document: app.document_generation,
            focus: app.focus_generation,
            pane: app.active_pane,
            context: app.input_context(),
            listing: app.panes[app.active_pane].listing_generation,
            folder: app.panes[app.active_pane]
                .folder
                .as_ref()
                .map(|f| f.path.clone()),
        }
    }
}
pub enum ChooserEvent {
    Prompt(AdmittedChooser),
    RestoreFocus(ChooserFocusPermit),
}
#[derive(Default)]
pub(super) struct ActorChooser {
    next: u64,
    pending: Option<AdmittedChooser>,
    loading: bool,
    invalid: bool,
}
impl ActorChooser {
    pub fn apply(
        &mut self,
        app: &mut App,
        envelope: Envelope,
        events: &mpsc::Sender<ChooserEvent>,
    ) -> Result<(), String> {
        if envelope.window_generation != app.window_generation
            || envelope.sequence <= app.ack_sequence
        {
            return Err("Chooser/command belongs to an expired window or sequence".into());
        }
        match envelope.command {
            Command::Browse(kind) => {
                if self.pending.is_some() {
                    return Err("Native chooser is already pending".into());
                }
                let context = app
                    .existing_path_scope(app.active_pane)
                    .ok_or("Chooser requires a settled active file pane")?;
                self.next = self
                    .next
                    .checked_add(1)
                    .ok_or("Chooser request identity exhausted")?;
                let admitted = AdmittedChooser {
                    ticket: ChooserTicket {
                        request_id: self.next,
                        kind,
                    },
                    context,
                };
                self.pending = Some(admitted.clone());
                self.loading = false;
                self.invalid = false;
                let _ = events.send(ChooserEvent::Prompt(admitted));
                app.ack_sequence = envelope.sequence;
                Ok(())
            }
            Command::ChooserResult { ticket, outcome } => {
                let Some(admitted) = self
                    .pending
                    .as_ref()
                    .filter(|a| a.ticket == ticket)
                    .cloned()
                else {
                    return Ok(());
                };
                if self.loading {
                    return Ok(());
                }
                if self.invalid || !app.existing_path_scope_is_current(&admitted.context) {
                    self.pending = None;
                    app.ack_sequence = envelope.sequence;
                    return Ok(());
                }
                match outcome.as_ref() {
                    ChooserOutcome::Selected(path) => {
                        match app.request_existing_path(
                            admitted.context.pane,
                            path.clone(),
                            match ticket.kind {
                                ChooserKind::File => ExistingPathKind::File,
                                ChooserKind::Folder => ExistingPathKind::Folder,
                            },
                            admitted.context.clone(),
                            ticket.request_id,
                        ) {
                            Ok(()) => self.loading = true,
                            Err(error) => {
                                app.set_status(error, true);
                                app.focus_generation = app.focus_generation.wrapping_add(1);
                                self.pending = None;
                                restore(app, &admitted, events);
                            }
                        }
                    }
                    ChooserOutcome::Canceled => {
                        self.pending = None;
                        restore(app, &admitted, events);
                    }
                    ChooserOutcome::Error(error) => {
                        app.set_status(error.clone(), true);
                        app.focus_generation = app.focus_generation.wrapping_add(1);
                        self.pending = None;
                        restore(app, &admitted, events);
                    }
                }
                app.ack_sequence = envelope.sequence;
                Ok(())
            }
            command => {
                let shutdown = matches!(&command,Command::Input(Input::Key(key)) if
                    (key.modifiers==KeyModifiers::CONTROL && matches!(key.code,KeyCode::Char('c'|'C')))
                    || (key.modifiers==KeyModifiers::NONE && key.code==KeyCode::Char('q') && matches!(app.input_context(),ira_core::input::InputContext::Pane(_))));
                if shutdown {
                    return apply(
                        app,
                        Envelope {
                            command,
                            ..envelope
                        },
                    );
                }
                if self.pending.is_some() {
                    // Shutdown is out-of-band; ordinary queued interactions cannot mutate state.
                    // Remember attempted focus/context transitions even if input is blocked.
                    if matches!(command, Command::FocusPane(_) | Command::SetFocus(_)) {
                        self.invalid = true;
                        app.invalidate_existing_path_requests();
                    }
                    return Err("Native chooser owns input; shutdown remains available".into());
                }
                apply(
                    app,
                    Envelope {
                        command,
                        ..envelope
                    },
                )
            }
        }
    }
    pub fn permit(&self, app: &App) -> Option<AdmittedChooser> {
        self.pending
            .as_ref()
            .filter(|a| {
                !self.invalid && !self.loading && app.existing_path_scope_is_current(&a.context)
            })
            .cloned()
    }
    pub fn drain(&mut self, app: &mut App, events: &mpsc::Sender<ChooserEvent>) {
        for receipt in app.take_existing_path_receipts() {
            let Some(admitted) = self
                .pending
                .as_ref()
                .filter(|a| a.ticket.request_id == receipt.request_id && a.context == receipt.scope)
                .cloned()
            else {
                continue;
            };
            self.pending = None;
            self.loading = false;
            if !self.invalid {
                restore(app, &admitted, events);
            }
        }
        if self.loading
            && self
                .pending
                .as_ref()
                .is_some_and(|a| !app.existing_path_scope_is_current(&a.context))
        {
            // Successful navigation changes listing/folder, and emitted a receipt above.
            // A superseding scope clears only actor bookkeeping; physical core admission persists.
            self.pending = None;
            self.loading = false;
        }
    }
}
fn restore(app: &App, _admitted: &AdmittedChooser, events: &mpsc::Sender<ChooserEvent>) {
    let _ = events.send(ChooserEvent::RestoreFocus(ChooserFocusPermit::capture(app)));
}
#[cfg(test)]
#[path = "runtime_chooser_tests.rs"]
mod tests;
