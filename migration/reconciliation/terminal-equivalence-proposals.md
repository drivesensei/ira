# Proposed terminal-to-native intent mappings — not signoffs

These proposals supplement historical D0005; every affected row stays UNVERIFIED. Advisor, adversarial and logic signoffs plus actual platform screenshots/effects remain pending. No WAIVED/EQUIVALENT_VERIFIED transition is authorized by this document.

| Rows | Source intent | Proposed native observable equivalent | Missing proof |
|---|---|---|---|
| F068/F069/F070 | src/preview macOS/Windows overlays place a bounded image beside terminal cells, hide when terminal loses foreground, support pane composition | GPUI-contained image/preview uses pane bounds, remains clipped, hides with its native window, supports both panes | actual macOS/Windows placement, resize, focus, multiple-pane screenshots + required three signoffs |
| F072/F073 | src/theme/icons.rs and Windows font configuration readers select printable/fallback glyphs from available terminal fonts/settings | native font coverage selects readable fallback glyphs without invisible rows; terminal diagnostics retain host-specific distinction | native font/config variants, missing-glyph screenshots; IRA font inputs/signoff; --check-terminal is not a waiver |
| F088 | src/app mount/eject on Linux only, errors retain removable-drive state | same platform service command/result/error contract from native action | actual Linux removable-volume tests + unsupported platform behavior proof; not replaced by a no-op |
| F028 | src/main --check-terminal reports actual terminal protocol/font environment | desktop may explicitly report its native host, while TUI diagnostic remains byte-compatible | source intent review + documented host difference approval, native execution/signoffs; version/check-terminal cannot waive preview/font/TTY rows |

The six historical blocked rows remain explicit completion blockers until proof exists; diagnostic text alone is insufficient.
