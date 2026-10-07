//! App-owned main-thread native retirement. Pure pins survive every bounded retry.
use super::accessibility_bridge::NativeBridge;
use super::accessibility_worker::Worker;
use crate::platform::accessibility::ActionSink;
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};

pub type Pins = Vec<Box<dyn Send>>;
struct Closing {
    bridge: NativeBridge,
    sink: ActionSink,
    worker: Worker,
    pins: Pins,
    detached: bool,
}
#[derive(Clone, Default)]
pub struct Retirement(Rc<RefCell<VecDeque<Closing>>>, Rc<Cell<bool>>);
impl gpui::Global for Retirement {}
impl Retirement {
    /// Reopen is deferred at this finite capacity; existing ownership is never discarded.
    pub fn can_open(&self) -> bool {
        !self.1.get() && self.0.borrow().len() < 4 && Worker::has_capacity()
    }
    pub fn begin_quit(&self) {
        self.1.set(true);
    }
    pub fn quit_committed(&self) -> bool {
        self.1.get()
    }
    pub fn is_empty(&self) -> bool {
        self.0.borrow().is_empty() && Worker::reclamation_complete()
    }
    pub fn close(&self, bridge: NativeBridge, sink: ActionSink, worker: Worker, pins: Pins) {
        sink.close();
        worker.cancel();
        self.0.borrow_mut().push_back(Closing {
            bridge,
            sink,
            worker,
            pins,
            detached: false,
        });
    }
    /// One GLOBAL budget, shared by every closing window in this UI turn.
    pub fn pump(&self, budget: usize) -> usize {
        let mut remaining = budget;
        let mut queue = self.0.borrow_mut();
        let turns = queue.len();
        for _ in 0..turns {
            let Some(mut closing) = queue.pop_front() else {
                break;
            };
            if !closing.detached {
                match closing.bridge.detach_prepared() {
                    Ok(retired) => {
                        closing.pins.push(Box::new(retired));
                        closing.detached = true;
                    }
                    Err(_) => {
                        queue.push_back(closing);
                        continue;
                    }
                }
            }
            let dropped = closing
                .bridge
                .drain_native_retirement(remaining)
                .unwrap_or(0);
            remaining = remaining.saturating_sub(dropped);
            // Windows closing=true is insufficient without a successful detach above.
            if closing.detached && closing.bridge.native_retirement_complete() {
                let Closing {
                    bridge,
                    sink,
                    worker,
                    mut pins,
                    ..
                } = closing;
                drop(bridge); // Must stay on the main thread, before registry pins retire.
                pins.push(Box::new(sink));
                worker.close_with(pins); // Exactly one final handoff per window.
            } else {
                queue.push_back(closing);
            }
        }
        budget - remaining
    }
    /// Only called after Application::run returns and explicit quit is committed.
    pub fn retain_at_process_exit(self) {
        if self.quit_committed() && !self.is_empty() {
            std::mem::forget(self);
        }
    }
}
