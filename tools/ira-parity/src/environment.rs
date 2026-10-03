use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use thiserror::Error;
#[derive(Debug, Error)]
#[error("protected environment root escaped run root: {0}")]
pub struct EnvironmentError(String);
#[derive(Clone, Debug)]
pub struct EnvironmentPolicy {
    windows: bool,
}
impl EnvironmentPolicy {
    pub fn host() -> Self {
        Self {
            windows: cfg!(windows),
        }
    }
    pub fn unix_for_test() -> Self {
        Self { windows: false }
    }
    pub fn windows_for_test() -> Self {
        Self { windows: true }
    }
}
#[derive(Clone, Debug)]
pub struct ChildEnvironment {
    values: BTreeMap<String, String>,
    root: PathBuf,
}
fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        use std::path::Component;
        match component {
            Component::Prefix(prefix) => out.push(prefix.as_os_str()),
            Component::RootDir => out.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(part) => out.push(part),
        }
    }
    out
}
fn contained_path(root: &Path, path: &Path) -> Result<PathBuf, EnvironmentError> {
    let canonical_root = std::fs::canonicalize(root).unwrap_or_else(|_| normalize_lexically(root));
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let lexical = normalize_lexically(&resolved);
    if !lexical.starts_with(&canonical_root) {
        return Err(EnvironmentError("path escapes scenario root".into()));
    }

    // Check the deepest existing ancestor before the caller creates any leaf
    // directories. canonicalize(full_path) is insufficient for a missing leaf
    // below an in-root symlink that resolves outside the fixture.
    let mut ancestor = lexical.as_path();
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or_else(|| EnvironmentError("path has no existing ancestor".into()))?;
    }
    let canonical_ancestor = std::fs::canonicalize(ancestor)
        .map_err(|e| EnvironmentError(format!("cannot resolve protected path ancestor: {e}")))?;
    if !canonical_ancestor.starts_with(&canonical_root) {
        return Err(EnvironmentError(
            "path escapes scenario root through symlink".into(),
        ));
    }

    if lexical.exists() {
        let canonical = std::fs::canonicalize(&lexical)
            .map_err(|e| EnvironmentError(format!("cannot resolve protected path: {e}")))?;
        if !canonical.starts_with(&canonical_root) {
            return Err(EnvironmentError(
                "path escapes scenario root through symlink".into(),
            ));
        }
        Ok(canonical)
    } else {
        Ok(lexical)
    }
}
impl ChildEnvironment {
    pub fn build<I, K, V>(
        policy: &EnvironmentPolicy,
        root: impl AsRef<Path>,
        overrides: I,
    ) -> Result<Self, EnvironmentError>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        Self::build_inner(policy, root, overrides, false)
    }
    pub fn build_with_overrides<I, K, V>(
        policy: &EnvironmentPolicy,
        root: impl AsRef<Path>,
        overrides: I,
    ) -> Result<Self, EnvironmentError>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        Self::build_inner(policy, root, overrides, true)
    }
    fn build_inner<I, K, V>(
        policy: &EnvironmentPolicy,
        root: impl AsRef<Path>,
        overrides: I,
        strict_roots: bool,
    ) -> Result<Self, EnvironmentError>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(&root).map_err(|e| EnvironmentError(e.to_string()))?;
        let mut values = BTreeMap::new();
        let keys = if policy.windows {
            vec!["USERPROFILE", "APPDATA", "LOCALAPPDATA", "TEMP", "TMP"]
        } else {
            vec![
                "HOME",
                "XDG_CONFIG_HOME",
                "XDG_CACHE_HOME",
                "XDG_DATA_HOME",
                "TMPDIR",
            ]
        };
        for k in &keys {
            values.insert(
                k.to_string(),
                root.join(k.to_ascii_lowercase())
                    .to_string_lossy()
                    .to_string(),
            );
        }
        if policy.windows {
            for k in ["SystemRoot", "WINDIR", "PATH", "HOMEDRIVE", "HOMEPATH"] {
                if let Some(v) = std::env::var_os(k) {
                    values.insert(k.into(), v.to_string_lossy().to_string());
                }
            }
        } else {
            values.insert("TERM".into(), "xterm-256color".into());
            values.insert("LANG".into(), "C.UTF-8".into());
        }
        for (key, value) in overrides {
            let key = key.as_ref();
            if [
                "HOME",
                "XDG_CONFIG_HOME",
                "XDG_CACHE_HOME",
                "XDG_DATA_HOME",
                "TMPDIR",
                "USERPROFILE",
                "APPDATA",
                "LOCALAPPDATA",
                "TEMP",
                "TMP",
            ]
            .contains(&key)
            {
                if !strict_roots {
                    continue;
                }
                let candidate = contained_path(&root, Path::new(value.as_ref()))
                    .map_err(|_| EnvironmentError(key.into()))?;
                values.insert(key.into(), candidate.to_string_lossy().to_string());
            } else if strict_roots && key == "IRA_IMAGES" {
                values.insert(key.into(), value.as_ref().into());
            } else if strict_roots {
                return Err(EnvironmentError(format!(
                    "unsupported environment override {key}"
                )));
            }
        }
        for k in &keys {
            let p = PathBuf::from(&values[*k]);
            std::fs::create_dir_all(&p).map_err(|e| EnvironmentError(e.to_string()))?;
            let c = std::fs::canonicalize(&p).map_err(|e| EnvironmentError(e.to_string()))?;
            if !c.starts_with(std::fs::canonicalize(&root).unwrap_or(root.clone())) {
                return Err(EnvironmentError((*k).into()));
            }
            values.insert((*k).into(), c.to_string_lossy().into());
        }
        let root = std::fs::canonicalize(&root).map_err(|e| EnvironmentError(e.to_string()))?;
        Ok(Self { values, root })
    }
    pub fn build_with_root(
        policy: &EnvironmentPolicy,
        root: impl AsRef<Path>,
    ) -> Result<Self, EnvironmentError> {
        Self::build(policy, root, std::iter::empty::<(String, String)>())
    }
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }
    pub fn path_is_within_root(&self, key: &str) -> bool {
        self.get(key)
            .is_some_and(|v| Path::new(v).starts_with(&self.root))
    }
    pub fn iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.values.iter()
    }
}
