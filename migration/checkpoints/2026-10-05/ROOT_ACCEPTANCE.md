# Root acceptance checkpoint — 2026-10-05

Source: `9634fbcc23b55e35b85833387718867f8b029984`, canonical branch `migrate/desktop-parity-checkpoints`. Continue in this existing worktree with its existing target only; [single-worktree policy](../../SINGLE_WORKTREE.md) applies. The [regression checkpoint](REGRESSION.md) records 40 distinct focused passing bodies. Overall migration parity remains incomplete.

## Local root compile: blocked, zero new bodies

Two bounded root `cargo test --offline --locked --bin ira --no-run` attempts did not complete. The first stopped on a monitor FileNotFoundError whose missing path was not captured; its cause remains unknown. A narrowly reviewed accounting correction then tolerated only disappearing generated inner entries, retaining strict root/ancestor and other error guards. The corrected attempt stopped at its approved storage growth threshold. Neither attempt produced a compiler verdict, so neither reproduces nor closes the historical duplicate-App compile gap.

Both owned Cargo processes were terminated and reaped. Zero of the four root save/cleanup bodies or five root real-worker settlement bodies ran. All 135 pinned source files remained unchanged. Root library test-harness compilation, integrated local desktop test compilation, native runtime and aggregate/full-matrix acceptance remain UNRUN. The additional root App/occupancy candidates require a coherent linked library first and a separate bounded execution grant; they are not passing evidence.

The retained partial output charge is 155,222,016 bytes across both attempts; total active elapsed is 47.838 seconds. Final target charge was 322,240,512 bytes. The corrected receipt recorded 15,467,487,232 bytes physically free, but only 54,283,042 bytes (about 51.8 MiB) remained under the cumulative 10 GiB cap after the separate 500 MiB reserve, before subsequent control-file writes. Physical free space therefore does not supply the required allocation headroom. Substantial further root/desktop test compilation requires an explicit additional resource decision and fresh accounting/grant. Do not retry, create another target/cache/worktree, or delete retained proof opportunistically.

Local-only evidence: `R031/acceptance-phase1-result.json`, `R031/acceptance-continuation-result.json`, and `R032-integrated-acceptance-review.md` in the task evidence/report workspace. Continuation receipt SHA256 `65788273d3307ed624381346e203665b54d8e8df4b504bb9dd5093c52a2fe789`; its 1,266-byte dependency-only log SHA256 `fd1c4e5edbfd33ef846f27df64b9e26ea2f6cdac9af9ef3f72daf896e17fb62a`. Those receipts and fixture logs remain local and are not shipped here. Sampled accounting is not an atomic inventory or kernel allocation guarantee.

## Publication and CI at this source

The user manually published through `9634fbc`. A saved read-only remote observation confirms migration HEAD `9634fbcc23b55e35b85833387718867f8b029984` and unchanged main `25ffceb165281baa382aea46616917e7b6aa2789`. [PR 9](https://github.com/drivesensei/ira/pull/9) is open and regular, with that head. This closes publication through that SHA only; future agent publication remains paused. This new checkpoint is a subsequent local documentation delta unless separately published.

The saved exact-head PR observation records both macOS and Windows desktop binary checks SUCCESS in [run 37253185820](https://github.com/drivesensei/ira/actions/runs/37253185820), completed at 01:54:22 and 01:56:11 UTC respectively on 2026-10-05. These are production build/package/upload checks, not execution of local desktop test bodies or native runtime/parity tests. Local-only provenance: `R033/manual-push-remote-verification.json`, `R033/pr9-current.json`, `R033/workflow-run.json`, `R033/macos-job.json` and `R033/windows-job.json`; CI success does not convert the stopped local root compile into a passing test harness.

## Remaining acceptance and access dependencies

Historical R011 seven variant builds belong to an earlier original/candidate/mutant causal experiment, not seven automatic prerequisite rebuilds for the integrated source. Preserve its failed history: native AppKit behavior, real settlement and lost-receipt/error-priority acceptance, mutation sensitivity and authentication obligations remain open. The current stops do not explain historical failures or waive those obligations.

Dependabot alert 1 returned HTTP 403 through the existing authorized API account. Package, advisory, affected/fixed versions and migration relevance are unknown; access denial is not a code finding. No authentication scopes, credentials, dependencies or security settings were changed, and no broader security investigation was performed. Saved local-only receipt: `R033/dependabot-alert-1-access.json`. An authorized access decision is needed before acting on that specific alert.

Next: resolve the compiler resource allocation dependency, complete bounded root binary/library acceptance, then desktop/native verification under fresh safety/budget gates, and aggregate/full-matrix acceptance. Keep real user configuration intact. No app window, live TTY, real data/configuration, new cache/target/worktree, cleanup or push was performed by these acceptance attempts.
