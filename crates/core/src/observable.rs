//! Owned view projections: never expose mutable App or worker control handles.
use crate::{
    application::App,
    domain::data::Folder,
    model::{ConfirmAction, PreviewMode},
    services::{
        file_info::SizeInfo,
        list_files::FEntry,
        transfer::{JobKind, JobStatus, OverwritePolicy},
    },
    theme::{icons::IconSet, ThemePreset},
};
use std::{path::PathBuf, sync::Arc, time::Instant};
#[derive(Debug, Clone)]
pub struct Row {
    pub underlying_index: usize,
    pub entry: FEntry,
    pub selected: bool,
    pub deleting: bool,
}
#[derive(Debug, Clone)]
pub struct PaneSnapshot {
    pub folder: Option<Folder>,
    pub rows: Arc<Vec<Row>>,
    pub cursor: Option<usize>,
    pub selected_paths: Arc<Vec<PathBuf>>,
    pub listing_settled: bool,
    pub listing_generation: u64,
    pub preview_mode: PreviewMode,
    pub filter_query: Option<String>,
    pub sort_mode: usize,
    pub pending_select: Option<String>,
    pub user_navigated: bool,
}
#[derive(Debug, Clone)]
pub struct PromptSnapshot {
    pub text: String,
    pub cursor: usize,
    pub original: Option<String>,
    pub index: Option<usize>,
}
#[derive(Debug, Clone)]
pub struct ConfirmationSnapshot {
    pub action: ConfirmAction,
    pub policy: OverwritePolicy,
    pub label: String,
    pub paths: Vec<PathBuf>,
    pub dest_dir: Option<PathBuf>,
}
#[derive(Debug, Clone)]
pub struct JobSnapshot {
    pub id: u64,
    pub kind: JobKind,
    pub overwrite: OverwritePolicy,
    pub paths: Vec<PathBuf>,
    pub dest_dir: PathBuf,
    pub label: String,
    pub total_bytes: Option<u64>,
    pub copied_bytes: u64,
    pub current: PathBuf,
    pub status: JobStatus,
    pub started_at: Instant,
}
#[derive(Debug, Clone)]
pub struct InfoSnapshot {
    pub lines: Vec<String>,
    pub path: PathBuf,
    pub pending: bool,
    pub started: Instant,
}
#[derive(Debug, Clone)]
pub struct MultiInfoSnapshot {
    pub paths: Vec<PathBuf>,
    pub folders: usize,
    pub files: usize,
    pub started: Instant,
    pub aggregate: (bool, u64, u64, u64),
}
#[derive(Debug, Clone)]
pub struct DeletionSnapshot {
    pub total: usize,
    pub done: usize,
    pub current: Option<PathBuf>,
    pub started: Instant,
    pub hidden: bool,
}
#[derive(Debug, Clone)]
pub struct EditorSnapshot {
    pub document_id: u64,
    pub pane_index: usize,
    pub path: PathBuf,
    pub fs_path: PathBuf,
    pub content: String,
    pub edit_revision: u64,
    pub dirty: bool,
    pub read_only: bool,
    pub crlf: bool,
}
#[derive(Debug, Clone)]
pub struct StatusSnapshot {
    pub text: String,
    pub is_error: bool,
    pub raised: Instant,
}
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub initializing: bool,
    pub pending_editor: Option<crate::model::OpenEditorRequest>,
    pub input_context: crate::input::InputContext,
    pub transfer_dest: Option<crate::model::TransferDestSync>,
    pub revision: u64,
    pub ack_sequence: u64,
    pub window_generation: u64,
    pub document_generation: u64,
    pub focus_generation: u64,
    pub running: bool,
    pub panes: [PaneSnapshot; 2],
    pub active_pane: usize,
    pub split: bool,
    pub drives: Option<Vec<Folder>>,
    pub folders: Option<Vec<Folder>>,
    pub bookmarks: Option<Vec<Folder>>,
    pub search_query: Option<String>,
    pub show_hidden: bool,
    pub copy_board: bool,
    pub board_focused: bool,
    pub copy_board_cursor: Option<usize>,
    pub jobs: Vec<JobSnapshot>,
    pub confirming: Option<ConfirmationSnapshot>,
    pub renaming: Option<PromptSnapshot>,
    pub new_entry: Option<PromptSnapshot>,
    pub goto_prompt: Option<String>,
    pub info: Option<InfoSnapshot>,
    pub multi_info: Option<MultiInfoSnapshot>,
    pub deletion: Option<DeletionSnapshot>,
    pub edit: Option<EditorSnapshot>,
    pub edit_focus: bool,
    pub keybindings_visible: bool,
    pub status: Option<StatusSnapshot>,
    pub theme_preset: ThemePreset,
    pub icons: IconSet,
    pub sizes: Vec<(PathBuf, SizeInfo)>,
}
#[derive(Clone, PartialEq, Eq)]
struct ProjectionKey {
    generation: u64,
    listing: u64,
    deletion: u64,
    files: usize,
    sort: usize,
    filter: Option<String>,
    search: Option<String>,
}
/// Actor-only cached owned values. Snapshots clone Arcs; old publications stay immutable.
pub(crate) struct PaneProjection {
    key: ProjectionKey,
    rows: Arc<Vec<Row>>,
    selected_paths: Arc<Vec<PathBuf>>,
}
impl App {
    fn pane_projection(&self, pane_index: usize) -> std::cell::Ref<'_, PaneProjection> {
        let pane = &self.panes[pane_index];
        let key = ProjectionKey {
            generation: pane.projection_generation,
            listing: pane.listing_generation,
            deletion: self.deletion_generation,
            files: pane.files.len(),
            sort: pane.sort_mode,
            filter: pane.filter_query.clone(),
            search: if pane_index == self.active_pane {
                self.search_query.clone()
            } else {
                None
            },
        };
        let cache = &self.snapshot_cache[pane_index];
        if cache
            .borrow()
            .as_ref()
            .is_none_or(|projection| projection.key != key)
        {
            let rows = self
                .pane_visible_rows(pane_index)
                .into_iter()
                .map(|(i, e)| Row {
                    underlying_index: i,
                    entry: e.clone(),
                    selected: pane.selected.get(i).copied().unwrap_or(false),
                    deleting: self.deleting_started(&e.path).is_some(),
                })
                .collect();
            let selected_paths = pane
                .files
                .iter()
                .enumerate()
                .filter(|(i, _)| pane.selected.get(*i) == Some(&true))
                .map(|(_, e)| PathBuf::from(&e.path))
                .collect();
            *cache.borrow_mut() = Some(PaneProjection {
                key,
                rows: Arc::new(rows),
                selected_paths: Arc::new(selected_paths),
            });
        }
        std::cell::Ref::map(cache.borrow(), |entry| {
            entry.as_ref().expect("projection initialized")
        })
    }
}
impl App {
    pub fn snapshot(&self) -> Snapshot {
        let panes = std::array::from_fn(|pane_index| {
            let pane = &self.panes[pane_index];
            let projection = self.pane_projection(pane_index);
            PaneSnapshot {
                folder: pane.folder.clone(),
                rows: Arc::clone(&projection.rows),
                cursor: pane.state.selected(),
                selected_paths: Arc::clone(&projection.selected_paths),
                listing_settled: pane.listing_settled,
                listing_generation: pane.listing_generation,
                preview_mode: pane.preview_mode,
                filter_query: pane.filter_query.clone(),
                sort_mode: pane.sort_mode,
                pending_select: pane.pending_select.clone(),
                user_navigated: pane.user_navigated,
            }
        });
        Snapshot {
            initializing: self.is_initializing(),
            pending_editor: self.pending_editor_request(),
            input_context: self.input_context(),
            transfer_dest: self.transfer_dest.clone(),
            revision: self.revision,
            ack_sequence: self.ack_sequence,
            window_generation: self.window_generation,
            document_generation: self.document_generation,
            focus_generation: self.focus_generation,
            running: self.running,
            panes,
            active_pane: self.active_pane,
            split: self.split,
            drives: self.drives.clone(),
            folders: self.folders.clone(),
            bookmarks: self.bookmarks.clone(),
            search_query: self.search_query.clone(),
            show_hidden: self.show_hidden,
            copy_board: self.copy_board,
            board_focused: self.board_focused,
            copy_board_cursor: self.copy_board_state.selected(),
            jobs: self
                .jobs
                .iter()
                .map(|j| JobSnapshot {
                    id: j.id,
                    kind: j.kind,
                    overwrite: j.overwrite,
                    paths: j.paths.iter().map(PathBuf::from).collect(),
                    dest_dir: PathBuf::from(&j.dest_dir),
                    label: j.label.clone(),
                    total_bytes: j.total_bytes,
                    copied_bytes: j.copied_bytes,
                    current: PathBuf::from(&j.current),
                    status: j.status.clone(),
                    started_at: j.started_at,
                })
                .collect(),
            confirming: self.confirming.as_ref().map(|c| ConfirmationSnapshot {
                action: c.action,
                policy: c.policy,
                label: c.label.clone(),
                paths: c.paths.iter().map(PathBuf::from).collect(),
                dest_dir: c.dest_dir.as_ref().map(PathBuf::from),
            }),
            renaming: self.renaming.as_ref().map(|p| PromptSnapshot {
                text: p.text.iter().collect(),
                cursor: p.cursor,
                original: Some(p.original.clone()),
                index: Some(p.index),
            }),
            new_entry: self.new_entry.as_ref().map(|p| PromptSnapshot {
                text: p.text.iter().collect(),
                cursor: p.cursor,
                original: None,
                index: None,
            }),
            goto_prompt: self.goto_prompt.clone(),
            info: self.info.as_ref().map(|i| InfoSnapshot {
                lines: i.lines.clone(),
                path: PathBuf::from(&i.path),
                pending: i.pending,
                started: i.started,
            }),
            multi_info: self.multi_info.as_ref().map(|i| MultiInfoSnapshot {
                paths: i.paths.iter().map(PathBuf::from).collect(),
                folders: i.folders,
                files: i.files,
                started: i.started,
                aggregate: self.multi_info_aggregate(),
            }),
            deletion: self.deletion.as_ref().map(|d| DeletionSnapshot {
                total: d.total,
                done: d.done,
                current: d.current.as_ref().map(PathBuf::from),
                started: d.started,
                hidden: self.deletion_box_hidden,
            }),
            edit: self.edit.as_ref().map(|e| EditorSnapshot {
                document_id: e.document_id,
                pane_index: e.pane_index,
                path: PathBuf::from(&e.path),
                fs_path: e.fs_path.clone(),
                content: e.content.clone(),
                edit_revision: e.edit_revision,
                dirty: e.dirty,
                read_only: e.read_only,
                crlf: e.crlf,
            }),
            edit_focus: self.edit_focus,
            keybindings_visible: self.keybindings_visible,
            status: self.status.as_ref().map(|s| StatusSnapshot {
                text: s.text.clone(),
                is_error: s.is_error,
                raised: s.raised,
            }),
            theme_preset: self.theme_preset,
            icons: self.icons,
            sizes: self.size_snapshot(),
        }
    }
}
