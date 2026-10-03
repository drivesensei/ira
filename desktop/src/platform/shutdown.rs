//! Checked receipt ownership retained across time budgets and actor/window lifetimes.
use super::geometry::{Checked, Failure, Writer};
use crate::runtime::{Runtime, ShutdownState};
use std::{
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Waiting,
    PendingError(String),
    Ready,
}
pub struct Coordinator {
    receipt: mpsc::Receiver<Checked>,
    geometry: Option<Result<u64, Arc<Failure>>>,
    disconnected: bool,
    started: Instant,
}
impl Coordinator {
    pub fn new(writer: &Writer) -> Self {
        Self {
            receipt: writer.final_receipt(),
            geometry: None,
            disconnected: false,
            started: Instant::now(),
        }
    }
    pub fn poll(&mut self, runtime: &Runtime) -> Status {
        if self.geometry.is_none() && !self.disconnected {
            match self.receipt.try_recv() {
                Ok(Ok(receipt)) => self.geometry = Some(Ok(receipt.epoch)),
                Ok(Err(error)) => self.geometry = Some(Err(Arc::new(error))),
                Err(mpsc::TryRecvError::Disconnected) => self.disconnected = true,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(Err(error)) = &self.geometry {
            return Status::PendingError(format!("Shutdown paused: {}", error.message));
        }
        if self.disconnected {
            return Status::PendingError("Shutdown paused: geometry receipt disconnected".into());
        }
        match runtime.shutdown_state() {
            Some(ShutdownState::Failure(error)) => Status::PendingError(format!(
                "Shutdown paused: {}",
                error
                    .errors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; ")
            )),
            Some(ShutdownState::Disconnected) => {
                Status::PendingError("Shutdown paused: persistence receipt disconnected".into())
            }
            Some(ShutdownState::Success { .. }) if matches!(self.geometry, Some(Ok(_))) => {
                Status::Ready
            }
            _ if self.started.elapsed() >= Duration::from_secs(2) => Status::PendingError(
                "Shutdown paused: checked persistence or geometry receipt not confirmed".into(),
            ),
            _ => Status::Waiting,
        }
    }
    pub fn retry(&mut self, runtime: &Runtime) {
        if let Some(Err(error)) = &self.geometry {
            self.receipt = error.retry.retry();
            self.geometry = None;
            self.disconnected = false;
        }
        runtime.retry_shutdown();
        // Keep the original pending receiver; a time budget is not a new snapshot.
    }
}

#[cfg(test)]
#[path = "shutdown_tests.rs"]
mod tests;
