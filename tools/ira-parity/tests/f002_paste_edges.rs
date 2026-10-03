//! S14 edge contract for the frozen TUI PTY encoder.
mod runner {
    pub use crate::{Observation, RunCapture};
}
#[allow(dead_code)] // Included encoder probe compiles unrelated golden methods.
mod golden {
    include!("../src/golden.rs");
}
mod baseline {
    pub use ira_parity::baseline::*;
}
mod environment {
    pub use ira_parity::environment::*;
}
mod normalize {
    pub use ira_parity::normalize::*;
}
mod lifecycle {
    pub use ira_parity::lifecycle::*;
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
            "paste-contract-target"
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

    fn event(value: &str) -> InputEvent {
        InputEvent::Paste {
            value: value.into(),
        }
    }

    // GAP-FIXED(G-F002-ADV-55) sev=high kind=behavior-divergence feature=F-002
    //   fixed-by: typed actual observation bundles and explicit bracketed paste protocol (T-006)
    //   what:     Unix paste must preserve multiline and safe escape-looking payloads inside bracketed-paste framing.
    //   tui-ref:  migration/specs/F-002.md S3,S14; frozen Crossterm 0.29 Unix parser
    //   oracle:   Live Linux PTY probe is recorded in migration/reports/F-002/capture-paste-red-1.md.
    //   repro:    Encode newline, partial terminator, partial opener, and arrow-looking payloads.
    //   expected: CSI 200~ + exact payload + CSI 201~ for every safe payload.
    //   actual:   encode(Paste) currently returns the raw payload without framing.
    //   cover:    unix_paste_frames_newline_and_safe_escape_looking_payloads
    #[cfg(unix)]
    #[test]
    fn unix_paste_frames_newline_and_safe_escape_looking_payloads() {
        let payloads = [
            "line one\nline two",
            "\x1b[20x harmless partial marker",
            "\x1b[200x opening-like but not the exact opener",
            "ordinary text with \x1b[A arrow-looking content",
        ];
        for payload in payloads {
            assert_eq!(
                encode(&event(payload)).unwrap(),
                [
                    b"\x1b[200~".as_slice(),
                    payload.as_bytes(),
                    b"\x1b[201~".as_slice()
                ]
                .concat()
            );
        }
    }

    // GAP-FIXED(G-F002-ADV-56) sev=high kind=edge-case feature=F-002
    //   fixed-by: typed actual observation bundles and explicit bracketed paste protocol (T-006)
    //   what:     Unix paste must reject a payload containing the exact closing delimiter.
    //   tui-ref:  migration/specs/F-002.md S14; Crossterm terminates at the first ESC[201~.
    //   oracle:   Frozen parser cannot represent the delimiter as payload content.
    //   repro:    Encode "prefix ESC[201~ suffix" as one logical paste.
    //   expected: Explicit unsupported/invalid-paste diagnostic, with no bytes delivered.
    //   actual:   Current encoder accepts it and would let Crossterm truncate/misframe input.
    //   cover:    unix_paste_rejects_the_exact_closing_marker
    #[cfg(unix)]
    #[test]
    fn unix_paste_rejects_the_exact_closing_marker() {
        let result = encode(&event("prefix\x1b[201~suffix"));
        assert!(
            result.is_err(),
            "closing marker inside a payload would terminate Crossterm paste early"
        );
        assert!(result
            .unwrap_err()
            .to_string()
            .to_lowercase()
            .contains("paste"));
    }

    // GAP-FIXED(G-F002-ADV-57) sev=medium kind=edge-case feature=F-002
    //   fixed-by: typed actual observation bundles and explicit bracketed paste protocol (T-006)
    //   what:     Ordinary Text must remain ordinary terminal input, not acquire paste framing.
    //   tui-ref:  migration/specs/F-002.md S3,S14
    //   oracle:   Text is a separate logical event from Crossterm Paste.
    //   repro:    Encode Text containing newline and escape-like content.
    //   expected: Encoder emits its ordinary Text representation without bracketed-paste markers.
    //   actual:   This assertion currently passes; retain it as a regression guard while paste changes.
    //   cover:    ordinary_text_does_not_gain_bracketed_paste_framing
    #[test]
    fn ordinary_text_does_not_gain_bracketed_paste_framing() {
        let text = InputEvent::Text {
            value: "ordinary\ntext\x1b[A".into(),
        };
        assert_eq!(encode(&text).unwrap(), b"ordinary\ntext\x1b[A");
    }

    #[cfg(windows)]
    // GAP(G-F002-ADV-58) sev=high kind=platform feature=F-002
    //   what:     Windows paste must be explicitly unsupported if frozen native Crossterm emits no Paste event.
    //   tui-ref:  migration/specs/F-002.md S14; Windows native InputRecord backend
    //   oracle:   No native Windows runner was available for this branch's local test session.
    //   repro:    Encode a Paste on Windows after a live native probe confirms the frozen event path.
    //   expected: Explicit unsupported diagnostic; never type the payload as ordinary keys.
    //   actual:   Current encoder returns raw payload; behavior has not been natively probed here.
    //   cover:    windows_frozen_oracle_reports_paste_unsupported_instead_of_typing_it
    #[test]
    fn windows_frozen_oracle_reports_paste_unsupported_instead_of_typing_it() {
        let error = encode(&event("paste payload")).expect_err(
            "Windows native Crossterm has no Paste event path unless a live probe proves otherwise",
        );
        assert!(error.to_string().to_lowercase().contains("unsupported"));
    }
}
