# Distribution Plan — GitHub Releases, Homebrew, winget, Linux packages

Planning for distributing IRA (Rust/ratatui TUI file manager) to Windows, macOS, and Linux testers.

## Current state

- Single `Cargo.toml` crate, `ira` 0.1.0. Cross-platform code already in place:
  - `src/services/drives.rs` — macOS `/Volumes` enumeration, Windows drive letters (`winapi`), Linux `lsblk` + `udisksctl`.
  - `src/services/windows_drives_labels.rs` — `GetVolumeInformationW` (windows-only).
  - `src/app.rs` `open_file` — linux-only `gio` path.
- Linux extra deps are only for the drive panel: `lsblk` (util-linux), `udisks2`. Absent elsewhere → degrade gracefully; packages should list them as optional/recommended deps.
- `.github/workflows/release.yml` is broken and will be replaced, not patched:
  - `actions-rs/toolchain@v1` — archived.
  - `actions/upload-artifact@v3` — retired; workflow fails today.
  - `create_release` globs `*.tar.gz` but the runner never downloaded artifacts (missing `download-artifact` step).

## Approach: cargo-dist

[cargo-dist](https://opensource.axo.dev/cargo-dist/) generates the release pipeline from a `dist.toml`:
build matrix, `.tar.xz`/`.zip` archives, `SHA256SUMS`, shell + PowerShell install scripts, GitHub Release upload, and automatic Homebrew formula publishing to a tap.

## Release targets

| Target | Runner | Notes |
|---|---|---|
| `x86_64-unknown-linux-gnu` | `ubuntu-22.04` | build on 22.04 for glibc 2.35 max, runs on newer distros |
| `aarch64-unknown-linux-gnu` | `ubuntu-22.04` (cross) | RPi/ARM servers |
| `x86_64-apple-darwin` + `aarch64-apple-darwin` | `macos-14` | optional universal (lipo) binary |
| `x86_64-pc-windows-msvc` | `windows-latest` | `.zip` with `ira.exe` |

## Release source

Releases are cut from whatever commit is tagged, not automatically from master. Convention:

- Only tag releases on `master` (`git describe --tags` must match the released artifact; cargo-dist validates the tag against `Cargo.toml` `version` at the tagged commit).
- Flow: merge feature branches → `master` → bump version → tag `v*.*.*` → push.
- Prereleases: tag `v*.*.*-rc.N` directly on a feature branch (e.g. `update-dependencies`) to let testers try it before merging; cargo-dist marks these as prereleases and they never become the "latest" release.

## Distribution channels (rollout order)

1. **GitHub Releases** (day 1) — tag `v*.*.*` → binaries + checksums + `curl | sh` / PowerShell one-liners. No accounts, no review.
2. **Homebrew tap** (day 1, same pipeline) — personal tap repo `homebrew-ira`; users: `brew install <you>/ira/ira`. cargo-dist pushes updated formula + sha256 on every tag (needs `HOMEBREW_TAP_TOKEN` PAT secret). homebrew-core requires notable popularity; a tap is instant with identical UX.
3. **crates.io** (day 1) — `cargo install ira`. Prereq: `Cargo.toml` needs `description` and `repository` fields (crates.io rejects missing description; repository is expected).
4. **winget** (after 2–3 releases settle) — manifest PR to `microsoft/winget-pkgs` via `wingetcreate`, pointing at the versioned GitHub release zip + SHA256. ID like `VladimirLopez.IRA` (bare `ira` will collide). Portable package type (zip + exe) is right for a TUI — no MSI. Automatable in CI with fork + PAT.
5. **AUR** (parallel with winget) — `ira` (builds from tag) and `ira-bin` (release binary). Needs AUR account + deploy SSH key secret. Optional.

## Known risks (deferred)

- **macOS notarization** — unsigned browser-downloaded binaries hit Gatekeeper; `curl`/`brew` installs don't get the quarantine attribute. Defer Apple Developer ID ($99/yr) + notarytool until users complain; document `xattr -d com.apple.quarantine` workaround in release notes.
- **Windows SmartScreen** — unsigned `.exe` shows "unknown publisher". Cosmetic for a CLI; Authenticode cert deferred.
- **Version discipline** — tag must match `Cargo.toml` version (cargo-dist enforces). Flow: bump version → commit → tag → push.

## Phases

1. **Phase 1 — pipeline**: `dist init`; add `description`/`repository` to Cargo.toml; replace `release.yml`; tag `v0.1.0`; verify all assets + checksums land on the release.
2. **Phase 2 — Homebrew tap**: create tap repo, add `HOMEBREW_TAP_TOKEN` PAT secret; formula auto-published by dist.
3. **Phase 3 — crates.io**: publish token secret; add `cargo publish` step (or run manually per release).
4. **Phase 4 — winget + AUR**: manifests/PKGBUILDs, per-release automation.
5. **Phase 5 (deferred)**: Apple notarization, Windows Authenticode.

Verification per phase: fresh-VM smoke tests in CI — `brew install` on a macOS runner, `winget install` on a Windows runner, tarball extract + run on Ubuntu — confirm the TUI launches and the drive panel degrades cleanly where udisks is absent.

## Account/secret prerequisites (owner-only)

- GitHub PAT with repo scope → `HOMEBREW_TAP_TOKEN` secret.
- Tap repo `homebrew-ira` under the account.
- crates.io token → `CARGO_REGISTRY_TOKEN` secret.
- winget: fork of `microsoft/winget-pkgs` + PAT; AUR account + SSH key.
- Later: Apple Developer ID, Windows code-signing cert.
