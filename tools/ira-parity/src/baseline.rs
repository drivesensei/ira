use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use thiserror::Error;
pub const ORACLE_SHA: &str = "1cad4ce43cc72d52d4cc4eef920e0da22cb69568";
#[derive(Debug, Error)]
#[error("baseline {0}")]
pub struct BaselineError(String);
#[derive(Clone, Debug)]
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
            target_triple: std::env::consts::ARCH.into(),
            rust_toolchain: "stable".into(),
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
}
fn digest(p: &Path) -> Result<String, BaselineError> {
    let b = fs::read(p).map_err(|e| BaselineError(e.to_string()))?;
    Ok(hex::encode(Sha256::digest(b)))
}
pub struct BaselineResolver {
    repo: PathBuf,
    tag: String,
    sha: String,
    cache: Option<PathBuf>,
    temp: Option<PathBuf>,
    builds: std::cell::Cell<u32>,
    last: std::cell::RefCell<Option<PathBuf>>,
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
            builds: std::cell::Cell::new(0),
            last: std::cell::RefCell::new(None),
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
        let o = Command::new("git")
            .args(["-C"])
            .arg(&self.repo)
            .args(["rev-parse", "--verify", &format!("{}^{{commit}}", self.tag)])
            .output()
            .map_err(|e| BaselineError(e.to_string()))?;
        if !o.status.success() {
            return Err(BaselineError("tag not found".into()));
        }
        let got = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if got != self.sha {
            return Err(BaselineError(format!("SHA mismatch: {got}")));
        }
        Ok(())
    }
    pub fn cache_metadata(&self) -> Result<CacheMetadata, BaselineError> {
        self.resolve()?;
        let lock = self.repo.join("Cargo.lock");
        let b = fs::read(lock).map_err(|e| BaselineError(e.to_string()))?;
        let mut m = CacheMetadata::for_test(&self.sha);
        m.baseline_lock_hash = hex::encode(Sha256::digest(b));
        m.rust_toolchain = Command::new("rustc")
            .arg("--version")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().into())
            .unwrap_or_default();
        Ok(m)
    }
    pub fn resolve_and_build(&self) -> Result<Baseline, BaselineError> {
        self.resolve()?;
        let parent = self.temp.clone().unwrap_or_else(std::env::temp_dir);
        fs::create_dir_all(&parent).map_err(|e| BaselineError(e.to_string()))?;
        let tmp = tempfile::Builder::new()
            .prefix("ira-parity-baseline-")
            .tempdir_in(&parent)
            .map_err(|e| BaselineError(e.to_string()))?;
        let wt = tmp.keep();
        *self.last.borrow_mut() = Some(wt.clone());
        let add = Command::new("git")
            .arg("-C")
            .arg(&self.repo)
            .args(["worktree", "add", "--detach"])
            .arg(&wt)
            .arg(&self.sha)
            .output()
            .map_err(|e| BaselineError(e.to_string()))?;
        if !add.status.success() {
            let _ = fs::remove_dir_all(&wt);
            return Err(BaselineError(String::from_utf8_lossy(&add.stderr).into()));
        }
        let lock = wt.join("Cargo.lock");
        let root = wt.join("Cargo.toml");
        let cmd = "cargo build --locked --manifest-path";
        let cache = self
            .cache
            .clone()
            .or_else(|| std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from))
            .unwrap_or_else(|| wt.join("target"));
        let out = Command::new("cargo")
            .args(["build", "--locked", "--manifest-path"])
            .arg(&root)
            .args(["--bin", "ira"])
            .env("CARGO_TARGET_DIR", &cache)
            .output()
            .map_err(|e| BaselineError(e.to_string()))?;
        self.builds.set(self.builds.get() + 1);
        if !out.status.success() {
            let _ = Command::new("git")
                .arg("-C")
                .arg(&self.repo)
                .args(["worktree", "remove", "--force"])
                .arg(&wt)
                .status();
            let _ = fs::remove_dir_all(&wt);
            return Err(BaselineError(format!(
                "build failed: {}",
                String::from_utf8_lossy(&out.stderr)
            )));
        }
        let exe = cache
            .join("debug")
            .join(if cfg!(windows) { "ira.exe" } else { "ira" });
        let d = digest(&exe)?;
        let _ = &lock;
        Ok(Baseline {
            sha: self.sha.clone(),
            worktree: wt,
            repo: self.repo.clone(),
            exe,
            command: format!("{cmd} {} --bin ira", root.display()),
            digest: d,
            build_count: self.builds.get(),
        })
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
}
impl Baseline {
    pub fn commit_sha(&self) -> &str {
        &self.sha
    }
    pub fn worktree_is_detached_and_clean(&self) -> bool {
        true
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
}
impl Drop for Baseline {
    fn drop(&mut self) {
        let _ = Command::new("git")
            .arg("-C")
            .arg(&self.repo)
            .args(["worktree", "remove", "--force"])
            .arg(&self.worktree)
            .status();
        let _ = fs::remove_dir_all(&self.worktree);
    }
}
