//! Unit-level adapter probe. Including the implementation in this isolated
//! integration-test crate gives the test access to its private encoder without
//! widening the production API solely for review.

mod baseline {
    pub use ira_parity::baseline::{Baseline, BaselineResolver, ORACLE_SHA};
}
mod environment {
    pub use ira_parity::environment::*;
}
mod normalize {
    pub use ira_parity::normalize::*;
}
mod testing {
    pub struct ScriptedTarget;

    impl ScriptedTarget {
        pub fn recording() -> Self {
            Self
        }
    }

    impl crate::TraceTarget for ScriptedTarget {
        fn name(&self) -> &str {
            "included-runner-test-double"
        }
        fn start(
            &mut self,
            _: &std::path::Path,
            _: (u16, u16),
            _: &std::collections::BTreeMap<String, String>,
        ) -> Result<(), crate::RunError> {
            Ok(())
        }
        fn wait_ready(
            &mut self,
            _: &ira_parity::trace::Readiness,
            _: std::time::Instant,
        ) -> Result<(), crate::RunError> {
            Ok(())
        }
        fn apply(&mut self, _: &ira_parity::trace::InputEvent) -> Result<(), crate::RunError> {
            Ok(())
        }
        fn observe(
            &mut self,
            _: &crate::ObservationKind,
        ) -> Result<crate::Observation, crate::RunError> {
            Ok(crate::Observation::default())
        }
        fn shutdown(&mut self) -> Result<i32, crate::RunError> {
            Ok(0)
        }
    }
}
mod trace {
    pub use ira_parity::trace::*;
}

include!("../src/runner.rs");

mod tests {
    use super::*;

    // GAP(G-F002-LOG-05) sev=high kind=behavior-divergence feature=F-002
    //   what:     Logical paste is encoded as ordinary terminal input bytes instead of a bracketed-paste event.
    //   tui-ref:  migration/specs/F-002.md S3-S5; src/tui.rs:61-67; src/event.rs:52-64; src/main.rs:65-68
    //   oracle:   frozen TUI enables bracketed paste and dispatches Crossterm Paste to app.handle_paste.
    //   repro:    Encode InputEvent::Paste { value: "pasted" } for the frozen TUI PTY adapter.
    //   expected: PTY bytes are CSI 200~ + payload + CSI 201~ so Crossterm emits one Paste event.
    //   actual:   runner.rs::encode currently writes only the payload bytes, which Crossterm reads as ordinary key events.
    //   cover:    paste_event_uses_bracketed_paste_protocol
    #[test]
    fn paste_event_uses_bracketed_paste_protocol() {
        let event = InputEvent::Paste {
            value: "pasted".into(),
        };
        assert_eq!(
            encode(&event).unwrap(),
            b"\x1b[200~pasted\x1b[201~".to_vec()
        );
    }
}
