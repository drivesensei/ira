# Parity matrix

Manager parity ledger derived from nine discovery inventories. Surface ownership is recorded in `migration/surface-ownership.tsv`; six terminal-host rows are BLOCKED pending signed intent mappings. The 2026-10-01 Phase 2 advisor review is reconciled in D-0010; all other rows remain NOT_STARTED.

| ID | Area | Feature | Surface | TUI refs | Deps | Wave | Status | Owner | Evidence | Gaps |
|---|---|---|---|---|---|---|---|---|---|---|
| F-001 | foundation | Core crate and TUI/GPUI boundary | - | Cargo.toml; desktop/Cargo.toml; INV-DOCS-002 | - | 0 | VERIFIED | - | tests/boundary/test_f001_boundary.py (7 passed); cargo build --locked; desktop locked build; root tests (278 passed); migration/reports/F-001/{dev-1,logic-review,adversarial-review}.md; CI run 36937979352 (macOS + Windows); merge e346263 | - |
| F-002 | foundation | Frozen-oracle scenario and differential parity harness | - | tools/ira-parity/**; migration/oracle/traces/**; migration/reports/F-002/** | F-001 | 0 | NOT_STARTED | - | - | - |
| F-003 | foundation | Action registry and keymap engine | - | src/handler.rs; INV-INPUT-001 | F-001,F-002,F-004 | 0 | NOT_STARTED | - | - | - |
| F-004 | foundation | Application state model | - | src/app.rs; INV-STATE-001 | F-001,F-002 | 0 | NOT_STARTED | - | - | - |
| F-005 | foundation | Filesystem service and error taxonomy | - | src/services/filesystem.rs; INV-filesystem-001 | F-001,F-004 | 0 | NOT_STARTED | - | - | - |
| F-006 | foundation | Async job runner with progress and cancellation | - | src/services/jobs.rs; INV-files-ops-012 | F-001,F-004,F-005 | 0 | NOT_STARTED | - | - | - |
| F-007 | foundation | Config, session, and persisted-format compatibility | - | src/services/state.rs; INV-CONFIG-001 | F-001,F-004 | 0 | NOT_STARTED | - | - | - |
| F-008 | foundation | Theme tokens and theme loading | - | src/theme/**; INV-CONFIG-005 | F-001,F-007 | 0 | NOT_STARTED | - | - | - |
| F-009 | foundation | Reusable file list view and pane composition | - | src/ui/**; INV-RENDER-002 | F-003,F-004,F-005,F-008,F-011,F-013 | 0 | NOT_STARTED | - | - | - |
| F-010 | foundation | Overlay host, dialogs, prompts, and status messages | - | src/ui/**; INV-RENDER-006 | F-003,F-008,F-011 | 0 | NOT_STARTED | - | - | - |
| F-011 | foundation | GPUI event and keyboard-focus adapter contracts | - | desktop/src/**; UX floor | F-001,F-003 | 0 | NOT_STARTED | - | - | - |
| F-012 | foundation | Resize behavior and responsive layout | - | INV-RENDER-001; desktop window sizing | F-009,F-010,F-011 | 0 | NOT_STARTED | - | - | - |
| F-013 | foundation | Performance baselines and regression budgets | - | migration/oracle/perf.md; 100k entries | F-001,F-002 | 0 | NOT_STARTED | - | - | - |
| F-014 | foundation | Platform service traits and OS path/process adapters | - | src/services/{folders,drives,filesystem}.rs; desktop/src/platform/contracts/** | F-001,F-004 | 0 | NOT_STARTED | - | - | - |
| F-015 | config | User configuration roots and supported files | file:~/.config/ira/state | INV-CONFIG-001 | F-007 | 1 | NOT_STARTED | - | - | - |
| F-016 | config | Session state file format, defaults, and tolerant parsing | file:state,cfg:state.split,cfg:state.theme,cfg:state.preview1,cfg:state.active,cfg:session.hidden,cfg:state.right,cfg:state.left,cfg:state.size,cfg:state.preview0,cfg:state.hidden | INV-CONFIG-002 | F-004,F-007 | 1 | NOT_STARTED | - | - | - |
| F-017 | config | Session contents and persistence timing | - | INV-CONFIG-003 | F-004,F-007 | 1 | NOT_STARTED | - | - | - |
| F-018 | config | Bookmark list persistence and byte-compatible round trip | ui:bookmark-row,file:bookmarks | INV-CONFIG-004 | F-007 | 1 | NOT_STARTED | - | - | - |
| F-019 | config | Theme preset source, aliases, and cycle | cfg:theme.preset | INV-CONFIG-005 | F-008 | 1 | NOT_STARTED | - | - | - |
| F-020 | config | Theme TOML fields, colors, and icon environment override | cfg:theme.dir,cfg:theme.surface_alt,cfg:theme.shadow,cfg:theme.icons,cfg:theme.chips,cfg:theme.key_fg,cfg:theme.text_muted,cfg:theme.files.video,cfg:theme.files.audio,cfg:theme.border,file:theme.toml,cfg:theme.error,cfg:theme.hidden,cfg:theme.accent,cfg:theme.border_active,cfg:theme.files.archive,cfg:theme.info,cfg:theme.selection,cfg:theme.surface,cfg:theme.files.data,cfg:theme.files.code,cfg:theme.bg,cfg:theme.text,cfg:theme.cursor_bg,env:IRA_ICONS,cfg:theme.cursor_fg,cfg:theme.files.document,cfg:theme.key_bg,cfg:theme.success,cfg:theme.files.image,cfg:theme.files.executable,cfg:theme.warning | INV-CONFIG-006 | F-008 | 1 | NOT_STARTED | - | - | - |
| F-021 | config | Two panes, active pane, and no tab/history stack | - | INV-STATE-001 | F-004,F-009 | 1 | NOT_STARTED | - | - | - |
| F-022 | config | Cursor and multi-selection model | - | INV-STATE-002 | F-004,F-009 | 1 | NOT_STARTED | - | - | - |
| F-023 | config | Per-pane sort modes | - | INV-STATE-003 | F-004,F-009 | 1 | NOT_STARTED | - | - | - |
| F-024 | config | Hidden-file visibility | - | INV-STATE-004 | F-004,F-005,F-009 | 1 | NOT_STARTED | - | - | - |
| F-025 | config | Live fuzzy search and sticky filter | - | INV-STATE-005 | F-003,F-004,F-009,F-010 | 1 | NOT_STARTED | - | - | - |
| F-026 | config | Folder navigation and absent navigation history | - | INV-STATE-006 | F-004,F-005,F-009 | 1 | NOT_STARTED | - | - | - |
| F-027 | input | --version` / `-V` early exit | msg:ira-version-line,exit:code:0,cli:--version,cli:-V | INV-ENTRY-001 | F-001 | 1 | NOT_STARTED | - | - | - |
| F-028 | input | --check-terminal` diagnostic exit | ui:terminal-capability-report,env:WT_SESSION,env:IRA_IMAGES,env:LC_TERMINAL,env:TERM_PROGRAM,cli:--check-terminal,env:TERM | INV-ENTRY-002 | F-001,F-007,F-014 | 1 | NOT_STARTED | - | - | - |
| F-029 | input | Ordinary startup and initialization order | ui:initial-tui-screen | INV-ENTRY-003 | F-001,F-004,F-007,F-009,F-011 | 1 | NOT_STARTED | - | - | - |
| F-030 | input | Session restore and initial pane source | - | INV-ENTRY-004 | F-004,F-007,F-009 | 1 | NOT_STARTED | - | - | - |
| F-031 | input | Unrecognized CLI args and arguments without a TTY | cli:unknown-argument-falls-through | INV-ENTRY-005 | F-001 | 1 | NOT_STARTED | - | - | - |
| F-032 | input | Press-only event loop, ignored events, and paste routing | ui:event-bracketed-paste,ui:event-press-only,ui:event-resize-ignored,ui:event-mouse-ignored,mode:normal,ui:event-focus-ignored | INV-INPUT-001 | F-003,F-011 | 2 | NOT_STARTED | - | - | - |
| F-033 | input | Cursor movement and directory enter/leave | key:normal:z,key:normal:down,key:normal:alt-down,key:normal:alt-up,key:normal:Right,key:normal:right,key:normal:left,key:normal:Left,key:normal:x,key:normal:up | INV-INPUT-002 | F-003,F-004,F-005,F-009 | 2 | NOT_STARTED | - | - | - |
| F-034 | input | Quit action bindings | key:normal:q,key:global:ctrl-c | INV-INPUT-003 | F-003,F-004,F-009 | 2 | NOT_STARTED | - | - | - |
| F-035 | input | Copy-to-other-pane action binding | key:normal:c | INV-INPUT-004 | F-003,F-004,F-005,F-006,F-009 | 2 | NOT_STARTED | - | - | - |
| F-036 | input | Rename prompt mode | key:normal:enter,key:rename:enter,key:rename:left,key:normal:Enter,key:rename:backspace,key:rename:escape,mode:rename,key:rename:char,key:rename:right | INV-INPUT-005 | F-003,F-004,F-010,F-011 | 2 | NOT_STARTED | - | - | - |
| F-037 | input | Go-to-path prompt mode | key:goto-path:backspace,key:goto-path:escape,key:normal:left-bracket,key:goto-path:char,key:goto-path:enter,mode:goto-path | INV-INPUT-006 | F-003,F-004,F-005,F-010,F-011 | 2 | NOT_STARTED | - | - | - |
| F-038 | input | Create-new prompt mode | key:create-entry:char,key:create-entry:right,key:create-entry:backspace,key:create-entry:left,key:create-entry:escape,mode:create-entry,key:normal:n,mode:new-entry,key:create-entry:enter | INV-INPUT-007 | F-003,F-004,F-005,F-006,F-010 | 2 | NOT_STARTED | - | - | - |
| F-039 | input | Fuzzy-search input and confirmed filter | mode:search,key:fuzzy-search:char,key:fuzzy-search:down,key:fuzzy-search:alt-down,key:fuzzy-search:backspace,msg:select a file,key:fuzzy-search:up,key:fuzzy-search:right,key:search:Enter,mode:fuzzy-search-input,msg:select-a-file,key:fuzzy-search:enter,key:fuzzy-search:escape,key:search:Esc,key:fuzzy-search:alt-up | INV-INPUT-008 | F-003,F-004,F-009,F-010 | 2 | NOT_STARTED | - | - | - |
| F-040 | input | Copy Board focus mode | key:copy-board:asterisk,key:copy-board:backtick,key:copy-board:v,key:copy-board:tab-focus,key:copy-board:space,key:copy-board:q,key:copy-board:backslash,key:copy-board:down,mode:copy-board-focus,key:copy-board:p,key:copy-board:escape,key:copy-board:up,key:copy-board:x | INV-INPUT-009 | F-003,F-004,F-006,F-009 | 2 | NOT_STARTED | - | - | - |
| F-041 | input | Confirmation prompt mode | key:confirmation:escape,key:confirm:n,key:confirm:escape,key:confirm:y,mode:confirmation,key:confirmation:y,key:confirm:o,key:confirmation:enter,key:confirmation:n,key:normal:o,key:confirmation:o,key:confirm:enter | INV-INPUT-010 | F-003,F-010 | 2 | NOT_STARTED | - | - | - |
| F-042 | input | Info dialog actions | key:info-dialog:r,msg:Error reading metadata,ui:info-dialog,key:info-dialog:x,mode:info-dialog | INV-INPUT-011 | F-003,F-004,F-006,F-010 | 2 | NOT_STARTED | - | - | - |
| F-043 | input | Preview editor focus, discard, and exit behavior | key:preview-text-edit:textarea,key:editor:esc:exit-preview-text,mode:preview-text-edit,key:editor:tab:discard-and-focus-cycle,key:preview-text-edit:tab,key:preview-text-edit:escape | INV-INPUT-012 | F-003,F-010,F-011 | 2 | NOT_STARTED | - | - | - |
| F-044 | input | Transient shortcut help and key priority | key:normal:asterisk,mode:keybindings-help,key:keybindings-help:any-key-dismiss | INV-INPUT-013 | F-003,F-009,F-010 | 2 | NOT_STARTED | - | - | - |
| F-045 | fileops | Directory enumeration and metadata | - | INV-files-ops-001 | F-004,F-005 | 2 | NOT_STARTED | - | - | - |
| F-046 | fileops | Sort cycle and cursor retention | msg:Sorted by last modified (newest first),ui:four-mode-sort-cycle,msg:Sorted by name,msg:Sorted by size (largest first),msg:Sorted by kind | INV-files-ops-002 | F-004,F-009 | 2 | NOT_STARTED | - | - | - |
| F-047 | fileops | Hidden-file toggle | - | INV-files-ops-003 | F-004,F-005,F-009 | 2 | NOT_STARTED | - | - | - |
| F-048 | fileops | Details row format | - | INV-files-ops-004 | F-008,F-009 | 2 | NOT_STARTED | - | - | - |
| F-049 | fileops | Operation source selection | - | INV-files-ops-005 | F-004,F-009 | 2 | NOT_STARTED | - | - | - |
| F-050 | fileops | Create file/folder | msg:'{name}' already exists and is not a folder.,msg:Failed to create,msg:Enter a name first.,msg:already exists and is not a folder.,msg:'{name}' already exists.,msg:already exists,msg:FAILED ( | INV-files-ops-006 | F-004,F-005,F-006,F-010 | 2 | NOT_STARTED | - | - | - |
| F-051 | fileops | Rename entry | msg:Failed to rename:,msg:Cannot rename,msg:Cannot rename:,msg:Failed to rename | INV-files-ops-007 | F-004,F-005,F-006,F-010 | 2 | NOT_STARTED | - | - | - |
| F-052 | fileops | Copy | msg:SKIPPED (already exists):,msg:The other pane has no folder to copy into.,msg:Cannot copy/move a folder into itself. | INV-files-ops-008 | F-004,F-005,F-006,F-009,F-010 | 2 | NOT_STARTED | - | - | - |
| F-053 | fileops | Move | - | INV-files-ops-009 | F-004,F-005,F-006,F-010 | 2 | NOT_STARTED | - | - | - |
| F-054 | fileops | Conflict policy | - | INV-files-ops-010 | F-005,F-006,F-010 | 2 | NOT_STARTED | - | - | - |
| F-055 | fileops | Permanent delete confirmation | msg:Failed to delete '{path}': {err},msg:Failed to delete | INV-files-ops-011 | F-004,F-005,F-006,F-010 | 2 | NOT_STARTED | - | - | - |
| F-056 | fileops | Copy Board progress, pause, cancel | - | INV-files-ops-012 | F-003,F-004,F-006,F-010 | 2 | NOT_STARTED | - | - | - |
| F-057 | fileops | Transfer metadata and symlink behavior | - | INV-files-ops-013 | F-004,F-005,F-006 | 2 | NOT_STARTED | - | - | - |
| F-058 | filesystem | Directory listing and display columns | msg:loading…,ui:filesystem-metadata,ui:filesystem-listing,msg:Error reading entry,msg:loading {label}…,msg:Loading…,msg:Error reading entry: {e} | INV-filesystem-001 | F-004,F-005,F-009 | 1 | NOT_STARTED | - | - | - |
| F-059 | filesystem | Hidden entry filtering | ui:filesystem-hidden-filter | INV-filesystem-002 | F-004,F-005,F-009 | 1 | NOT_STARTED | - | - | - |
| F-060 | filesystem | Metadata following symlinks | ui:filesystem-symlink-classification | INV-filesystem-003 | F-004,F-005 | 1 | NOT_STARTED | - | - | - |
| F-061 | filesystem | Bounded listing and large-folder streaming | ui:filesystem-large-directory-streaming | INV-filesystem-004 | F-004,F-005,F-006,F-009 | 1 | NOT_STARTED | - | - | - |
| F-062 | filesystem | Metadata dialog and recursive folder size | ui:metadata-size-progress | INV-filesystem-005 | F-004,F-005,F-006,F-010 | 1 | NOT_STARTED | - | - | - |
| F-063 | filesystem | Listing errors and refresh behavior | ui:filesystem-refresh-after-operations,ui:filesystem-external-change-no-auto-refresh | INV-filesystem-006 | F-004,F-005,F-009 | 1 | NOT_STARTED | - | - | - |
| F-064 | filesystem | Permissions and special files | ui:filesystem-special-file-classification,ui:filesystem-permissions-display | INV-filesystem-007 | F-004,F-005,F-009 | 1 | NOT_STARTED | - | - | - |
| F-065 | platform | Public library module surface | - | INV-PLAT-001 | F-001 | 2 | NOT_STARTED | - | - | - |
| F-066 | platform | Common-folder path discovery | - | INV-PLAT-002 | F-004,F-014 | 2 | NOT_STARTED | - | - | - |
| F-067 | platform | Common-folder panel and key navigation | msg:No common folders found,ui:empty-common-folders,ui:common-folders-empty-message,ui:common-folders-panel,ui:common-folder-row,ui:common-folder-icon | INV-PLAT-003 | F-004,F-009,F-014 | 2 | NOT_STARTED | - | - | - |
| F-068 | platform | Terminal image overlay geometry and composition | ui:overlay-size-cap,ui:terminal-image-preview-overlay,ui:overlay-two-pane-limit | INV-PLAT-004 | F-014 | 2 | BLOCKED(terminal-host behavior needs equivalent mapping/sign-off) | - | - | D-0005 |
| F-069 | platform | macOS native preview overlay | env:TERM_PROGRAM:mac-terminal-detection,ui:macos-appkit-preview-panel,ui:macos-terminal-window-geometry-overlay,ui:overlay-hidden-when-terminal-not-frontmost | INV-PLAT-005 | F-014 | 2 | BLOCKED(terminal-host behavior needs equivalent mapping/sign-off) | - | - | D-0005 |
| F-070 | platform | Windows native preview overlay and font-cell metrics | ui:windows-terminal-window-geometry-overlay,ui:windows-layered-preview-popup | INV-PLAT-006 | F-014 | 2 | BLOCKED(terminal-host behavior needs equivalent mapping/sign-off) | - | - | D-0005 |
| F-071 | platform | Windows volume label lookup | ui:windows-volume-label-display | INV-PLAT-007 | F-004,F-014 | 2 | NOT_STARTED | - | - | - |
| F-072 | platform | Nerd-font capability detection | ui:nerd-font-capability-fallback | INV-PLAT-008 | F-008,F-014 | 2 | BLOCKED(terminal-host behavior needs equivalent mapping/sign-off) | - | - | D-0005 |
| F-073 | platform | Windows Terminal and VS Code JSONC font settings | file:WindowsTerminal-portable-marker-.portable,file:VSCode-User-settings.json,env:WT_PROFILE_ID,env:IRA_WT_SETTINGS,file:WindowsTerminal-settings.json,env:LOCALAPPDATA | INV-PLAT-009 | F-014 | 2 | BLOCKED(terminal-host behavior needs equivalent mapping/sign-off) | - | - | D-0005 |
| F-074 | preview | Preview presentation modes | ui:preview-off,ui:preview-column,msg:Preview-grid,msg:Preview-details,key:normal:v,ui:preview-details,msg:Preview:,ui:preview-grid,msg:Preview-off,msg:Preview-column | INV-PE-001 | F-004,F-009 | 3 | NOT_STARTED | - | - | - |
| F-075 | preview | Text preview content and bounds | ui:preview-readonly-editor,ui:empty-file-preview,msg:… truncated,ui:text-file-preview,msg:binary file,ui:preview-truncated-marker,msg:(empty file),ui:binary-preview-placeholder,msg:empty-file,msg:non-UTF-8 file — read-only preview only,msg:binary-file,msg:read-only,msg:truncated,msg:non-UTF8-preview-readonly,ui:empty-preview-placeholder | INV-PE-002 | F-005,F-009,F-010 | 3 | NOT_STARTED | - | - | - |
| F-076 | preview | In-app text editor is offered from text Column preview | msg:file-path-changed-on-disk,ui:preview-text-editor,msg:file-changed-on-disk,msg:saved-file,msg:file path changed on disk — press Esc and reopen,msg:Saved,msg:file-too-large-to-edit,ui:preview-dirty-indicator,msg:file changed on disk — press Esc and reopen,msg:file too large to edit (> 5 MB),msg:save failed,msg:binary-file-not-editable,msg:binary file — not editable,key:normal:tab:text-preview-editor | INV-PE-003 | F-005,F-006,F-010,F-011 | 3 | NOT_STARTED | - | - | - |
| F-077 | preview | Image preview formats and decode behavior | ui:preview-error-state,msg:cannot decode,msg:format not supported (png/jpg/gif/bmp/webp/mp4/mov/heic/pdf),ui:preview-decode-error-placeholder,msg:cannot-decode,ui:image-preview,msg:format-not-supported | INV-PE-004 | F-005,F-006,F-009 | 3 | NOT_STARTED | - | - | - |
| F-078 | preview | Video preview uses optional ffmpeg | msg:install-ffmpeg-video,env:PATH:ffmpeg-and-pdftoppm,ui:video-frame-preview,msg:install ffmpeg for video previews | INV-PE-005 | F-005,F-006,F-009 | 3 | NOT_STARTED | - | - | - |
| F-079 | preview | HEIC/HEIF preview uses optional ffmpeg | msg:install ffmpeg for HEIC previews,msg:install-ffmpeg-heic,ui:heic-preview | INV-PE-006 | F-005,F-006,F-009 | 3 | NOT_STARTED | - | - | - |
| F-080 | preview | PDF first-page preview uses optional Poppler | msg:install-poppler-pdftoppm,ui:pdf-first-page-preview,msg:install poppler (pdftoppm) for PDF previews | INV-PE-007 | F-005,F-006,F-009 | 3 | NOT_STARTED | - | - | - |
| F-081 | preview | Preview exclusions and contextual placeholders | msg:folders have no image preview,ui:preview-paused-under-modal,msg:preview-paused,msg:folders-have-no-image-preview | INV-PE-008 | F-005,F-009,F-010 | 3 | NOT_STARTED | - | - | - |
| F-082 | preview | Thumbnail async scheduling, prefetch, and cache | ui:preview-loading-placeholder,file:thumbnail-cache-png,env:IRA_THUMBNAIL_CACHE_DIR,ui:preview-loading-state | INV-PE-009 | F-005,F-006,F-009 | 3 | NOT_STARTED | - | - | - |
| F-083 | preview | Open selected file with system default app | ui:system-default-open,msg:failed-to-open,msg:open failed,key:normal:l:open-default-or-directory,key:normal:right:open-default-or-directory,msg:Failed to open,key:normal:enter:open-default,exit:external-app-detached | INV-PE-010 | F-014,F-009 | 3 | NOT_STARTED | - | - | - |
| F-084 | preview | Open-with chooser and editor/pager/shell spawning are absent | ui:open-with-chooser-absent | INV-PE-011 | F-014 | 3 | NOT_STARTED | - | - | - |
| F-085 | preview | Spawn a native terminal in current directory | msg:No folder open to start a terminal in.,msg:no-folder-for-terminal,key:normal:0:terminal-here,key:normal:0,msg:no-terminal-emulator-found,ui:terminal-here,msg:No terminal emulator found | INV-PE-012 | F-014 | 3 | NOT_STARTED | - | - | - |
| F-086 | preview | Reveal in OS file manager | key:normal:minus,key:normal:-:reveal-in-file-manager,msg:no-folder-to-reveal,msg:No folder open to reveal.,msg:no-file-browser-found,ui:reveal-in-file-manager,msg:No file browser found | INV-PE-013 | F-014 | 3 | NOT_STARTED | - | - | - |
| F-087 | preview | Copy current folder path to clipboard | msg:Folder path copied to clipboard,msg:folder-path-copied,msg:failed-to-copy-folder-path,key:normal:right-bracket,msg:Failed to copy the folder path to the clipboard.,ui:clipboard-folder-path,hook:osc52-clipboard-request,key:normal:]:copy-folder-path | INV-PE-014 | F-014 | 3 | NOT_STARTED | - | - | - |
| F-088 | preview | Linux removable media mount/eject is the only on-demand mount integration | msg:Failed to mount,msg:failed-to-mount-drive,msg:Failed to eject,ui:removable-drive-mount,msg:no-removable-drive-mounted,msg:failed-to-eject-drive,ui:removable-drive-eject,msg:No removable drive is mounted | INV-PE-015 | F-014,F-129 | 3 | BLOCKED(terminal-host behavior needs equivalent mapping/sign-off) | - | - | D-0005 |
| F-089 | preview | Git status decorations are absent | ui:git-status-integration-absent | INV-PE-016 | F-009 | 3 | NOT_STARTED | - | - | - |
| F-090 | preview | Remote filesystems / VFS are absent | ui:remote-vfs-integration-absent | INV-PE-017 | F-005 | 3 | NOT_STARTED | - | - | - |
| F-091 | preview | Plugins, user scripting hooks, and command mode are absent | ui:generic-command-mode-absent,ui:plugins-and-hooks-absent | INV-PE-018 | F-001 | 3 | NOT_STARTED | - | - | - |
| F-092 | preview | Drag/drop integration is absent | ui:os-drag-drop-absent | INV-PE-019 | F-114 | 3 | NOT_STARTED | - | - | - |
| F-093 | rendering | Fixed screen layout and minimum size fallback | ui:minimum-terminal-size-warning,msg:Please increase the terminal's size,ui:main-five-row-layout | INV-RENDER-001 | F-009,F-012 | 2 | NOT_STARTED | - | - | - |
| F-094 | rendering | Component rows, pane split, copy-board and preview columns | ui:split-pane-view,ui:copy-board,ui:copy-board-job-state,ui:drive-row,ui:split-pane,ui:actions-row | INV-RENDER-002 | F-009,F-010 | 2 | NOT_STARTED | - | - | - |
| F-095 | rendering | List-row rendering and truncation | ui:truncated-preview,ui:files-list-view,ui:file-row-truncation | INV-RENDER-003 | F-009 | 2 | NOT_STARTED | - | - | - |
| F-096 | rendering | Theme palette, semantic file colors, and chip styles | ui:keybinding-chip-square,ui:keybinding-chip-rounded,ui:theme-preset-label,ui:keybinding-chip-outline,ui:theme-filetype-colors | INV-RENDER-004 | F-008 | 2 | NOT_STARTED | - | - | - |
| F-097 | rendering | Chips, hints, marquee, and keybindings help | ui:keybindings-help,ui:contextual-hint-marquee,ui:keybindings-help-dialog | INV-RENDER-005 | F-003,F-010 | 2 | NOT_STARTED | - | - | - |
| F-098 | rendering | Modal chrome and text input cursor | ui:dialog-glass-chrome,ui:text-input-block-cursor | INV-RENDER-006 | F-010,F-118 | 2 | NOT_STARTED | - | - | - |
| F-099 | rendering | Confirmation prompt widget | ui:confirmation-modal | INV-RENDER-007 | F-010,F-118 | 2 | NOT_STARTED | - | - | - |
| F-100 | rendering | Preview panes, file metadata and size formatting | - | INV-RENDER-008 | F-009 | 2 | NOT_STARTED | - | - | - |
| F-101 | rendering | Status notice/error channel and message catalog | ui:status-notice-bar,ui:error-modal,msg:any key dismiss | INV-RENDER-009 | F-010,F-121 | 2 | NOT_STARTED | - | - | - |
| F-102 | state | Pane and visible-row state | ui:visible-row-cursor-model | INV-STATE-SEARCH-001 | F-004,F-009 | 1 | NOT_STARTED | - | - | - |
| F-103 | state | Cursor movement and bounds | - | INV-STATE-SEARCH-002 | F-004,F-009 | 1 | NOT_STARTED | - | - | - |
| F-104 | state | Directory navigation and path jump | - | INV-STATE-SEARCH-003 | F-004,F-005,F-009 | 1 | NOT_STARTED | - | - | - |
| F-105 | state | Multi-selection state | - | INV-STATE-SEARCH-004 | F-004,F-009 | 1 | NOT_STARTED | - | - | - |
| F-106 | state | Sort modes | - | INV-STATE-SEARCH-005 | F-004,F-009 | 1 | NOT_STARTED | - | - | - |
| F-107 | state | Fuzzy scoring | - | INV-STATE-SEARCH-006 | F-003,F-004 | 1 | NOT_STARTED | - | - | - |
| F-108 | state | Live search mode | ui:fuzzy-live-search-results | INV-STATE-SEARCH-007 | F-003,F-004,F-010 | 1 | NOT_STARTED | - | - | - |
| F-109 | state | Confirmed filter commit and zero-match result rendering | ui:confirmed-zero-result-rendering,ui:confirmed-fuzzy-filter | INV-STATE-SEARCH-008 | F-003,F-004,F-009 | 1 | NOT_STARTED | - | - | - |
| F-110 | state | Two-pane split state | ui:two-pane-split-state | INV-STATE-SEARCH-009 | F-004,F-009 | 1 | NOT_STARTED | - | - | - |
| F-111 | contracts | CLI help and package entry contracts | cli:--help | INV-DOCS-001 | F-001 | 1 | NOT_STARTED | - | - | - |
| F-112 | contracts | Existing test suite and its evidence boundary | - | INV-DOCS-002 | F-002 | 1 | NOT_STARTED | - | - | - |
| F-113 | contracts | Documentation claims and known contract gaps | - | INV-DOCS-003 | F-001,F-002 | 1 | NOT_STARTED | - | - | - |
| F-114 | ux-floor | Mouse click/double-click/wheel/modified-selection action convergence | - | desktop UX floor; desktop/src/** | F-003,F-009,F-010 | 2 | NOT_STARTED | - | - | - |
| F-115 | ux-floor | Native window lifecycle and reopen | - | desktop UX floor; desktop/src/** | F-001,F-011 | 1 | NOT_STARTED | - | - | - |
| F-116 | ux-floor | Window geometry persistence | - | desktop UX floor; desktop/src/** | F-001,F-012 | 2 | NOT_STARTED | - | - | - |
| F-117 | ux-floor | System light/dark theme, contrast, HiDPI, and font fallback | - | desktop UX floor; desktop/src/** | F-008,F-009 | 2 | NOT_STARTED | - | - | - |
| F-118 | ux-floor | Native text input, selection, undo, clipboard, and IME composition | - | desktop UX floor; desktop/src/** | F-003,F-010,F-011 | 2 | NOT_STARTED | - | - | - |
| F-119 | ux-floor | Accessibility focus traversal, roles, contrast, and icon labels | - | desktop UX floor; desktop/src/** | F-009,F-010,F-011 | 2 | NOT_STARTED | - | - | - |
| F-120 | ux-floor | Native file chooser | - | desktop UX floor; desktop/src/platform/** | F-014,F-011 | 2 | NOT_STARTED | - | - | - |
| F-121 | ux-floor | Empty/no-match, loading, error, permission, and offline feedback | - | desktop UX floor; INV-RENDER-009; INV-filesystem-006 | F-005,F-006,F-009,F-010 | 2 | NOT_STARTED | - | - | - |
| F-122 | ux-floor | Native open/reveal/clipboard adapters | - | desktop UX floor; INV-PE-010/013/014 | F-014,F-003 | 3 | NOT_STARTED | - | - | - |
| F-123 | ux-floor | Keyboard-only reachability and menu convergence | - | desktop UX floor; ux-keymodel.md | F-003,F-009,F-010 | 2 | NOT_STARTED | - | - | - |
| F-124 | input | Bookmark toggle action | key:normal:b | INV-INPUT-003; INV-CONFIG-004 | F-003,F-004,F-007,F-009 | 2 | NOT_STARTED | - | - | - |
| F-125 | input | Fuzzy-search activation | key:normal:slash,key:normal:/ | INV-INPUT-003; INV-INPUT-008 | F-003,F-004,F-009,F-010 | 2 | NOT_STARTED | - | - | - |
| F-126 | input | Copy Board activation | key:normal:backtick | INV-INPUT-003; INV-INPUT-009 | F-003,F-004,F-006,F-009 | 2 | NOT_STARTED | - | - | - |
| F-127 | input | Delete action bindings and platform alias | key:normal:delete,key:normal:backspace:macos,key:normal:backspace-macos | INV-INPUT-004 | F-003,F-004,F-010 | 2 | NOT_STARTED | - | - | - |
| F-128 | input | Current-entry selection toggle | key:normal:Space,key:normal:space | INV-INPUT-004 | F-003,F-004,F-009 | 2 | NOT_STARTED | - | - | - |
| F-129 | input | Active-drive eject binding | key:normal:ctrl-minus:eject-drive,key:global:ctrl-minus | INV-INPUT-004; INV-PE-015 | F-003,F-014 | 2 | NOT_STARTED | - | - | - |
| F-130 | input | Error-dialog dismissal and focus restoration | mode:error-dialog,key:dialog:any-key-dismiss | INV-INPUT-011 | F-003,F-010 | 2 | NOT_STARTED | - | - | - |
| F-131 | input | Delete-progress hide and continuing-job semantics | ui:delete-progress,msg:any key hide (deletion continues),ui:deletion-progress-dialog,mode:delete-progress | INV-INPUT-011; INV-files-ops-012 | F-006,F-010 | 2 | NOT_STARTED | - | - | - |
| F-132 | input | Multi-selection info close and traversal semantics | mode:multi-selection-info,ui:multi-selection-info-dialog,msg:any key close | INV-INPUT-011 | F-003,F-004,F-010 | 2 | NOT_STARTED | - | - | - |
| F-133 | rendering | Rename, path, and create prompt widgets | ui:new-entry-dialog,ui:goto-path-dialog,ui:rename-dialog | INV-RENDER-007; INV-INPUT-005/006/007 | F-010,F-118 | 2 | NOT_STARTED | - | - | - |
| F-134 | input | Theme preset cycle action | key:normal:backslash,msg:Switched to | INV-INPUT-003; INV-CONFIG-005 | F-003,F-008 | 2 | NOT_STARTED | - | - | - |
| F-135 | input | Hidden-file visibility toggle action | key:normal:period,key:normal:. | INV-INPUT-003; INV-STATE-004 | F-003,F-004,F-005,F-009 | 2 | NOT_STARTED | - | - | - |
| F-136 | input | Sort-cycle action binding | key:normal:comma | INV-INPUT-003; INV-files-ops-002 | F-003,F-004,F-009 | 2 | NOT_STARTED | - | - | - |
| F-137 | input | Split-pane toggle action | key:normal:+,key:normal:plus | INV-INPUT-003; INV-STATE-001 | F-003,F-004,F-009 | 2 | NOT_STARTED | - | - | - |
| F-138 | input | Pane-focus Tab action | key:normal:Tab,key:normal:tab | INV-INPUT-003; INV-STATE-001 | F-003,F-004,F-009 | 2 | NOT_STARTED | - | - | - |
| F-139 | input | Clear confirmed search filter action | key:normal:escape,key:normal:Esc | INV-INPUT-003; INV-STATE-SEARCH-008 | F-003,F-004,F-009 | 2 | NOT_STARTED | - | - | - |
| F-140 | input | Drive shortcut selection | key:normal:drive-digit-1-9 | INV-INPUT-002 | F-003,F-004,F-014 | 2 | NOT_STARTED | - | - | - |
| F-141 | input | Common-folder and bookmark destination shortcut dispatch | key:normal:w,key:normal:e,key:normal:y,key:normal:u,key:normal:t,key:normal:common-folder-shortcut,key:normal:i,key:normal:r,key:normal:bookmark-shortcut | INV-INPUT-002; INV-PLAT-003; INV-CONFIG-004 | F-003,F-004,F-007,F-014 | 2 | NOT_STARTED | - | - | - |
| F-142 | config | Bookmark shortcut allocation and exhaustion | msg:No free bookmark shortcut available (a-p are taken) | INV-CONFIG-004 | F-004,F-007 | 1 | NOT_STARTED | - | - | - |
| F-143 | input | Select-all and clear-all action aliases | key:normal:alt+a,key:normal:ctrl-a,key:normal:super+a,key:normal:alt-super-a | INV-INPUT-004 | F-003,F-004,F-009 | 2 | NOT_STARTED | - | - | - |
| F-144 | input | Invert-selection action aliases | key:normal:alt-super-i,key:normal:super+i,key:normal:alt-i,key:normal:alt+i | INV-INPUT-004 | F-003,F-004,F-009 | 2 | NOT_STARTED | - | - | - |
| F-145 | input | Move-to-other-pane action binding | key:normal:m | INV-INPUT-004 | F-003,F-004,F-005,F-006,F-009 | 2 | NOT_STARTED | - | - | - |
| F-146 | input | Preview text-editor save action | key:editor:ctrl-s:save-preview-text,key:preview-text-edit:ctrl-s | INV-INPUT-012; INV-PE-003 | F-005,F-006,F-010,F-011 | 3 | NOT_STARTED | - | - | - |
| F-147 | rendering | Details-view columns and relative-time formatting | ui:files-details-view,ui:details-listing,ui:details-relative-time | INV-RENDER-003; INV-files-ops-004 | F-008,F-009 | 2 | NOT_STARTED | - | - | - |
| F-148 | rendering | Grid-view row composition | ui:files-grid-view | INV-RENDER-003 | F-008,F-009 | 2 | NOT_STARTED | - | - | - |
| F-149 | platform | Native quit shortcuts | - | desktop UX floor; ux-keymodel.md | F-011,F-034,F-115 | 2 | NOT_STARTED | - | - | - |
| F-150 | foundation | Migration CI builds both locked manifests and replays oracle scenarios on Linux, macOS and Windows with native smoke evidence | - | .github/workflows/**; platform-ci.md | F-001,F-002 | 0 | NOT_STARTED | integrator | - | - |
| F-151 | input | Selected-entry information action | key:normal:?,key:normal:question-mark | INV-INPUT-003 | F-003,F-004,F-009,F-010 | 2 | NOT_STARTED | - | - | - |
