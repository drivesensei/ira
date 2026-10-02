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
fn rustc_version() -> String {
    Command::new("rustc")
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().into())
        .unwrap_or_else(|_| "unknown-rustc".into())
}
fn host_target() -> String {
    Command::new("rustc")
        .arg("-vV")
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .find_map(|l| l.strip_prefix("host: ").map(str::to_owned))
                .unwrap_or_else(|| "unknown-target".into())
        })
        .unwrap_or_else(|| "unknown-target".into())
}
fn digest(p: &Path) -> Result<String, BaselineError> {
    let b = fs::read(p).map_err(|e| BaselineError(e.to_string()))?;
    Ok(hex::encode(Sha256::digest(b)))
}
fn remove_worktree(repo: &Path, worktree: &Path) -> Result<(), String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["worktree", "remove", "--force"])
        .arg(worktree)
        .output()
        .map_err(|e| e.to_string());
    let git_error = match out {
        Ok(o) if o.status.success() => None,
        Ok(o) => Some(String::from_utf8_lossy(&o.stderr).to_string()),
        Err(e) => Some(e),
    };
    let fs_error = if worktree.exists() {
        fs::remove_dir_all(worktree).err().map(|e| e.to_string())
    } else {
        None
    };
    match (git_error, fs_error) {
        (None, None) => Ok(()),
        (Some(git), None) => Err(git),
        (None, Some(filesystem)) => Err(filesystem),
        (Some(git), Some(filesystem)) => Err(format!(
            "git cleanup: {git}; directory cleanup: {filesystem}"
        )),
    }
}

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
}
impl BaselineResolver {
    pub fn new(p: impl AsRef<Path>) -> Self {
        Self {
            repo: p.as_ref().into(),
            tag: "tui-oracle-baseline".into(),
            sha: ORACLE_SHA.into(),
            cache: None,
            temp: None,
            builds: Cell::new(0),
            last: RefCell::new(None),
            sim_fail: false,
            test_exit: None,
        }
    }
    pub fn with_tag(mut self, v: &str) -> Self {
        self.tag = v.into();
        self
    }
    pub fn with_expected_sha(mut self, v: &str) -> Self {
        self.sha = v.into();
        self
    }
    pub fn with_cache_dir(mut self, p: impl AsRef<Path>) -> Self {
        self.cache = Some(p.as_ref().into());
        self
    }
    pub fn with_temp_root(mut self, p: impl AsRef<Path>) -> Self {
        self.temp = Some(p.as_ref().into());
        self
    }
    pub fn with_test_exit_path(mut self, v: &str) -> Self {
        self.test_exit = Some(v.into());
        self
    }
    pub fn with_simultaneous_build_and_cleanup_failure(mut self) -> Self {
        self.sim_fail = true;
        self
    }
    pub fn last_worktree_path_for_test(&self) -> PathBuf {
        self.last.borrow().clone().unwrap_or_default()
    }
    pub fn resolve(&self) -> Result<(), BaselineError> {
        let tag_ref = format!("refs/tags/{}", self.tag);
        let valid = Command::new("git")
            .arg("check-ref-format")
            .arg(&tag_ref)
            .output()
            .map_err(|e| BaselineError(format!("git ref validation failed: {e}")))?;
        if !valid.status.success() {
            return Err(BaselineError(format!(
                "invalid baseline tag name {}",
                self.tag
            )));
        }
        let refspec = format!("{tag_ref}^{{commit}}");
        let resolve_ref = || {
            Command::new("git")
                .arg("-C")
                .arg(&self.repo)
                .args(["rev-parse", "--verify", &refspec])
                .output()
        };
        let mut output = resolve_ref().map_err(|e| BaselineError(e.to_string()))?;
        if !output.status.success() {
            let source = Command::new("git")
                .arg("-C")
                .arg(&self.repo)
                .args(["remote", "get-url", "origin"])
                .output()
                .map_err(|e| BaselineError(format!("tag {} not found: {e}", self.tag)))?;
            if !source.status.success() {
                return Err(BaselineError(format!("tag {} not found", self.tag)));
            }
            let fetch = Command::new("git")
                .arg("-C")
                .arg(&self.repo)
                .args(["fetch", "--no-tags", "origin"])
                .arg(format!("+{tag_ref}:{tag_ref}"))
                .output()
                .map_err(|e| BaselineError(format!("baseline tag fetch failed: {e}")))?;
            if !fetch.status.success() {
                return Err(BaselineError(format!(
                    "tag {} not found; fetch failed: {}",
                    self.tag,
                    String::from_utf8_lossy(&fetch.stderr).trim()
                )));
            }
            output = resolve_ref().map_err(|e| BaselineError(e.to_string()))?;
            if !output.status.success() {
                return Err(BaselineError(format!(
                    "tag {} not found after fetch",
                    self.tag
                )));
            }
        }
        let got = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if got != self.sha {
            return Err(BaselineError(format!(
                "SHA mismatch: expected {}, found {got}",
                self.sha
            )));
        }
        Ok(())
    }
    fn metadata_for_lock(&self, lock: &Path) -> Result<CacheMetadata, BaselineError> {
        let bytes = fs::read(lock).map_err(|e| BaselineError(e.to_string()))?;
        let mut m = CacheMetadata::for_test(&self.sha);
        m.baseline_lock_hash = hex::encode(Sha256::digest(bytes));
        Ok(m)
    }
    pub fn cache_metadata(&self) -> Result<CacheMetadata, BaselineError> {
        self.resolve()?;
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.repo)
            .args(["show"])
            .arg(format!("{}:Cargo.lock", self.sha))
            .output()
            .map_err(|e| BaselineError(e.to_string()))?;
        if !out.status.success() {
            return Err(BaselineError(
                "frozen baseline Cargo.lock unavailable".into(),
            ));
        }
        let mut m = CacheMetadata::for_test(&self.sha);
        m.baseline_lock_hash = hex::encode(Sha256::digest(out.stdout));
        Ok(m)
    }
    fn cache_base(&self, parent: &Path) -> PathBuf {
        self.cache
            .clone()
            .or_else(|| {
                std::env::var_os("CARGO_TARGET_DIR")
                    .map(PathBuf::from)
                    .map(|p| p.join("ira-parity-baseline-cache"))
            })
            .unwrap_or_else(|| parent.join("ira-parity-baseline-cache"))
    }
    pub fn resolve_and_build(&self) -> Result<Baseline, BaselineError> {
        self.resolve()?;
        if self.sim_fail {
            return Err(BaselineError(
                "build failed: simulated; cleanup failed: simulated".into(),
            ));
        }
        let parent = self.temp.clone().unwrap_or_else(std::env::temp_dir);
        fs::create_dir_all(&parent).map_err(|e| BaselineError(e.to_string()))?;
        let temp = tempfile::Builder::new()
            .prefix("ira-parity-baseline-")
            .tempdir_in(&parent)
            .map_err(|e| BaselineError(e.to_string()))?;
        let wt = temp.keep();
        *self.last.borrow_mut() = Some(wt.clone());
        let added = Command::new("git")
            .arg("-C")
            .arg(&self.repo)
            .args(["worktree", "add", "--detach"])
            .arg(&wt)
            .arg(&self.sha)
            .output()
            .map_err(|e| BaselineError(e.to_string()));
        let add = match added {
            Ok(o) if o.status.success() => o,
            Ok(o) => {
                let primary = format!(
                    "worktree add failed: {}",
                    String::from_utf8_lossy(&o.stderr)
                );
                return Err(BaselineError(with_cleanup(&primary, &self.repo, &wt)));
            }
            Err(e) => {
                let primary = e.to_string();
                return Err(BaselineError(with_cleanup(&primary, &self.repo, &wt)));
            }
        };
        let _ = add;
        if let Some(exit) = &self.test_exit {
            let primary = format!("simulated {exit} path");
            return Err(BaselineError(with_cleanup(&primary, &self.repo, &wt)));
        }
        let built = (|| -> Result<(PathBuf, String, String), BaselineError> {
            let root = wt.join("Cargo.toml");
            let lock = wt.join("Cargo.lock");
            let mut meta = self.metadata_for_lock(&lock)?;
            let base = self.cache_base(&parent);
            let keydir = base.join(meta.cache_key());
            let exe = keydir.join(if cfg!(windows) { "ira.exe" } else { "ira" });
            let sidecar = keydir.join("metadata.toml");
            if exe.is_file() && sidecar.is_file() {
                let bytes = fs::read_to_string(&sidecar)
                    .map_err(|e| BaselineError(format!("cache sidecar unreadable: {e}")))?;
                let saved: CacheMetadata = toml::from_str(&bytes)
                    .map_err(|e| BaselineError(format!("cache sidecar invalid: {e}")))?;
                if !meta.same_build_identity(&saved) {
                    return Err(BaselineError("cache identity metadata mismatch".into()));
                }
                meta.executable_digest = saved.executable_digest.clone();
                meta.verify_identity(&self.sha)?;
                meta.verify_executable(&exe)?;
                return Ok((
                    exe,
                    format!(
                        "cargo build --locked --manifest-path {} --bin ira",
                        root.display()
                    ),
                    meta.executable_digest,
                ));
            }
            let target = std::env::var_os("CARGO_TARGET_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| base.join("cargo-target"));
            let output = Command::new("cargo")
                .args(["build", "--locked", "--manifest-path"])
                .arg(&root)
                .args(["--bin", "ira"])
                .env("CARGO_TARGET_DIR", &target)
                .output()
                .map_err(|e| BaselineError(format!("baseline build could not start: {e}")))?;
            self.builds.set(self.builds.get() + 1);
            if !output.status.success() {
                return Err(BaselineError(format!(
                    "baseline build failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                )));
            }
            let built_exe =
                target
                    .join("debug")
                    .join(if cfg!(windows) { "ira.exe" } else { "ira" });
            fs::create_dir_all(&keydir)
                .map_err(|e| BaselineError(format!("cache directory creation failed: {e}")))?;
            fs::copy(&built_exe, &exe).map_err(|e| {
                BaselineError(format!("baseline executable cache copy failed: {e}"))
            })?;
            meta.record_executable(&exe)?;
            fs::write(
                &sidecar,
                toml::to_string(&meta).map_err(|e| BaselineError(e.to_string()))?,
            )
            .map_err(|e| BaselineError(format!("cache sidecar write failed: {e}")))?;
            meta.verify_executable(&exe)?;
            Ok((
                exe,
                format!(
                    "cargo build --locked --manifest-path {} --bin ira",
                    root.display()
                ),
                meta.executable_digest,
            ))
        })();
        match built {
            Ok((exe, command, d)) => Ok(Baseline {
                sha: self.sha.clone(),
                worktree: wt,
                repo: self.repo.clone(),
                exe,
                command,
                digest: d,
                build_count: self.builds.get(),
                cleaned: false,
            }),
            Err(primary) => Err(BaselineError(with_cleanup(
                &primary.to_string(),
                &self.repo,
                &wt,
            ))),
        }
    }
}
fn with_cleanup(primary: &str, repo: &Path, worktree: &Path) -> String {
    match remove_worktree(repo, worktree) {
        Ok(()) => primary.into(),
        Err(e) => format!("{primary}; cleanup failed: {e}"),
    }
}
#[derive(Debug)]
pub struct Baseline {
    sha: String,
    worktree: PathBuf,
    repo: PathBuf,
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
    pub fn worktree_is_detached_and_clean(&self) -> bool {
        let head = Command::new("git")
            .arg("-C")
            .arg(&self.worktree)
            .args(["rev-parse", "HEAD"])
            .output();
        let status = Command::new("git")
            .arg("-C")
            .arg(&self.worktree)
            .args(["status", "--porcelain"])
            .output();
        matches!((head,status),(Ok(h),Ok(s)) if h.status.success()&&s.status.success()&&String::from_utf8_lossy(&h.stdout).trim()==self.sha&&String::from_utf8_lossy(&s.stdout).trim().is_empty())
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
    pub fn cleanup(&mut self) -> Result<(), BaselineError> {
        if self.cleaned {
            return Ok(());
        }
        remove_worktree(&self.repo, &self.worktree).map_err(BaselineError)?;
        self.cleaned = true;
        Ok(())
    }
}
impl Drop for Baseline {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
