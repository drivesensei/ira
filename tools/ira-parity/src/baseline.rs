use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    cell::{Cell, RefCell},
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use thiserror::Error;
pub const ORACLE_SHA: &str = "1cad4ce43cc72d52d4cc4eef920e0da22cb69568";
#[derive(Debug, Error)]
#[error("baseline {0}")]
pub struct BaselineError(String);
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CacheMetadata {
    pub baseline_sha: String,
    pub os: String,
    pub target_triple: String,
    pub rust_toolchain: String,
    pub baseline_lock_hash: String,
    pub build_profile: String,
    pub executable_digest: String,
}
impl CacheMetadata {
    pub fn for_test(sha: &str) -> Self {
        Self {
            baseline_sha: sha.into(),
            os: std::env::consts::OS.into(),
            target_triple: host_target(),
            rust_toolchain: rustc_version(),
            baseline_lock_hash: String::new(),
            build_profile: "debug".into(),
            executable_digest: String::new(),
        }
    }
    pub fn has_field(&self, n: &str) -> bool {
        [
            "baseline_sha",
            "os",
            "target_triple",
            "rust_toolchain",
            "baseline_lock_hash",
            "build_profile",
        ]
        .contains(&n)
    }
    pub fn cache_key_with_os(&self, os: &str) -> String {
        format!(
            "{}-{os}-{}-{}-{}-{}",
            self.baseline_sha,
            self.target_triple,
            self.rust_toolchain,
            self.baseline_lock_hash,
            self.build_profile
        )
    }
    fn cache_key(&self) -> String {
        self.cache_key_with_os(&self.os)
    }
    pub fn record_executable(&mut self, p: impl AsRef<Path>) -> Result<(), BaselineError> {
        self.executable_digest = digest(p.as_ref())?;
        Ok(())
    }
    pub fn verify_executable(&self, p: impl AsRef<Path>) -> Result<(), BaselineError> {
        if digest(p.as_ref())? == self.executable_digest {
            Ok(())
        } else {
            Err(BaselineError("executable digest mismatch".into()))
        }
    }
    pub fn verify_identity(&self, sha: &str) -> Result<(), BaselineError> {
        if self.baseline_sha == sha {
            Ok(())
        } else {
            Err(BaselineError("SHA identity mismatch".into()))
        }
    }
    fn same_build_identity(&self, other: &Self) -> bool {
        self.baseline_sha == other.baseline_sha
            && self.os == other.os
            && self.target_triple == other.target_triple
            && self.rust_toolchain == other.rust_toolchain
            && self.baseline_lock_hash == other.baseline_lock_hash
            && self.build_profile == other.build_profile
    }
}
fn probe(args: &[&str]) -> Option<String> {
    crate::source_export::output(
        Command::new("rustc").args(args),
        std::time::Instant::now() + std::time::Duration::from_secs(5),
        16384,
    )
    .ok()
    .filter(|o| o.status.success())
    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
}
fn rustc_version() -> String {
    probe(&["--version"]).unwrap_or_else(|| "unknown-rustc".into())
}
fn host_target() -> String {
    probe(&["-vV"])
        .and_then(|o| {
            o.lines()
                .find_map(|l| l.strip_prefix("host: ").map(str::to_owned))
        })
        .unwrap_or_else(|| "unknown-target".into())
}
fn digest(p: &Path) -> Result<String, BaselineError> {
    fs::read(p)
        .map(|b| hex::encode(Sha256::digest(b)))
        .map_err(|e| BaselineError(e.to_string()))
}
/// Explicit caller-approved oracle build. No build occurs by default.
#[derive(Clone, Debug)]
pub struct OracleBuildPolicy {
    pub deadline: std::time::Duration,
    pub output_limit: usize,
}
#[derive(Debug)]
pub struct BaselineResolver {
    repo: PathBuf,
    tag: String,
    sha: String,
    cache: Option<PathBuf>,
    temp: Option<PathBuf>,
    builds: Cell<u32>,
    last: RefCell<Option<PathBuf>>,
    sim_fail: bool,
    test_exit: Option<String>,
    build_policy: Option<OracleBuildPolicy>,
}
impl BaselineResolver {
    /// Original red-contract constructor; same local-only resolver.
    pub fn new(p: impl AsRef<Path>) -> Self {
        Self::from_manifest_dir(p)
    }
    /// Original cache-directory spelling; containment policy is unchanged.
    pub fn with_cache_dir(self, p: impl AsRef<Path>) -> Self {
        self.with_cache_root(p)
    }
    pub fn from_manifest_dir(p: impl AsRef<Path>) -> Self {
        let manifest = p.as_ref();
        let repo = manifest
            .ancestors()
            .find(|a| a.join(".git").exists())
            .unwrap_or(manifest)
            .to_owned();
        Self {
            repo,
            tag: "tui-oracle-baseline".into(),
            sha: ORACLE_SHA.into(),
            cache: None,
            temp: None,
            builds: Cell::new(0),
            last: RefCell::new(None),
            sim_fail: false,
            test_exit: None,
            build_policy: None,
        }
    }
    pub fn with_tag(mut self, tag: &str) -> Self {
        self.tag = tag.into();
        self
    }
    pub fn with_expected_sha(mut self, sha: &str) -> Self {
        self.sha = sha.into();
        self
    }
    pub fn with_cache_root(mut self, p: impl AsRef<Path>) -> Self {
        self.cache = Some(p.as_ref().into());
        self
    }
    pub fn with_temp_root(mut self, p: impl AsRef<Path>) -> Self {
        self.temp = Some(p.as_ref().into());
        self
    }
    pub fn with_build_policy(mut self, p: OracleBuildPolicy) -> Self {
        self.build_policy = Some(p);
        self
    }
    pub fn with_test_exit_path(mut self, p: &str) -> Self {
        self.test_exit = Some(p.into());
        self
    }
    pub fn with_simultaneous_build_and_cleanup_failure(mut self) -> Self {
        self.sim_fail = true;
        self
    }
    /// Historical spelling: a path to a retained export, never a registered worktree.
    pub fn last_worktree_path_for_test(&self) -> PathBuf {
        self.last.borrow().clone().unwrap_or_default()
    }
    pub fn last_source_export_path_for_test(&self) -> Option<PathBuf> {
        self.last.borrow().clone()
    }
    fn git(&self, args: &[&str], limit: usize) -> Result<Vec<u8>, BaselineError> {
        let o = crate::source_export::output(
            Command::new("git")
                .arg("--no-optional-locks")
                .arg("-C")
                .arg(&self.repo)
                .args(args),
            std::time::Instant::now() + std::time::Duration::from_secs(5),
            limit,
        )
        .map_err(BaselineError)?;
        if !o.status.success() {
            return Err(BaselineError(String::from_utf8_lossy(&o.stderr).into()));
        }
        Ok(o.stdout)
    }
    pub fn resolve(&self) -> Result<(), BaselineError> {
        if self.sha.len() != 40 || !self.sha.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(BaselineError("expected full commit SHA".into()));
        }
        self.git(
            &["check-ref-format", &format!("refs/tags/{}", self.tag)],
            4096,
        )?;
        let out = self
            .git(
                &[
                    "rev-parse",
                    "--verify",
                    &format!("refs/tags/{}^{{commit}}", self.tag),
                ],
                4096,
            )
            .map_err(|e| {
                BaselineError(format!(
                    "local pinned tag unavailable; implicit fetch disabled: {e}"
                ))
            })?;
        if String::from_utf8_lossy(&out).trim() != self.sha {
            return Err(BaselineError("SHA mismatch for local baseline tag".into()));
        }
        Ok(())
    }
    pub fn cache_metadata(&self) -> Result<CacheMetadata, BaselineError> {
        self.resolve()?;
        let lock = self.git(
            &["show", &format!("{}:Cargo.lock", self.sha)],
            8 * 1024 * 1024,
        )?;
        let mut m = CacheMetadata::for_test(&self.sha);
        m.baseline_lock_hash = hex::encode(Sha256::digest(lock));
        m.build_profile = "dev-debug0-incremental0".into();
        if m.rust_toolchain == "unknown-rustc" || m.target_triple == "unknown-target" {
            return Err(BaselineError("toolchain identity probe unavailable".into()));
        }
        Ok(m)
    }
    pub fn resolve_and_build(&self) -> Result<Baseline, BaselineError> {
        self.resolve()?;
        if self.sim_fail {
            return Err(BaselineError("build failed: simulated; cleanup failed: simulated historical seam (no files removed)".into()));
        }
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| BaselineError("explicit canonical CARGO_TARGET_DIR required".into()))?;
        let parent = self
            .temp
            .clone()
            .unwrap_or_else(|| target.join("ira-parity-exports"));
        let (root, manifest) =
            crate::source_export::export(&self.repo, &self.sha, &parent).map_err(BaselineError)?;
        *self.last.borrow_mut() = Some(root.clone());
        if let Some(exit) = &self.test_exit {
            return Err(BaselineError(format!(
                "simulated {exit}; source export retained at {}",
                root.display()
            )));
        }
        let mut meta = self.cache_metadata()?;
        let exported_lock = digest(&root.join("Cargo.lock"))?;
        if exported_lock != meta.baseline_lock_hash {
            return Err(BaselineError("export lock digest mismatch".into()));
        }
        let cache = self
            .cache
            .clone()
            .unwrap_or_else(|| target.join("ira-parity-baseline-cache"));
        crate::source_export::owned_target_child(&cache).map_err(BaselineError)?;
        let keydir = cache.join(meta.cache_key());
        crate::source_export::owned_target_child(&keydir).map_err(BaselineError)?;
        let exe = keydir.join(if cfg!(windows) { "ira.exe" } else { "ira" });
        let sidecar = keydir.join("metadata.toml");
        let command = format!(
            "cargo build --offline --locked --jobs 1 --manifest-path {} --bin ira",
            root.join("Cargo.toml").display()
        );
        if exe.exists() || sidecar.exists() {
            for p in [&exe, &sidecar] {
                let metadata = fs::symlink_metadata(p)
                    .map_err(|e| BaselineError(format!("incomplete cache: {e}")))?;
                if !metadata.is_file() || metadata.file_type().is_symlink() {
                    return Err(BaselineError(
                        "cache artifact must be a regular file".into(),
                    ));
                }
            }
            crate::source_export::owned_target_child(&keydir).map_err(BaselineError)?;
            let saved: CacheMetadata = toml::from_str(
                &fs::read_to_string(&sidecar).map_err(|e| BaselineError(e.to_string()))?,
            )
            .map_err(|e| BaselineError(e.to_string()))?;
            if !meta.same_build_identity(&saved) {
                return Err(BaselineError("cache identity metadata mismatch".into()));
            }
            meta.executable_digest = saved.executable_digest;
            meta.verify_executable(&exe)?;
        } else {
            let policy=self.build_policy.as_ref().ok_or_else(||BaselineError(format!("verified cache unavailable; explicit bounded oracle build policy required; export retained at {}",root.display())))?;
            if policy.deadline.is_zero()
                || policy.deadline > std::time::Duration::from_secs(180)
                || policy.output_limit == 0
                || policy.output_limit > 1024 * 1024
            {
                return Err(BaselineError(
                    "oracle build policy exceeds 180s/1MiB bounds".into(),
                ));
            }
            // Windows Cargo descendants need job containment before opt-in builds can be enabled.
            if cfg!(windows) {
                return Err(BaselineError(
                    "Windows oracle build containment pending; verified cache remains supported"
                        .into(),
                ));
            }
            let o = crate::source_export::output(
                Command::new("cargo")
                    .args([
                        "build",
                        "--offline",
                        "--locked",
                        "--jobs",
                        "1",
                        "--manifest-path",
                    ])
                    .arg(root.join("Cargo.toml"))
                    .args(["--bin", "ira"])
                    .env("CARGO_TARGET_DIR", &target)
                    .env("CARGO_INCREMENTAL", "0")
                    .env("CARGO_PROFILE_DEV_DEBUG", "0"),
                std::time::Instant::now() + policy.deadline,
                policy.output_limit,
            )
            .map_err(BaselineError)?;
            self.builds.set(self.builds.get() + 1);
            fs::write(root.join(".ira-parity-build.stdout"), &o.stdout)
                .map_err(|e| BaselineError(e.to_string()))?;
            fs::write(root.join(".ira-parity-build.stderr"), &o.stderr)
                .map_err(|e| BaselineError(e.to_string()))?;
            if !o.status.success() {
                return Err(BaselineError(format!(
                    "baseline build failed; export/logs retained at {}",
                    root.display()
                )));
            }
            crate::source_export::owned_target_child(&keydir).map_err(BaselineError)?;
            fs::create_dir_all(&keydir).map_err(|e| BaselineError(e.to_string()))?;
            let mut src = fs::File::open(target.join("debug").join(if cfg!(windows) {
                "ira.exe"
            } else {
                "ira"
            }))
            .map_err(|e| BaselineError(e.to_string()))?;
            let mut dst = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&exe)
                .map_err(|e| BaselineError(e.to_string()))?;
            std::io::copy(&mut src, &mut dst).map_err(|e| BaselineError(e.to_string()))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                dst.set_permissions(fs::Permissions::from_mode(0o755))
                    .map_err(|e| BaselineError(e.to_string()))?;
            }
            meta.record_executable(&exe)?;
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&sidecar)
                .and_then(|mut f| {
                    std::io::Write::write_all(&mut f, toml::to_string(&meta).unwrap().as_bytes())
                })
                .map_err(|e| BaselineError(e.to_string()))?;
        }
        Ok(Baseline {
            sha: self.sha.clone(),
            worktree: root,
            manifest,
            exe,
            command,
            digest: meta.executable_digest,
            build_count: self.builds.get(),
            cleaned: false,
        })
    }
}
#[derive(Debug)]
pub struct Baseline {
    sha: String,
    worktree: PathBuf,
    manifest: crate::source_export::Manifest,
    exe: PathBuf,
    command: String,
    digest: String,
    build_count: u32,
    cleaned: bool,
}
impl Baseline {
    pub fn commit_sha(&self) -> &str {
        &self.sha
    }
    /// Compatibility seam: this retained export is intentionally NOT a Git worktree.
    pub fn worktree_is_detached_and_clean(&self) -> bool {
        false
    }
    pub fn source_export_is_verified(&self) -> bool {
        crate::source_export::verified(&self.worktree, &self.manifest)
    }
    pub fn build_command(&self) -> &str {
        &self.command
    }
    pub fn worktree_path(&self) -> &Path {
        &self.worktree
    }
    pub fn executable_path(&self) -> &Path {
        &self.exe
    }
    pub fn executable_digest(&self) -> &str {
        &self.digest
    }
    pub fn build_count(&self) -> u32 {
        self.build_count
    }
    /// Retention replaces historical cleanup. No outputs, source or refs are deleted.
    pub fn cleanup(&mut self) -> Result<(), BaselineError> {
        self.cleaned = true;
        Ok(())
    }
}
