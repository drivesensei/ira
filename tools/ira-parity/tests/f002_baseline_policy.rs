//! Single-worktree policy regression. All repositories/exports are synthetic.
use ira_parity::baseline::BaselineResolver;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
fn git(repo: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args([
            "-c",
            "user.name=Parity fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().into()
}
fn fixture() -> (PathBuf, PathBuf, String) {
    // Retained even on success: no recursive cleanup of exported source/evidence.
    let root = tempfile::tempdir().unwrap().keep();
    let repo = root.join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "--quiet"]);
    fs::write(repo.join("Cargo.lock"), "version = 3\n").unwrap();
    fs::write(repo.join("Cargo.toml"), "# synthetic: no build permitted\n").unwrap();
    fs::write(repo.join("sentinel"), b"FROZEN\0BYTES").unwrap();
    git(&repo, &["add", "."]);
    git(
        &repo,
        &["commit", "--quiet", "-m", "synthetic frozen source"],
    );
    let sha = git(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["tag", "fixture-baseline"]);
    (root, repo, sha)
}
fn resolver(repo: &Path, sha: &str, parent: &Path) -> BaselineResolver {
    BaselineResolver::from_manifest_dir(repo)
        .with_tag("fixture-baseline")
        .with_expected_sha(sha)
        .with_temp_root(parent)
}
#[test]
fn missing_local_tag_never_fetches_or_changes_refs() {
    let (_root, repo, sha) = fixture();
    git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "file:///definitely-unavailable-parity-remote",
        ],
    );
    let refs = git(&repo, &["show-ref"]);
    let error = BaselineResolver::from_manifest_dir(&repo)
        .with_tag("missing")
        .with_expected_sha(&sha)
        .resolve()
        .unwrap_err()
        .to_string();
    assert!(error.contains("implicit fetch disabled"));
    assert_eq!(git(&repo, &["show-ref"]), refs);
}
#[test]
fn cache_miss_retains_frozen_export_without_worktree_or_implicit_build() {
    let (root, repo, sha) = fixture();
    let before = git(&repo, &["worktree", "list", "--porcelain"]);
    let refs = git(&repo, &["show-ref"]);
    let exports = root.join("exports");
    let resolver = resolver(&repo, &sha, &exports);
    let error = resolver.resolve_and_build().unwrap_err().to_string();
    assert!(
        error.contains("explicit bounded oracle build policy required"),
        "{error}"
    );
    let export = resolver.last_source_export_path_for_test().unwrap();
    assert_eq!(fs::read(export.join("sentinel")).unwrap(), b"FROZEN\0BYTES");
    assert!(export.join(".ira-parity-export.toml").is_file());
    assert!(!export.join(".git").exists());
    assert_eq!(git(&repo, &["worktree", "list", "--porcelain"]), before);
    assert_eq!(git(&repo, &["show-ref"]), refs);
    assert_eq!(git(&repo, &["status", "--porcelain"]), "");
}
#[test]
fn wrong_local_commit_is_rejected_without_ref_mutation() {
    let (_root, repo, _sha) = fixture();
    let refs = git(&repo, &["show-ref"]);
    let error = BaselineResolver::from_manifest_dir(&repo)
        .with_tag("fixture-baseline")
        .with_expected_sha("0000000000000000000000000000000000000000")
        .resolve()
        .unwrap_err()
        .to_string();
    assert!(error.contains("SHA mismatch"));
    assert_eq!(git(&repo, &["show-ref"]), refs);
}
#[cfg(unix)]
#[test]
fn symlink_blob_is_rejected_before_export_payload_creation() {
    let (root, repo, _sha) = fixture();
    std::os::unix::fs::symlink("sentinel", repo.join("link")).unwrap();
    git(&repo, &["add", "link"]);
    git(&repo, &["commit", "--quiet", "-m", "synthetic symlink"]);
    git(&repo, &["tag", "-f", "fixture-baseline"]);
    let sha = git(&repo, &["rev-parse", "HEAD"]);
    let exports = root.join("exports");
    let error = resolver(&repo, &sha, &exports)
        .resolve_and_build()
        .unwrap_err()
        .to_string();
    assert!(error.contains("regular Git blobs"), "{error}");
    assert_eq!(fs::read_dir(&exports).unwrap().count(), 0);
    assert_eq!(git(&repo, &["status", "--porcelain"]), "");
}
fn cached_fixture() -> (PathBuf, ira_parity::baseline::Baseline) {
    let (root, repo, sha) = fixture();
    let resolver = resolver(&repo, &sha, &root.join("exports")).with_cache_root(root.join("cache"));
    let mut metadata = resolver.cache_metadata().unwrap();
    let key = root
        .join("cache")
        .join(metadata.cache_key_with_os(std::env::consts::OS));
    fs::create_dir_all(&key).unwrap();
    let exe = key.join(if cfg!(windows) { "ira.exe" } else { "ira" });
    fs::write(&exe, b"synthetic cache identity only: never execute").unwrap();
    metadata.record_executable(&exe).unwrap();
    fs::write(
        key.join("metadata.toml"),
        toml::to_string(&metadata).unwrap(),
    )
    .unwrap();
    let baseline = resolver.resolve_and_build().unwrap();
    assert!(baseline.source_export_is_verified());
    (root, baseline)
}
#[cfg(unix)]
#[test]
fn cache_key_symlink_is_rejected_before_build_or_external_write() {
    let (root, repo, sha) = fixture();
    let resolver = resolver(&repo, &sha, &root.join("exports")).with_cache_root(root.join("cache"));
    let metadata = resolver.cache_metadata().unwrap();
    fs::create_dir(root.join("cache")).unwrap();
    let external = root.join("external");
    fs::create_dir(&external).unwrap();
    std::os::unix::fs::symlink(
        &external,
        root.join("cache")
            .join(metadata.cache_key_with_os(std::env::consts::OS)),
    )
    .unwrap();
    assert!(resolver
        .resolve_and_build()
        .unwrap_err()
        .to_string()
        .contains("symlink"));
    assert_eq!(fs::read_dir(external).unwrap().count(), 0);
}
#[cfg(unix)]
#[test]
fn source_export_verification_rejects_changed_executable_mode() {
    use std::os::unix::fs::PermissionsExt;
    let (_root, baseline) = cached_fixture();
    let file = baseline.worktree_path().join("sentinel");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!baseline.source_export_is_verified());
}
#[cfg(unix)]
#[test]
fn source_export_verification_rejects_replaced_root_symlink() {
    let (root, baseline) = cached_fixture();
    let export = baseline.worktree_path();
    let renamed = root.join("moved-export");
    fs::rename(export, &renamed).unwrap();
    std::os::unix::fs::symlink(&renamed, export).unwrap();
    assert!(!baseline.source_export_is_verified());
}
