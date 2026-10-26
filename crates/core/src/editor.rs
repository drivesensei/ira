//! Editor identity and disk I/O. Call these functions only on background workers.
use std::fs::{self, Permissions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::SystemTime;

pub const EDIT_MAX_BYTES: u64 = 5 * 1024 * 1024;
pub type DocumentId = u64;

#[derive(Debug, Clone)]
pub struct EditorDocument {
    pub id: DocumentId,
    pub listed_path: PathBuf,
    pub canonical_path: PathBuf,
    pub mtime: Option<SystemTime>,
    pub permissions: Permissions,
    pub read_only: bool,
    pub crlf: bool,
    /// LF-normalized initial content. The host draft becomes authoritative after open.
    pub content: String,
}
#[derive(Debug, Clone)]
pub struct SaveSnapshot {
    pub document_id: DocumentId,
    pub base_document: EditorDocument,
    pub mtime: Option<SystemTime>,
    pub edit_revision: u64,
    pub content: String,
}
#[derive(Debug, Clone)]
pub struct SaveCompletion {
    pub document_id: DocumentId,
    pub edit_revision: u64,
    pub document: EditorDocument,
    pub new_size: u64,
    /// Evict preview entries for this listed path, independent of mtime granularity.
    pub invalidate_path: PathBuf,
}
impl SaveCompletion {
    pub fn may_clear_dirty(&self, document_id: DocumentId, edit_revision: u64) -> bool {
        self.document_id == document_id && self.edit_revision == edit_revision
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorError(pub String);
impl std::fmt::Display for EditorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for EditorError {}
fn open_error(error: std::io::Error) -> EditorError {
    EditorError(format!("open failed: {error}"))
}

pub fn open_document(id: DocumentId, listed_path: &Path) -> Result<EditorDocument, EditorError> {
    let canonical_path = fs::canonicalize(listed_path).map_err(open_error)?;
    let file = fs::File::open(&canonical_path).map_err(open_error)?;
    let metadata = file.metadata().map_err(open_error)?;
    if metadata.len() > EDIT_MAX_BYTES {
        return Err(EditorError("file too large to edit (> 5 MB)".into()));
    }
    let mut bytes = Vec::new();
    file.take(EDIT_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(open_error)?;
    if bytes.len() as u64 > EDIT_MAX_BYTES {
        return Err(EditorError("file too large to edit (> 5 MB)".into()));
    }
    if bytes.contains(&0) {
        return Err(EditorError("binary file — not editable".into()));
    }
    let content = String::from_utf8(bytes)
        .map_err(|_| EditorError("non-UTF-8 file — read-only preview only".into()))?;
    let crlf = content.contains("\r\n");
    Ok(EditorDocument {
        id,
        listed_path: listed_path.to_owned(),
        canonical_path,
        mtime: metadata.modified().ok(),
        permissions: metadata.permissions(),
        read_only: metadata.permissions().readonly(),
        crlf,
        content: if crlf {
            content.replace("\r\n", "\n")
        } else {
            content
        },
    })
}

/// Standalone exact-base save; hosts serialize saves for each document.
pub fn save_document(snapshot: &SaveSnapshot) -> Result<SaveCompletion, EditorError> {
    let doc = &snapshot.base_document;
    // Serialize all in-process document sessions targeting the same canonical file.
    static LOCKS: OnceLock<Mutex<std::collections::HashMap<PathBuf, Weak<Mutex<()>>>>> =
        OnceLock::new();
    let lock = {
        let mut locks = LOCKS
            .get_or_init(|| Mutex::new(std::collections::HashMap::new()))
            .lock()
            .map_err(|_| EditorError("save failed: editor lock poisoned".into()))?;
        locks.retain(|_, lock| lock.strong_count() > 0);
        match locks.get(&doc.canonical_path).and_then(Weak::upgrade) {
            Some(lock) => lock,
            None => {
                let lock = Arc::new(Mutex::new(()));
                locks.insert(doc.canonical_path.clone(), Arc::downgrade(&lock));
                lock
            }
        }
    };
    let _guard = lock
        .lock()
        .map_err(|_| EditorError("save failed: editor lock poisoned".into()))?;
    if doc.id != snapshot.document_id {
        return Err(EditorError(
            "editor document changed — reopen before saving".into(),
        ));
    }
    let name = doc
        .listed_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    if doc.read_only {
        return Err(EditorError(format!("{name} is read-only")));
    }
    match fs::canonicalize(&doc.listed_path) {
        Ok(current) if current == doc.canonical_path => {}
        _ => {
            return Err(EditorError(
                "file path changed on disk — press Esc and reopen".into(),
            ))
        }
    }
    let current_mtime = fs::metadata(&doc.canonical_path)
        .ok()
        .and_then(|m| m.modified().ok());
    if let (Some(expected), Some(actual)) = (snapshot.mtime, current_mtime) {
        if expected != actual {
            return Err(EditorError(
                "file changed on disk — press Esc and reopen".into(),
            ));
        }
    }
    let content = if doc.crlf {
        snapshot.content.replace('\n', "\r\n")
    } else {
        snapshot.content.clone()
    };
    staging::save(
        &doc.canonical_path,
        content.as_bytes(),
        doc.permissions.clone(),
    )
    .map_err(|error| EditorError(format!("save failed: {error}")))?;
    let (mtime, new_size) = fs::metadata(&doc.canonical_path)
        .map(|m| (m.modified().ok(), m.len()))
        .unwrap_or((None, 0));
    let mut document = doc.clone();
    document.mtime = mtime;
    document.content = snapshot.content.clone();
    Ok(SaveCompletion {
        document_id: doc.id,
        edit_revision: snapshot.edit_revision,
        document,
        new_size,
        invalidate_path: doc.listed_path.clone(),
    })
}

/// Worker-side save ordering; newer queued revisions use the last successful baseline.
pub struct EditorSession {
    state: Mutex<(EditorDocument, Option<u64>)>,
}
impl EditorSession {
    pub fn new(document: EditorDocument) -> Self {
        Self {
            state: Mutex::new((document, None)),
        }
    }
    pub fn save(&self, request: &SaveSnapshot) -> Result<SaveCompletion, EditorError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| EditorError("save failed: editor lock poisoned".into()))?;
        if state.0.id != request.document_id
            || state.0.canonical_path != request.base_document.canonical_path
        {
            return Err(EditorError(
                "editor document changed — reopen before saving".into(),
            ));
        }
        if state.1.is_some_and(|last| request.edit_revision <= last) {
            return Err(EditorError(
                "stale editor revision — save the current draft".into(),
            ));
        }
        let mut snapshot = request.clone();
        snapshot.base_document = state.0.clone();
        snapshot.mtime = state.0.mtime;
        let completion = save_document(&snapshot)?;
        state.0 = completion.document.clone();
        state.1 = Some(completion.edit_revision);
        Ok(completion)
    }
}

#[cfg(test)]
#[path = "editor_tests.rs"]
mod tests;

#[path = "editor_staging.rs"]
mod staging;
