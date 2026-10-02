# Advisory: F-002 platform spec follow-up by platform-advisor round 2

Scope reviewed: F-002 S12-S14 and D-0013; `migration/reports/F-002/architecture-redesign-1.md` and `architecture-spec-review-2.md`; exact pinned `portable-pty 0.9.0` and frozen Crossterm `0.29.0` source in the local Cargo registry. No production code or tests changed.

## Verdict: PROCEED WITH CHANGES

S12-S14 address real gaps, but S12's stated shutdown order is not implementable as written on the pinned APIs, and S14 cannot assume one Crossterm paste path across Unix and Windows.

## Recommendations (ranked)

### R1 [must] S12 — ADAPT teardown ordering to actual PTY handle semantics

`portable_pty::Child` exposes `try_wait`, blocking `wait`, and `kill`, but no timed wait (`portable-pty-0.9.0/src/lib.rs:130-152`). `MasterPty` provides a cloned blocking reader and writer; dropping the writer sends EOF, but the trait exposes no reader cancellation or close method (`src/lib.rs:88-102`). The timeout and thread-join guarantees must therefore be implemented by the harness and proved against each native backend; they are not guaranteed by the crate.

On Windows, `ConPtyMasterPty` clones an `Arc<Mutex<Inner>>`, and the inner object owns the ConPTY plus pipe handles (`portable-pty-0.9.0/src/win/conpty.rs:10-58,75-98`). The cloned reader is a cloned file descriptor. `PsuedoCon::drop` calls `ClosePseudoConsole` (`src/win/psuedocon.rs:70-75`). Microsoft documents that closing ConPTY can emit a final output frame and that output must be drained or closed in the correct order; on older supported Windows releases closure may wait for attached clients to disconnect ([ClosePseudoConsole](https://learn.microsoft.com/en-us/windows/console/closepseudoconsole), [Creating a Pseudoconsole Session](https://learn.microsoft.com/en-us/windows/console/creating-a-pseudoconsole-session)). Consequently, “close PTY handles, drain reader, then join” is contradictory and may deadlock or lose output.

**Adapt S12** to specify phases rather than an unconditional close-then-drain sequence: (1) stop input and drop writer/slave-side owned handles to signal EOF; (2) poll/terminate/reap the child using a deadline; (3) continue servicing reader output while ConPTY is closed, or use a platform-specific sequence shown by the native test to close it without blocking; (4) stop and join the reader within a separate deadline; (5) only after the reader is gone, remove the fixture. If a reader cannot be stopped and joined, report cleanup failure and retain the fixture; do not detach a thread and claim cleanup succeeded. Preserve the original operation error alongside cleanup errors. On Windows test spawned descendants too: ConPTY closure may be needed to terminate attached clients, and waiting for the initial child alone may not drain the session.

**Required live evidence:** Run the same extracted session/helper child on Linux, macOS, and Windows MSVC; include child still alive, normal child exit with queued output, reader failure, and timeout. Record OS/build, child exit status, reader termination/join, and fixture existence at each phase. Use broad duration bounds and diagnose a timeout per OS rather than asserting a tight timing. Windows must demonstrate that the actual `portable-pty` ConPTY and duplicated pipe handles are no longer held when fixture deletion is attempted. The local harness can compile/test only Linux behavior; native Windows/macOS claims require F-150 CI evidence.

### R2 [must] S13 — ADAPT byte and path representation for Windows and macOS filesystems

The requirement for exact persisted bytes is sound, but a TOML string is not a byte container. Use a tagged encoding for each observed payload (`utf8` when valid, otherwise `base64` or hex), while keeping terminal screen text's decoding/normalization rules separate. Include the `ObservationKind` and relative path identity, stable ordering, and write bundle members into a unique staging directory. Write the manifest last (or use a same-volume atomic rename) so interrupted capture cannot look complete. Prefer create-new unique staging outputs; Windows replacement/rename behavior differs when a destination exists or is open, while macOS APFS is commonly Unicode-normalization-insensitive. Avoid comparing path identity by lossy `String`: Windows names are UTF-16 and Unix names may contain non-UTF-8 bytes. Store a reversible OS-path representation or explicitly constrain and validate fixture names as Unicode at schema parse time.

**Required tests/evidence:** Round-trip arbitrary bytes (including NUL and invalid UTF-8), Unicode NFC/NFD names, and a non-Unicode Unix fixture name if the schema allows it; test deterministic observation order and interrupted writes. On Windows, verify the bundle is readable after close and that staging cleanup/rename succeeds without open-handle sharing violations. Validate that every payload is from `observe(kind)` and not copied from trace expectations. Keep this feature staging-only until logic review approves a captured candidate.

### R3 [must] S14 — ADAPT paste framing by OS; do not claim uniform frozen-oracle support

On Unix, Crossterm 0.29.0 recognizes `ESC [ 200 ~ ... ESC [ 201 ~` in `event/sys/unix/parse.rs:197-199,813-822`; the parser ends at the first exact closing marker and uses lossy UTF-8 conversion. Thus the string payload cannot contain the exact terminator unambiguously. Reject a payload containing `ESC[201~` explicitly. Test harmless opening-like/partial escape sequences, newlines, and other escape-looking text as preserved payload; distinguish these from the exact terminator. The schema already uses a Unicode `String`, so arbitrary invalid UTF-8 is outside its paste value domain.

Crossterm's Windows event source (`event/source/windows.rs:1-95`) reads Win32 console `InputRecord`s and dispatches key, mouse, resize, and focus records; it has no bracketed-paste parser. The Unix VT byte parser is not the Windows implementation. Since `portable-pty` uses ConPTY pipes, S14 must not infer that writing Unix bracketed-paste delimiters makes Windows `Event::Paste` happen. Run an oracle probe on native Windows MSVC with the exact frozen binary/ConPTY path. If the frozen TUI cannot produce `Event::Paste` there, keep that outcome explicit as unsupported and make the Windows scenario fail/limit the paste case with a precise unsupported-event diagnostic; do not silently type the payload or report it as passed. Do not change frozen TUI behavior to manufacture parity.

**Required live evidence:** On Linux and macOS, prove the frozen binary dispatches to `Event::Paste`/`handle_paste` for ordinary text, newline, safe escape-like sequences, and explicitly rejected terminator content; show ordinary `Text` remains ordinary input. On Windows, capture the observed ConPTY/Crossterm path and resulting event. Preserve exact oracle SHA, OS/build, payload, and observation in the report/artifact. Until the native Windows probe exists, that part remains unverified and blocks F-002 completion under the existing native-run requirement.

### R4 [should] D-0013 sequencing — ADAPT by landing per-platform contracts with each slice

Retain lifecycle first, followed by the byte-safe observation format, then paste. Add platform-specific acceptance criteria to each red-test brief before implementation. Run the same test names natively on all three OSes; any unsupported Windows paste result needs a recorded evidence artifact and explicit test outcome, not an ignored test or an assertion imported from Unix. Keep F-150 as the evidence collection barrier; Linux-only success does not resolve Windows/macOS lifecycle behavior.

## Risks not covered

No additional risks beyond native GUI runner availability already tracked under F-150. The current S12-S14 text should be revised against R1-R3 before implementation; this report does not authorize an implementation or waive native evidence.
