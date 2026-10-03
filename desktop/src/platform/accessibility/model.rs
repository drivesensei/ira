//! Pure host semantic projection. No native objects, actor locks or filesystem reads.
use ira_core::{input::InputContext, observable::Snapshot};
use std::{collections::BTreeMap, ops::Range, path::PathBuf, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId {
    pub window: u64,
    pub serial: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    Window,
    Group,
    List,
    Row,
    Button,
    Status,
    Dialog,
    TextField,
    TextArea,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Target {
    Window,
    Pane(usize),
    Entry {
        pane: usize,
        path: PathBuf,
        listing_generation: u64,
    },
    Place {
        kind: PlaceKind,
        path: PathBuf,
        shortcut: char,
        occurrence: usize,
    },
    Job(u64),
    Modal,
    Text {
        document: u64,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PlaceKind {
    Drive,
    Common,
    Bookmark,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Focus,
    Reveal,
    Activate,
    SetSelected(bool),
    SelectOnly,
    SetValue(String),
    SetSelection(Range<usize>),
    Dismiss,
    Pause,
    Cancel,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Capability {
    Focus,
    Activate,
    Selection,
    Value,
    TextSelection,
    Dismiss,
    Pause,
    Cancel,
}
impl Action {
    pub fn capability(&self) -> Capability {
        match self {
            Self::Focus | Self::Reveal => Capability::Focus,
            Self::Activate => Capability::Activate,
            Self::SetSelected(_) | Self::SelectOnly => Capability::Selection,
            Self::SetValue(_) => Capability::Value,
            Self::SetSelection(_) => Capability::TextSelection,
            Self::Dismiss => Capability::Dismiss,
            Self::Pause => Capability::Pause,
            Self::Cancel => Capability::Cancel,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl Rect {
    pub fn valid(self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|x| x.is_finite())
            && self.width > 0.0
            && self.height > 0.0
    }
    pub fn contains(self, x: f64, y: f64) -> bool {
        self.valid()
            && x >= self.x
            && y >= self.y
            && x < self.x + self.width
            && y < self.y + self.height
    }
    pub fn intersection(self, other: Self) -> Option<Self> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let r = Self {
            x,
            y,
            width: (self.x + self.width).min(other.x + other.width) - x,
            height: (self.y + self.height).min(other.y + other.height) - y,
        };
        r.valid().then_some(r)
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub bounds: Rect,
    pub visible: Rect,
}
#[derive(Clone, Debug, Default)]
pub struct LayoutSnapshot {
    pub window_generation: u64,
    pub semantic_revision: u64,
    pub revision: u64,
    pub nodes: BTreeMap<NodeId, Geometry>,
}
impl LayoutSnapshot {
    /// Only record bounds collected from actual GPUI layout/prepaint; clip before native hit testing.
    pub fn record(&mut self, id: NodeId, bounds: Rect, clip: Rect) -> bool {
        if !bounds.valid() || !clip.valid() || id.window != self.window_generation {
            self.nodes.remove(&id);
            return false;
        }
        if let Some(visible) = bounds.intersection(clip) {
            self.nodes.insert(id, Geometry { bounds, visible });
            true
        } else {
            self.nodes.remove(&id);
            false
        }
    }
}
#[derive(Clone, Debug)]
pub struct NativeTextSnapshot {
    pub document_generation: u64,
    pub focus_generation: u64,
    pub revision: u64,
    pub text: Arc<str>,
    pub selection_utf16: Range<usize>,
    pub marked_utf16: Option<Range<usize>>,
    pub read_only: bool,
    pub disabled: bool,
    pub multiline: bool,
}
#[derive(Clone, Debug)]
pub struct Node {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub role: Role,
    pub name: String,
    pub value: Option<Arc<str>>,
    pub help: Option<String>,
    pub children: Vec<NodeId>,
    pub enabled: bool,
    pub focusable: bool,
    pub selected: bool,
    pub read_only: bool,
    pub busy: bool,
    pub capabilities: Vec<Capability>,
    pub target: Target,
    pub geometry: Option<Geometry>,
    pub text_selection: Option<Range<usize>>,
    pub marked_text: Option<Range<usize>>,
}
impl Node {
    /// Native scalar queries never copy a list's complete children vector.
    pub fn clone_metadata(&self) -> Self {
        Self {
            id: self.id,
            parent: self.parent,
            role: self.role,
            name: self.name.clone(),
            value: self.value.clone(),
            help: self.help.clone(),
            children: Vec::new(),
            enabled: self.enabled,
            focusable: self.focusable,
            selected: self.selected,
            read_only: self.read_only,
            busy: self.busy,
            capabilities: self.capabilities.clone(),
            target: self.target.clone(),
            geometry: self.geometry,
            text_selection: self.text_selection.clone(),
            marked_text: self.marked_text.clone(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub window: u64,
    pub focus: u64,
    pub document: u64,
    pub revision: u64,
    pub text_revision: u64,
}
#[derive(Clone, Debug)]
pub struct SemanticTree {
    pub stamp: Stamp,
    pub layout_revision: u64,
    pub root: NodeId,
    pub focused: Option<NodeId>,
    pub active_modal: Option<NodeId>,
    pub nodes: BTreeMap<NodeId, Node>,
}
impl SemanticTree {
    pub fn hit_test(&self, x: f64, y: f64) -> Option<NodeId> {
        fn visit(t: &SemanticTree, id: NodeId, x: f64, y: f64) -> Option<NodeId> {
            let n = t.nodes.get(&id)?;
            for child in n.children.iter().rev() {
                if let Some(found) = visit(t, *child, x, y) {
                    return Some(found);
                }
            }
            n.geometry.filter(|g| g.visible.contains(x, y)).map(|_| id)
        }
        visit(self, self.active_modal.unwrap_or(self.root), x, y)
    }
    pub fn in_modal_scope(&self, id: NodeId) -> bool {
        let Some(modal) = self.active_modal else {
            return true;
        };
        let mut node = Some(id);
        while let Some(id) = node {
            if id == modal {
                return true;
            };
            node = self.nodes.get(&id).and_then(|n| n.parent);
        }
        false
    }
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Identity {
    Named(String),
    Entry(usize, PathBuf, PathBuf),
    Place(PlaceKind, PathBuf, char, usize),
    Job(u64),
    Text(u64),
    Modal(u64, String),
}
#[derive(Default)]
pub struct AccessibilityModel {
    window: Option<u64>,
    next: u64,
    registry: BTreeMap<Identity, NodeId>,
}
impl AccessibilityModel {
    /// Override semantic cursor focus only with a host-observed actual focused control.
    /// The host owns native FocusHandles; this never navigates or mutates the actor snapshot.
    pub fn project_with_host_focus(
        &mut self,
        snapshot: &Snapshot,
        text: Option<&NativeTextSnapshot>,
        layout: &LayoutSnapshot,
        host_focused: Option<NodeId>,
    ) -> SemanticTree {
        let mut tree = self.project(snapshot, text, layout);
        if let Some(id) = host_focused
            && tree
                .nodes
                .get(&id)
                .is_some_and(|node| node.enabled && node.focusable)
            && tree.in_modal_scope(id)
        {
            tree.focused = Some(id);
        }
        tree
    }

    fn id(&mut self, key: Identity, window: u64) -> NodeId {
        if let Some(id) = self.registry.get(&key) {
            return *id;
        }
        self.next += 1;
        let id = NodeId {
            window,
            serial: self.next,
        };
        self.registry.insert(key, id);
        id
    }
    fn add(
        &mut self,
        t: &mut SemanticTree,
        key: Identity,
        parent: Option<NodeId>,
        role: Role,
        name: String,
        target: Target,
    ) -> NodeId {
        let id = self.id(key, t.stamp.window);
        t.nodes.insert(
            id,
            Node {
                id,
                parent,
                role,
                name,
                value: None,
                help: None,
                children: vec![],
                enabled: true,
                focusable: false,
                selected: false,
                read_only: false,
                busy: false,
                capabilities: vec![],
                target,
                geometry: None,
                text_selection: None,
                marked_text: None,
            },
        );
        if let Some(p) = parent
            && let Some(n) = t.nodes.get_mut(&p)
        {
            n.children.push(id);
        }
        id
    }
    pub fn project(
        &mut self,
        s: &Snapshot,
        text: Option<&NativeTextSnapshot>,
        layout: &LayoutSnapshot,
    ) -> SemanticTree {
        self.project_cancellable(s, text, layout, &|| false)
            .expect("uncancelled projection")
    }
    fn project_cancellable(
        &mut self,
        s: &Snapshot,
        text: Option<&NativeTextSnapshot>,
        layout: &LayoutSnapshot,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<SemanticTree, super::Rejection> {
        if cancelled() {
            return Err(super::Rejection::Stale);
        }
        if self.window != Some(s.window_generation) {
            self.registry.clear();
            self.next = 0;
            self.window = Some(s.window_generation);
        }
        let root = self.id(Identity::Named("root".into()), s.window_generation);
        let mut t = SemanticTree {
            stamp: Stamp {
                window: s.window_generation,
                focus: s.focus_generation,
                document: s.document_generation,
                revision: s.revision,
                text_revision: text
                    .filter(|x| {
                        x.document_generation == s.document_generation
                            && x.focus_generation == s.focus_generation
                    })
                    .map(|x| x.revision)
                    .unwrap_or(0),
            },
            layout_revision: layout.revision,
            root,
            focused: None,
            active_modal: None,
            nodes: BTreeMap::new(),
        };
        self.add(
            &mut t,
            Identity::Named("root".into()),
            None,
            Role::Window,
            "IRA file manager".into(),
            Target::Window,
        );
        for pane in 0..if s.split { 2 } else { 1 } {
            let p = &s.panes[pane];
            let dir = PathBuf::from(p.folder.as_ref().map(|f| f.path.as_str()).unwrap_or(""));
            let id = self.add(
                &mut t,
                Identity::Named(format!("pane-{pane}")),
                Some(root),
                Role::List,
                format!(
                    "{} files — {}",
                    if pane == 0 { "Left" } else { "Right" },
                    dir.display()
                ),
                Target::Pane(pane),
            );
            let n = t.nodes.get_mut(&id).unwrap();
            n.busy = !p.listing_settled;
            n.help = Some(format!(
                "{} items; {} selected{}",
                p.rows.len(),
                p.selected_paths.len(),
                if p.filter_query.is_some() {
                    "; filtered"
                } else {
                    ""
                }
            ));
            for (i, row) in p.rows.iter().enumerate() {
                if i % 64 == 0 && cancelled() {
                    return Err(super::Rejection::Stale);
                }
                let path = PathBuf::from(&row.entry.path);
                let row_id = self.add(
                    &mut t,
                    Identity::Entry(pane, dir.clone(), path.clone()),
                    Some(id),
                    Role::Row,
                    format!(
                        "{}{}",
                        path.file_name()
                            .unwrap_or(path.as_os_str())
                            .to_string_lossy(),
                        if row.entry.is_dir {
                            " — folder"
                        } else {
                            " — file"
                        }
                    ),
                    Target::Entry {
                        pane,
                        path,
                        listing_generation: p.listing_generation,
                    },
                );
                let n = t.nodes.get_mut(&row_id).unwrap();
                n.selected = row.selected;
                n.busy = row.deleting;
                n.focusable = true;
                n.help = Some(format!(
                    "{} bytes; modified {}",
                    row.entry.size,
                    row.entry
                        .modified
                        .map(|m| m.to_string())
                        .unwrap_or_else(|| "unknown".into())
                ));
                n.capabilities = vec![
                    Capability::Focus,
                    Capability::Activate,
                    Capability::Selection,
                ];
                if matches!(s.input_context,InputContext::Pane(active) if active==pane)
                    && p.cursor == Some(i)
                {
                    t.focused = Some(row_id);
                }
            }
            if p.rows.is_empty() {
                t.nodes.get_mut(&id).unwrap().help = Some(
                    if !p.listing_settled {
                        "Loading"
                    } else if p.filter_query.is_some() {
                        "No matches"
                    } else {
                        "Empty folder"
                    }
                    .into(),
                );
            }
        }
        for (kind, name, places) in [
            (PlaceKind::Drive, "Drives", &s.drives),
            (PlaceKind::Common, "Common folders", &s.folders),
            (PlaceKind::Bookmark, "Bookmarks", &s.bookmarks),
        ] {
            let group = self.add(
                &mut t,
                Identity::Named(name.into()),
                Some(root),
                Role::Group,
                name.into(),
                Target::Window,
            );
            let mut occurrences = BTreeMap::<(PathBuf, char), usize>::new();
            for (i, p) in places.iter().flatten().enumerate() {
                if i % 64 == 0 && cancelled() {
                    return Err(super::Rejection::Stale);
                }
                let path = PathBuf::from(&p.path);
                let next = occurrences.entry((path.clone(), p.shortcut)).or_default();
                let occurrence = *next;
                *next += 1;
                let id = self.add(
                    &mut t,
                    Identity::Place(kind, path.clone(), p.shortcut, occurrence),
                    Some(group),
                    Role::Button,
                    format!("{} ({})", p.label, p.shortcut),
                    Target::Place {
                        kind,
                        path,
                        shortcut: p.shortcut,
                        occurrence,
                    },
                );
                let n = t.nodes.get_mut(&id).unwrap();
                n.focusable = true;
                n.capabilities = vec![Capability::Focus, Capability::Activate];
            }
        }
        if s.copy_board {
            let group = self.add(
                &mut t,
                Identity::Named("jobs".into()),
                Some(root),
                Role::List,
                "Copy Board".into(),
                Target::Window,
            );
            for (i, job) in s.jobs.iter().enumerate() {
                if i % 64 == 0 && cancelled() {
                    return Err(super::Rejection::Stale);
                }
                let id = self.add(
                    &mut t,
                    Identity::Job(job.id),
                    Some(group),
                    Role::Row,
                    format!("{} — {:?}", job.label, job.status),
                    Target::Job(job.id),
                );
                let n = t.nodes.get_mut(&id).unwrap();
                n.value = Some(
                    format!(
                        "{} of {} bytes",
                        job.copied_bytes,
                        job.total_bytes
                            .map(|x| x.to_string())
                            .unwrap_or("unknown".into())
                    )
                    .into(),
                );
                let live = matches!(
                    job.status,
                    ira_core::services::transfer::JobStatus::Running
                        | ira_core::services::transfer::JobStatus::Queued
                        | ira_core::services::transfer::JobStatus::Paused
                );
                n.focusable = true;
                n.busy = live;
                n.capabilities = if live {
                    vec![Capability::Focus, Capability::Pause, Capability::Cancel]
                } else {
                    vec![Capability::Focus]
                };
                if matches!(s.input_context, InputContext::Board) && s.copy_board_cursor == Some(i)
                {
                    t.focused = Some(id);
                }
                if live {
                    for (suffix, name, capability) in [
                        ("pause", "Pause or resume", Capability::Pause),
                        ("cancel", "Cancel", Capability::Cancel),
                    ] {
                        let button = self.add(
                            &mut t,
                            Identity::Named(format!("job-{}-{suffix}", job.id)),
                            Some(id),
                            Role::Button,
                            format!("{name} {}", job.label),
                            Target::Job(job.id),
                        );
                        t.nodes.get_mut(&button).unwrap().capabilities = vec![capability];
                    }
                }
            }
        }
        if let Some(status) = &s.status {
            let id = self.add(
                &mut t,
                Identity::Named("status".into()),
                Some(root),
                Role::Status,
                status.text.clone(),
                Target::Window,
            );
            t.nodes.get_mut(&id).unwrap().read_only = true;
        }
        let modal_name = match s.input_context {
            InputContext::Rename => Some("Rename"),
            InputContext::Goto => Some("Go to path"),
            InputContext::Create => Some("Create new entry"),
            InputContext::Help => Some("Keybindings"),
            InputContext::Error => Some("Error"),
            InputContext::Deletion => Some("Deletion progress"),
            InputContext::MultiInfo => Some("Selection information"),
            InputContext::Info => Some("File information"),
            InputContext::Confirmation => Some("Confirm operation"),
            _ => None,
        };
        let modal = modal_name.map(|name| {
            let id = self.add(
                &mut t,
                Identity::Modal(s.focus_generation, name.into()),
                Some(root),
                Role::Dialog,
                name.into(),
                Target::Modal,
            );
            let n = t.nodes.get_mut(&id).unwrap();
            n.focusable = true;
            n.capabilities = vec![Capability::Dismiss];
            n.value = s
                .confirming
                .as_ref()
                .map(|c| Arc::from(format!("{} — {:?}", c.label, c.policy)))
                .or_else(|| s.info.as_ref().map(|i| Arc::from(i.lines.join("\n"))))
                .or_else(|| {
                    s.status
                        .as_ref()
                        .filter(|x| x.is_error)
                        .map(|x| Arc::from(x.text.as_str()))
                });
            t.active_modal = Some(id);
            t.focused = Some(id);
            if matches!(s.input_context, InputContext::Confirmation) {
                let confirm = self.add(
                    &mut t,
                    Identity::Modal(s.focus_generation, "Confirm button".into()),
                    Some(id),
                    Role::Button,
                    "Confirm".into(),
                    Target::Modal,
                );
                t.nodes.get_mut(&confirm).unwrap().capabilities = vec![Capability::Activate];
            }
            let cancel = self.add(
                &mut t,
                Identity::Modal(s.focus_generation, "Dismiss button".into()),
                Some(id),
                Role::Button,
                "Close or cancel".into(),
                Target::Modal,
            );
            t.nodes.get_mut(&cancel).unwrap().capabilities = vec![Capability::Dismiss];
            id
        });
        if let Some(text) = text.filter(|x| {
            x.document_generation == s.document_generation
                && x.focus_generation == s.focus_generation
        }) {
            let editing = matches!(
                s.input_context,
                InputContext::Editor
                    | InputContext::Rename
                    | InputContext::Goto
                    | InputContext::Create
                    | InputContext::Search
            );
            if editing {
                let id = self.add(
                    &mut t,
                    Identity::Text(text.document_generation),
                    Some(modal.unwrap_or(root)),
                    if text.multiline {
                        Role::TextArea
                    } else {
                        Role::TextField
                    },
                    modal_name
                        .unwrap_or(if text.multiline {
                            "File editor"
                        } else {
                            "Search"
                        })
                        .into(),
                    Target::Text {
                        document: text.document_generation,
                    },
                );
                let n = t.nodes.get_mut(&id).unwrap();
                n.value = Some(text.text.clone());
                n.read_only = text.read_only;
                n.enabled = !text.disabled;
                n.focusable = true;
                n.capabilities = vec![Capability::Focus, Capability::TextSelection];
                if !text.read_only && !text.disabled {
                    n.capabilities.push(Capability::Value);
                }
                if valid_text_range(&text.text, &text.selection_utf16) {
                    n.text_selection = Some(text.selection_utf16.clone());
                }
                n.marked_text = text
                    .marked_utf16
                    .clone()
                    .filter(|r| valid_text_range(&text.text, r));
                t.focused = Some(id);
            }
        }
        let coherent = layout.window_generation == s.window_generation
            && layout.semantic_revision == s.revision;
        for (i, n) in t.nodes.values_mut().enumerate() {
            if i % 64 == 0 && cancelled() {
                return Err(super::Rejection::Stale);
            }
            if coherent {
                n.geometry = layout.nodes.get(&n.id).copied().filter(|g| {
                    g.bounds.valid()
                        && g.visible.valid()
                        && g.bounds.intersection(g.visible) == Some(g.visible)
                });
            }
        }
        if let Some(modal) = t.active_modal {
            t.nodes.get_mut(&root).unwrap().children = vec![modal];
            let mut background = Vec::new();
            for (i, id) in t.nodes.keys().enumerate() {
                if i % 64 == 0 && cancelled() {
                    return Err(super::Rejection::Stale);
                }
                if !t.in_modal_scope(*id) {
                    background.push(*id);
                }
            }
            for (i, id) in background.into_iter().enumerate() {
                if i % 64 == 0 && cancelled() {
                    return Err(super::Rejection::Stale);
                }
                let n = t.nodes.get_mut(&id).unwrap();
                n.enabled = false;
                n.capabilities.clear();
            }
        }
        Ok(t)
    }
}

/// UTF-16 ranges may not bisect a scalar (e.g. the surrogate pair for emoji).
pub fn valid_text_range(text: &str, range: &Range<usize>) -> bool {
    if range.start > range.end {
        return false;
    }
    let mut offset = 0;
    let mut start = range.start == 0;
    let mut end = range.end == 0;
    for ch in text.chars() {
        offset += ch.len_utf16();
        start |= offset == range.start;
        end |= offset == range.end;
    }
    start && end
}

/// Exact authoritative semantic identity; unrelated document/focus revisions are not ordered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestKey {
    pub window_generation: u64,
    pub semantic_revision: u64,
    pub document_generation: u64,
    pub focus_generation: u64,
    pub native_text_revision: u64,
    pub host_focus_revision: u64,
    pub host_presentation_revision: u64,
}
impl RequestKey {
    pub fn stamp(self) -> Stamp {
        Stamp {
            window: self.window_generation,
            revision: self.semantic_revision,
            document: self.document_generation,
            focus: self.focus_generation,
            text_revision: self.native_text_revision,
        }
    }
}
/// Actual native-rendered chrome, captured by the runtime rather than inferred here.
#[derive(Clone, Debug)]
pub struct HostPresentationSnapshot {
    pub revision: u64,
    pub footer: Arc<str>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameKey {
    pub request: RequestKey,
    pub request_seq: u64,
    pub layout_revision: u64,
}
/// All supported selector combinations are indexed, including unspecified selectors.
type SelectorIndex = BTreeMap<(Option<Role>, Option<Capability>), NodeId>;
#[derive(Debug, Default)]
pub struct TargetIndex {
    targets: BTreeMap<Target, SelectorIndex>,
}
impl TargetIndex {
    fn build(tree: &SemanticTree, cancelled: &dyn Fn() -> bool) -> Result<Self, super::Rejection> {
        let mut index = Self::default();
        for (i, node) in tree.nodes.values().enumerate() {
            if i % 64 == 0 && cancelled() {
                return Err(super::Rejection::Stale);
            }
            let selectors = index.targets.entry(node.target.clone()).or_default();
            for role in [None, Some(node.role)] {
                selectors.entry((role, None)).or_insert(node.id);
                for capability in &node.capabilities {
                    selectors
                        .entry((role, Some(*capability)))
                        .or_insert(node.id);
                }
            }
        }
        Ok(index)
    }
    pub fn lookup(
        &self,
        target: &Target,
        role: Option<Role>,
        capability: Option<Capability>,
    ) -> Option<NodeId> {
        self.targets.get(target)?.get(&(role, capability)).copied()
    }
}
#[derive(Debug)]
pub struct PreparedSemantic {
    pub key: RequestKey,
    pub tree: Arc<SemanticTree>,
    pub index: TargetIndex,
    hit_order: BTreeMap<NodeId, usize>,
    pub selection_containers: std::collections::BTreeSet<NodeId>,
    pub siblings: BTreeMap<NodeId, (Option<NodeId>, Option<NodeId>)>,
}
impl PreparedSemantic {
    /// Pure worker preparation. Never build an index on the foreground publication path.
    pub fn from_tree(tree: Arc<SemanticTree>, key: RequestKey) -> Result<Self, super::Rejection> {
        Self::from_tree_cancellable(tree, key, &|| false)
    }
    fn from_tree_cancellable(
        tree: Arc<SemanticTree>,
        key: RequestKey,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, super::Rejection> {
        if tree.stamp != key.stamp() {
            return Err(super::Rejection::Stale);
        }
        let index = TargetIndex::build(&tree, cancelled)?;
        let mut selection_containers = std::collections::BTreeSet::new();
        let mut siblings = BTreeMap::new();
        for (visit, node) in tree.nodes.values().enumerate() {
            if visit % 64 == 0 && cancelled() {
                return Err(super::Rejection::Stale);
            }
            if node.capabilities.contains(&Capability::Selection)
                && let Some(parent) = node.parent
            {
                selection_containers.insert(parent);
            }
            for (i, id) in node.children.iter().enumerate() {
                if i % 64 == 0 && cancelled() {
                    return Err(super::Rejection::Stale);
                }
                siblings.insert(
                    *id,
                    (
                        i.checked_sub(1).map(|i| node.children[i]),
                        node.children.get(i + 1).copied(),
                    ),
                );
            }
        }
        let mut hit_order = BTreeMap::new();
        let mut stack = vec![(tree.active_modal.unwrap_or(tree.root), false)];
        let mut visits = 0usize;
        while let Some((id, visited)) = stack.pop() {
            if visits.is_multiple_of(64) && cancelled() {
                return Err(super::Rejection::Stale);
            }
            visits += 1;
            if visited {
                hit_order.insert(id, hit_order.len());
            } else if let Some(node) = tree.nodes.get(&id) {
                stack.push((id, true));
                // LIFO visits the last logical child first, matching legacy hit_test.
                for (i, child) in node.children.iter().enumerate() {
                    if i % 64 == 0 && cancelled() {
                        return Err(super::Rejection::Stale);
                    }
                    stack.push((*child, false));
                }
            }
        }
        Ok(Self {
            key,
            tree,
            index,
            hit_order,
            selection_containers,
            siblings,
        })
    }
}
impl AccessibilityModel {
    pub fn retained_identity_count(&self) -> usize {
        self.registry.len()
    }
    pub fn prepare_with_presentation(
        &mut self,
        snapshot: &Snapshot,
        text: Option<&NativeTextSnapshot>,
        key: RequestKey,
        host_focused: Option<NodeId>,
        presentation: &HostPresentationSnapshot,
    ) -> Result<PreparedSemantic, super::Rejection> {
        self.prepare_with_presentation_cancellable(
            snapshot,
            text,
            key,
            host_focused,
            presentation,
            &|| false,
        )
    }
    pub fn prepare_with_presentation_cancellable(
        &mut self,
        snapshot: &Snapshot,
        text: Option<&NativeTextSnapshot>,
        key: RequestKey,
        host_focused: Option<NodeId>,
        presentation: &HostPresentationSnapshot,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<PreparedSemantic, super::Rejection> {
        if key.host_presentation_revision != presentation.revision {
            return Err(super::Rejection::Stale);
        }
        let mut tree =
            self.project_cancellable(snapshot, text, &LayoutSnapshot::default(), cancelled)?;
        if let Some(id) = host_focused
            && tree
                .nodes
                .get(&id)
                .is_some_and(|n| n.enabled && n.focusable)
            && tree.in_modal_scope(id)
        {
            tree.focused = Some(id);
        }
        let identity = Identity::Named("status".into());
        let id = self.id(identity.clone(), snapshot.window_generation);
        if !tree.nodes.contains_key(&id) {
            let root = tree.root;
            self.add(
                &mut tree,
                identity,
                Some(root),
                Role::Status,
                presentation.footer.to_string(),
                Target::Window,
            );
        }
        let node = tree.nodes.get_mut(&id).expect("status just projected");
        node.name = presentation.footer.to_string();
        node.value = Some(presentation.footer.clone());
        node.read_only = true;
        if let Some(modal) = tree.active_modal {
            node.enabled = false;
            node.capabilities.clear();
            let root = tree.root;
            tree.nodes.get_mut(&root).expect("projected root").children = vec![modal];
        }
        PreparedSemantic::from_tree_cancellable(Arc::new(tree), key, cancelled)
    }
    /// Registry deliberately retains window-lifetime identities across arbitrary refiltering.
    /// Its memory is O(unique identities visited in this window), not O(visible rows).
    pub fn prepare(
        &mut self,
        snapshot: &Snapshot,
        text: Option<&NativeTextSnapshot>,
        key: RequestKey,
        host_focused: Option<NodeId>,
    ) -> Result<PreparedSemantic, super::Rejection> {
        let tree =
            self.project_with_host_focus(snapshot, text, &LayoutSnapshot::default(), host_focused);
        PreparedSemantic::from_tree(Arc::new(tree), key)
    }
}
#[derive(Debug, Default)]
pub struct SparseGeometry {
    pub nodes: BTreeMap<NodeId, Geometry>,
    hit_order: Vec<NodeId>,
}
impl SparseGeometry {
    pub fn ordered_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.hit_order.iter().copied()
    }
    pub fn hit_test(&self, x: f64, y: f64) -> Option<NodeId> {
        self.hit_test_with_visits(x, y).0
    }
    /// Measures actual sparse production traversal, including misses.
    pub fn hit_test_with_visits(&self, x: f64, y: f64) -> (Option<NodeId>, usize) {
        for (visit, id) in self.hit_order.iter().enumerate() {
            if self.nodes[id].visible.contains(x, y) {
                return (Some(*id), visit + 1);
            }
        }
        (None, self.hit_order.len())
    }
}
pub type ValueChange = (Option<Arc<str>>, Option<Arc<str>>);
#[derive(Debug, Default)]
pub struct NotificationPlan {
    pub semantic_visits: usize,
    pub materialized_only: bool,
    pub materialization_revision: Option<u64>,
    pub focused: Option<NodeId>,
    pub layout_changed: bool,
    pub structure_changed: bool,
    pub selected_parents: Vec<NodeId>,
    pub selected_items: BTreeMap<NodeId, (bool, bool)>,
    pub values: BTreeMap<NodeId, ValueChange>,
    pub text_selections: Vec<NodeId>,
    pub removed: Vec<NodeId>,
}
#[derive(Debug)]
pub struct PreparedFrame {
    pub publication_seq: u64,
    pub base_publication_seq: u64,
    pub key: FrameKey,
    pub semantic: Arc<PreparedSemantic>,
    pub geometry: SparseGeometry,
    pub notifications: NotificationPlan,
    pub(crate) base_tree: Option<Arc<SemanticTree>>,
}
/// Call only on the background worker, with the last successfully installed/ACKed frame.
pub fn prepare_frame(
    base: Option<&PreparedFrame>,
    semantic: Arc<PreparedSemantic>,
    key: FrameKey,
    layout: LayoutSnapshot,
    publication_seq: u64,
) -> Result<PreparedFrame, super::Rejection> {
    prepare_frame_cancellable(base, semantic, key, layout, publication_seq, &|| false)
}
pub fn prepare_frame_cancellable(
    base: Option<&PreparedFrame>,
    semantic: Arc<PreparedSemantic>,
    key: FrameKey,
    layout: LayoutSnapshot,
    publication_seq: u64,
    cancelled: &dyn Fn() -> bool,
) -> Result<PreparedFrame, super::Rejection> {
    if cancelled() {
        return Err(super::Rejection::Stale);
    }
    if semantic.key != key.request
        || layout.window_generation != key.request.window_generation
        || layout.semantic_revision != key.request.semantic_revision
        || layout.revision != key.layout_revision
        || publication_seq == 0
        || base.is_some_and(|b| {
            publication_seq <= b.publication_seq
                || b.key.request.window_generation != key.request.window_generation
        })
    {
        return Err(super::Rejection::Stale);
    }
    let mut geometry = SparseGeometry::default();
    for (visit, (id, g)) in layout.nodes.into_iter().enumerate() {
        if visit % 64 == 0 && cancelled() {
            return Err(super::Rejection::Stale);
        }
        if semantic.hit_order.contains_key(&id)
            && g.bounds.valid()
            && g.visible.valid()
            && g.bounds.intersection(g.visible) == Some(g.visible)
        {
            geometry.nodes.insert(id, g);
        }
    }
    geometry.hit_order.extend(geometry.nodes.keys().copied());
    geometry
        .hit_order
        .sort_unstable_by_key(|id| semantic.hit_order[id]);
    let tree = &semantic.tree;
    let mut notifications = NotificationPlan::default();
    if let Some(base) = base {
        let old = &base.semantic.tree;
        notifications.focused = (old.focused != tree.focused)
            .then_some(tree.focused)
            .flatten();
        notifications.layout_changed =
            base.key.layout_revision != key.layout_revision || old.nodes.len() != tree.nodes.len();
        notifications.structure_changed =
            old.nodes.len() != tree.nodes.len() || old.active_modal != tree.active_modal;
        let mut parents = std::collections::BTreeSet::new();
        if !Arc::ptr_eq(old, tree) {
            for (visit, (id, node)) in tree.nodes.iter().enumerate() {
                notifications.semantic_visits += 1;
                if visit % 64 == 0 && cancelled() {
                    return Err(super::Rejection::Stale);
                }
                if let Some(before) = old.nodes.get(id) {
                    if before.children.len() != node.children.len()
                        || before.children.iter().zip(&node.children).enumerate().any(
                            |(i, (before, after))| (i % 64 == 0 && cancelled()) || before != after,
                        )
                    {
                        if cancelled() {
                            return Err(super::Rejection::Stale);
                        }
                        notifications.structure_changed = true;
                    }
                    if before.selected != node.selected {
                        parents.insert(node.parent.unwrap_or(tree.root));
                        notifications
                            .selected_items
                            .insert(*id, (before.selected, node.selected));
                    }
                    if before.value != node.value {
                        notifications
                            .values
                            .insert(*id, (before.value.clone(), node.value.clone()));
                    }
                    if before.text_selection != node.text_selection {
                        notifications.text_selections.push(*id);
                    }
                } else {
                    notifications.structure_changed = true;
                }
            }
            notifications.selected_parents.extend(parents);
            for (visit, id) in old.nodes.keys().enumerate() {
                notifications.semantic_visits += 1;
                if visit % 64 == 0 && cancelled() {
                    return Err(super::Rejection::Stale);
                }
                if !tree.nodes.contains_key(id) {
                    notifications.removed.push(*id);
                }
            }
        }
    } else {
        notifications.focused = tree.focused;
        notifications.layout_changed = true;
        notifications.structure_changed = true;
    }
    Ok(PreparedFrame {
        publication_seq,
        base_publication_seq: base.map_or(0, |b| b.publication_seq),
        key,
        semantic,
        geometry,
        notifications,
        base_tree: base.map(|b| b.semantic.tree.clone()),
    })
}

impl PreparedFrame {
    /// Capture the actual installed compatibility tree on the worker before first publication.
    pub fn compatibility_baseline(
        tree: Arc<SemanticTree>,
        key: RequestKey,
    ) -> Result<Self, super::Rejection> {
        let layout = LayoutSnapshot {
            window_generation: key.window_generation,
            semantic_revision: key.semantic_revision,
            revision: tree.layout_revision,
            nodes: tree
                .nodes
                .iter()
                .filter_map(|(id, n)| n.geometry.map(|g| (*id, g)))
                .collect(),
        };
        let frame_key = FrameKey {
            request: key,
            request_seq: 0,
            layout_revision: tree.layout_revision,
        };
        let semantic = Arc::new(PreparedSemantic::from_tree(tree, key)?);
        let mut frame = prepare_frame(None, semantic, frame_key, layout, 1)?;
        frame.publication_seq = 0;
        frame.notifications = NotificationPlan::default();
        Ok(frame)
    }
}

/// Native materialization membership. Foreground inserts one ID at a time; only the
/// preparation worker copies/enumerates the set. No native handles cross this seam.
#[derive(Default)]
pub struct MaterializedNodes {
    ids: std::sync::Mutex<std::collections::BTreeSet<NodeId>>,
    revision: std::sync::atomic::AtomicU64,
}
impl MaterializedNodes {
    pub fn record(&self, id: NodeId) -> Result<(), super::Rejection> {
        let mut ids = self
            .ids
            .try_lock()
            .map_err(|_| super::Rejection::Backpressure)?;
        if ids.insert(id) {
            self.revision
                .fetch_add(1, std::sync::atomic::Ordering::Release);
        }
        Ok(())
    }
    /// Freeze membership during the coherent native/sink commit; callbacks never wait.
    pub fn with_revision<T>(
        &self,
        expected: Option<u64>,
        commit: impl FnOnce() -> Result<T, super::Rejection>,
    ) -> Result<T, super::Rejection> {
        let _ids = self
            .ids
            .try_lock()
            .map_err(|_| super::Rejection::Backpressure)?;
        if expected != Some(self.revision.load(std::sync::atomic::Ordering::Acquire)) {
            return Err(super::Rejection::Stale);
        }
        commit()
    }
    pub fn prepare_notifications(&self, frame: &mut PreparedFrame) -> Result<(), super::Rejection> {
        let ids = self
            .ids
            .lock()
            .map_err(|_| super::Rejection::Backpressure)?;
        frame.notifications.values.retain(|id, _| ids.contains(id));
        frame
            .notifications
            .selected_items
            .retain(|id, _| ids.contains(id));
        frame
            .notifications
            .text_selections
            .retain(|id| ids.contains(id));
        // Removed IDs are worker-only metadata; native cleanup uses a bounded cache cursor.
        frame.notifications.materialized_only = true;
        frame.notifications.materialization_revision =
            Some(self.revision.load(std::sync::atomic::Ordering::Acquire));
        Ok(())
    }
}
