# Source/runtime check: external integration availability

- `src/handler.rs` has direct hotkey dispatch for `0`, `-`, `]`, and file Enter/right-arrow; no generic `:`/shell command dispatch. `*` exposes a fixed help list, not a command palette.
- No `$EDITOR` or `$PAGER` launch/configuration found. Tab in a text preview opens IRA's internal textarea (`src/app.rs:1223+`). Normal open uses OS default handler only (`src/app.rs:2206-2229`, `4704-4721`). No app-choice dialog.
- No git status queries; `.git` names only receive icons/categories (`src/theme/icons.rs`). No SSH/FTP/S3/VFS/remote backend, plugin registry, scripting hook, or file-drop receiver found in source/Cargo.toml searches. OSC52 clipboard reference to SSH is terminal protocol support, not remote filesystem support.
- Dynamic scope: while in Home on the fresh fixture, help/status exposed `v` preview and `0` terminal plus file-manager/copy-path actions in on-screen footer/help. Did not launch OS applications, copy to host clipboard, mount a physical drive, or attempt a command prompt. These are source-audited/safety-limited, not claimed as dynamic proof.
