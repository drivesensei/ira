# Local IRA source branch map

LOCAL documentation delta, UNCOMMITTED. Frozen checkpoint e4f03b8fc23afa03b4f54d713d32e94d6c1d5111 remains unchanged; original HANDOFF is retained byte-for-byte at the task workspace evidence/R015/HANDOFF-committed-e4f03b8.md (SHA 256 7b6af0666aac0c6cdd4c06a719fa33632dfb0752a9d6f66a0f214674db907ebe). No new publication authorization applies to these files.

The complete machine-readable `SOURCE_BRANCH_MAP.json` enumerates77 refs (heads/cached remotes/peeled tags),26 local branches and26 worktrees,261 unique commits beyond main, every exact HEAD/merge-base/parent/source-only commit set and separate patch-equivalence diagnostics. It contains full paths and dependency hashes for local use. Cached remote refs are not fresh server observations. Ancestry establishes Git inclusion; separate patch-equivalent commits do not establish accepted integration or tests.

| Local branch | HEAD | Git relation to checkpoint | Exact commits not included |
|---|---|---|---:|
| codex/ira-application-quit | edc8b7eb56ca | DIVERGED_NOT_INTEGRATED | 4 |
| codex/ira-boundary-reconciliation | 536f7bea65fa | DIVERGED_NOT_INTEGRATED | 4 |
| codex/ira-compile-repair | fc8e7d799e3c | ANCESTOR_INCLUDED | 0 |
| codex/ira-core-foundation | 5dacb49b46a7 | DIVERGED_NOT_INTEGRATED | 1 |
| codex/ira-desktop-parity | 2211ed8d6912 | DIVERGED_NOT_INTEGRATED | 37 |
| codex/ira-fixture-repair | 641d0b8dc25e | DIVERGED_NOT_INTEGRATED | 4 |
| codex/ira-gpui-runtime | d889ba18c8a4 | ANCESTOR_INCLUDED | 0 |
| codex/ira-harness-completion | bfaa5fc52bcc | DIVERGED_NOT_INTEGRATED | 27 |
| codex/ira-harness-safety | e7cc2507776b | DIVERGED_NOT_INTEGRATED | 29 |
| codex/ira-model-parity | cdbbe16bdead | DIVERGED_NOT_INTEGRATED | 6 |
| codex/ira-native-accessibility | 4996d6a0d020 | DIVERGED_NOT_INTEGRATED | 10 |
| codex/ira-native-chooser | 6233a1f43cdf | DIVERGED_NOT_INTEGRATED | 2 |
| codex/ira-native-gate-repair | ef687945c8f4 | DIVERGED_NOT_INTEGRATED | 2 |
| codex/ira-native-input | b868a431c553 | DIVERGED_NOT_INTEGRATED | 7 |
| codex/ira-native-kvo-repair | dbaf487c227d | DIVERGED_NOT_INTEGRATED | 4 |
| codex/ira-native-validation | dd8f7b380a7c | DIVERGED_NOT_INTEGRATED | 3 |
| codex/ira-overwrite-safety | 7191d9d0314c | DIVERGED_NOT_INTEGRATED | 8 |
| codex/ira-persistence-safe | 6684f31e63a4 | DIVERGED_NOT_INTEGRATED | 45 |
| codex/ira-preview-editor | 8970234952df | DIVERGED_NOT_INTEGRATED | 5 |
| codex/ira-shutdown-surface-repair | 738407a9dee7 | DIVERGED_NOT_INTEGRATED | 1 |
| codex/ira-terminal-fixture-repair | cb9741569a60 | ANCESTOR_INCLUDED | 0 |
| codex/ira-terminal-settlement | 66e9550460ca | ANCESTOR_INCLUDED | 0 |
| codex/ira-transfer-refresh | 51b1f88115de | DIVERGED_NOT_INTEGRATED | 37 |
| codex/ira-work-settlement | 1d8a4be1135a | DIVERGED_NOT_INTEGRATED | 4 |
| main | 25ffceb16528 | ANCESTOR_INCLUDED | 0 |
| migrate/desktop-parity-checkpoints | e4f03b8fc23a | SAME_HEAD | 0 |

## Source and proof placement

Checkpoint contains 94 existing source commits through cb9741569a60a9629da25134efc4a034662b2e52, plus 8810376128fd116adf5b04be5fbcf791fb8c62a3 maintenance reference and e4f03b8fc23afa03b4f54d713d32e94d6c1d5111 documentation. Runtime d889ba18c8a46cd9ddf846f722c2d7944249b1a5 and terminal settlement66e9550460ca760a547473495880ce06d587c3e8 are ancestors included. R011 candidate is26eac6b0f462e9013f34efe83e3eb0a1d1d8533a plus its approved frozen overlay, NOT the current checkpoint source (which adds Info fixes09d9aad/cb974156). Do not substitute the checkpoint for the frozen R011 candidate.

Pending native KVO chain: 392a688b0d4c945c8aa1a64e41205f928382e7ef → ef687945c8f49bcd4ad54280422eaa68a16f6854 → dd8f7b380a7ca5756358851cfb0dd33339f7e4f0 → dbaf487c227d965b6127d0194bf959d8834a261f. Core settlement chain: 4bc2fa95b8ecba10ec4719cd1dcc44a248a3ad5d → abe60c7c7e18f91c3f572e7a6ca2b84143699de1 → 5fc1a2c9f39677c2ad37d90a7c93791830010110 → 1d8a4be1135afb47e9a98df94e2ac025321ab7e0. Both exact series remain unapplied under pending-source/. Core abe60c7 repeats the compile-drain fix already present as fc8e7d799e3c0f36a95ee17f8e28be021c63b267: independent integration must resolve that prerequisite without dropping needed settlement changes. No blind cherry-pick/merge authorized.

Parity 2211ed8d69129c71ef9e7600aa8e2a2ca7d4da1a diverges with 37 not-reachable commits, including permanently disabled publication schemas/executor work. Its152-row matrix snapshot is documentary provenance only, not integrated source. Other divergent feature branches have full unique/exclusive commit sets and patch-equivalence output in JSON; do not reapply already integrated equivalents or assume clean local branches are reviewed. Original 25 worktrees and new checkpoint worktree all remain present.

Historical proof source: R003/R009 target dbaf487; R007 original a976cb2dfa75b61bcd79f55bf7f57c332a73e2ed and 26eac6b candidate/mutant; R010 target cb974156/base 26eac6b; T103 target 26eac6b. Exactreport/index hash bindings are historical-evidence-index.json and repeated in machine map. T103 24 numerical bodies passed but resolver compiler TMPDIR deviated from its grant; preserve conformance failure. None proves this combined checkpoint or native/full parity.

## Required local execution material

Task engineering root: `/Users/vladimir/Documents/Codex/2026-10-03/task-3/.devteam`. Source safety dependencies, expected hashes and present/missing metadata are in JSON local_dependencies. Small file hashes are observations, not complete source authentication. Binary/source trees were not exhaustively rehashed for this handoff.

1. Authenticate controllers-v5/INDEX.md and six v5-manifest files, all 183 artifact entries,22 external anchors and eight complete expected-tree indices/shards. Source v3/v4 scaffolds/overlay/prerequisite/mutant full-preimage files under evidence/R008-scaffolds are required. Read reports/R-011-root-settlement-proof-plan.md and R-011-root-settlement-execution-contract.md; old resource values and the Q creation prose there are historical. Current Q `/private/tmp/r011-settlement-8lyocdh0` already exists; preserve it. Original archive expected SHA 1168c7d64db4fbc0d3d67b2314eb29eded6e3adeb82276bd5bf72b4ab75d4665; root receipt expected SHA 8dd7763530a361f17a412167292c1d829a01a1f6ea49c59e25f453482385280f. Metadata presence is not current archive/tree verification.

2. v5 runner SHA 8d1dcad225f3231edb4b2a4850aab8c79cc925ceccbb4c29f8edcc7fe5d47c46 is compile-only; source approved historically. Actual 7 builds/7 inventories/5 bodies are UNRUN. Inventory/body drivers are UNPREPARED. Child core callback evidence/R011/child_core_limit.py is source-only and UNWIRED; its owned preexec child PID/held FD/zero RLIMIT_CORE witness must be integrated and independently reviewed before loss body. Abort alone is not expected semantic failure.

3. SharedB `/private/tmp/t058-build-5fy5pogk` contains cache/artifact/closure receipts. Fresh exclusive compiler lease, exact tool/SDK/offline graph/profile/source/extern/binary closure and actual compiler freshness are required. Never infer acceptance from reused Cargo cache or old seals. R013 preserved-artifacts contains4890 current products;229 raw receipts are retained at `.devteam/evidence/R013/preserved-artifacts/canonical-receipts`, outside deletion targets. Historical15 byte-bound executable references had1 current match and14 pre-existing mismatches: overwritten old bytes are unavailable, not reconstructed. No claim all historical binary closure is available.

4. Source authentication remains blocked by failed lost-7 read (later diagnostic matched; failed actuals not captured), cancelled full auth and retained initial failures. Ledger archive source 72c664... passed 9 isolated independent tests, but the last authenticated G8 snapshot is 6,473 bytes, 19 events, 12 links, closed, rounds 3; fresh live authentication remains unresolved. latest wrapper preread failed before CLI, unknown cause (reports/R011-live-ledger-archive-stop.md and evidence/R011/live-ledger-history-archive-preread-failure.json). Preserve failure/lifecycle and doctor 5,000-byte limit; do not reinterpret it from earlier ctime hydration diagnostics. The new approved cleanup attempt timed out after 180 seconds during canonical receipt verification (last FD 70), before the unlink section, with no completed deletion receipt. Independent post-timeout audit found all 29,559 original file identities/sizes/mtime/mode/UID and both roots unchanged; no deletion/recovered space is claimed. Post-timeout content rehash was NOT RUN. The retry is exhausted; exact timeout/post-state/audit hashes are in JSON.

5. Cumulative 10 GiB/max(size,allocated) without hardlink discount,8 GiB start/6 GiB floor/500 MiB per-build reserve apply. Recount all 14 original roots plus R013 preservation and R015/new checkout; no estimated cleanup cap credit. Refresh processes read-only: old isolated desktop PID 76583 was absent, installed terminal 45034 separate. Preserve synthetic namespace `/private/tmp/ira-resumed-desktop-tej_knsi` and unresolved real-state zero-byte incident; no real config/cache/HOME tests, app restart, signals or Codex UI.

## Publication and next action

Two initial pushes were auto-review rejected before execution. New direct17:57 approval passed review, then actual Git HTTPS push exited128/403: permission to drivesensei/ira denied to vladimir-lopez-lls. Remote branch and draftPR remain absent; main unchanged. Keychain-backed Git accountwriteaccess is now independently evidenced by remote denial. User may provide repository-scoped authorized credentials or collaborator access; do not silently change global accounts/credentialhelpers or retry/fork/alternate connector. Expanded local files are uncommitted and require separate scope review before any later commit/publication.

Account inventory: default gh auth status, token lines filtered, reports only vladimir-lopez-lls active in keyring with HTTPS. No credential or account change occurred. If that is the intended publisher, drivesensei can grant repository Write collaborator access; otherwise a user-selected authorized publishing account requires a later normal sign-in action. No invitation, access request or auth operation was performed.
