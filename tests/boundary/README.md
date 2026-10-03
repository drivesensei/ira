# Current F001 boundary verification

The exact historical seven-test starter contract is preserved at migration/historical/F-001-e346263. T030 explicitly approves replacing only starter S2/S6 cardinality/demo restrictions; current checks strengthen core isolation, alias/absolute path handling, unsafe exclusion, actual lifecycle wiring and exact approved source provenance. S1/S2 no longer duplicate S5 build invocations; S5 still runs both builds and every unchanged root test target.

Use /opt/homebrew/bin/python3.13 (tomllib is required) and /opt/homebrew/bin/cargo on this Mac. No system Python installation or global config change. GPUI0.2.2 stays pinned; approved runtime_shaders avoids the missing Metal compiler. Cargo runs locked/offline. run_builds creates synthetic HOME/XDG/TMPDIR under the native temporary directory, preserves native-length path stress, and isolates Git config/signing. It does not launch either app.

After coordinating exclusive target leases:

```
IRA_BOUNDARY_ROOT_TARGET=/tmp/ira-preview-target \
IRA_BOUNDARY_DESKTOP_TARGET=${HOME}/Projects/worktrees/ira-gpui-runtime/desktop/target \
/opt/homebrew/bin/python3.13 -m unittest discover -s tests/boundary -p 'test_*.py' -v
```

Environment target paths are cache selection only, not skips. S5 always runs actual root build --locked, desktop build --manifest-path desktop/Cargo.toml --locked --features gpui/runtime_shaders, and full root test --locked --no-fail-fast. Existing failures remain failed assertions. Dependencies/locks never update. Native-length G0011 clipping, the socket helper path limit and live-transfer assertions must be recorded; no short-TMPDIR passing result replaces them.

For a cheap structural-only report, select the seven named non-S5 F001 tests plus test_f001_mutations.py explicitly through unittest; record that S5 was not run. This is not a full-boundary/gate pass. Mutation tests alter synthetic fixture copies only, exercise the same checker and leave original/root/core/desktop source unchanged. Unknown root files or changed approved bytes fail. Future safety/editor integration requires exact reviewed blob/hash/regression provenance, never a path wildcard.

The 152-row matrix stays non-final absent final-SHA native evidence. Docs/surfaces correct source contradictions; they do not change runtime keybindings or normalize bookmark quirks. Existing mechanical gate must remain red until completion evidence exists.

The lexical helper `rust_source.py` decodes Rust normal-string escapes and raw literals (up to255 hashes) before resolving module `path` metadata, including nested `cfg_attr`. Comments and string contents are masked; conditional module paths are checked across target configurations. `test_rust_path_isolation.py` compiles/executes minimal Rust module probes before asserting external rejection and internal acceptance.

Reviewed-root promotion pins six exact source blobs (safety7191/editor9e) and five graph artifacts from runtime4940, with independent T033/T034 evidence. The previous d3e fixture remains in `migration/historical/T030-d3e46e5`. The default owned checkout is deliberately older and must fail current S6; it is never claimed green. For a clean read-only candidate run, set `IRA_BOUNDARY_ROOT` to its Git root and `IRA_BOUNDARY_EXPECTED_HEAD` to the exact selected revision; candidate HEAD and dirty-state checks remain mandatory. Versioned fixture/history stay anchored to this contract checkout. The override selects every actual root/core/desktop graph/source check and S5 build directory; it does not skip checks. Candidate checks require the frozen oracle tag/object history.

When sharing a Cargo target across same-named worktrees, use a fresh exact source archive and refresh only archive-file timestamps so source compilation is forced; record archive hash and compilation paths. A cached command exit alone does not establish exact-source proof. Parent owns the final integrated S5/native validation; structural source hashes do not prove native parity.

Source comparison reads actual src bytes/types against the frozen Git tree, including ignored/untracked files, rather than trusting index stat caches. Hidden assume-unchanged/skip-worktree entries are rejected in every contract mode and the candidate guard. Only disposable synthetic Git indexes are mutated by regressions.
