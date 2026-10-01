# Dependency graph (Phase 2 advisor plan review reconciled in D-0010)

```mermaid
graph TD
  F001[Core / GPUI boundary] --> F002[Oracle harness]
  F001 --> F013[Performance baseline]
  F002 --> F013
  F002 --> F003[Action registry]
  F002 --> F004[State schema]
  F004 --> F005[Filesystem service]
  F004 --> F007[Config and persisted formats]
  F004 --> F014[Platform service contracts]
  F003 --> F011[GPUI event/focus adapter]
  F005 --> F006[Async jobs]
  F007 --> F008[Theme tokens]
  F003 --> F009[File list and panes]
  F004 --> F009
  F005 --> F009
  F008 --> F009
  F011 --> F009
  F013 --> F009
  F003 --> F010[Overlay / text-input host]
  F008 --> F010
  F011 --> F010
  F009 --> F012[Responsive layout]
  F010 --> F012
  F011 --> F012
  F009 --> R[Parity behavior rows]
  F010 --> R
  F014 --> R
  F006 --> R
```

Edges in feature rows are the row-specific dependency record. Foundation APIs are pre-wired by the integrator before independent feature batches. F-013 captures the baseline before performance-sensitive UI work; it does not claim a performance improvement.

F-150 (CI) depends on F-001 and F-002. It runs independently after those contracts; platform workflows must also build both independent Cargo manifests and add native smoke evidence.
