use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{self, ErrorKind, Read},
    path::{Component, Path, PathBuf},
    process,
};

use gpui::{Bounds, Pixels};

#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};

pub(crate) const RECENT_FILE_LIMIT: usize = 10_000;
pub(crate) const RECENT_BATCH_SIZE: usize = 60;

const RECENT_FILE_HEADER: &str = "lst-recent-files-v1";
const PREVIEW_BYTES: u64 = 4096;
const PREVIEW_LINES: usize = 6;
const SEARCH_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RecentPreviewRead {
    Loaded(String),
    Missing,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RecentPreviewState {
    Loading,
    Loaded(String),
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RecentSelectionMove {
    Previous,
    Next,
    RowPrevious,
    RowNext,
}

#[must_use]
pub(crate) enum ApplyPreviewOutcome {
    Stored,
    Pruned,
}

/// Snapshot of what the recent panel should render this frame: the visible
/// slice (already filtered and paginated), totals for the count label, the
/// selected index within `visible`, and an empty-state message when nothing
/// matches. Computing it together walks `entries()` once instead of three
/// times.
pub(crate) struct RecentPage {
    pub(crate) visible: Vec<PathBuf>,
    pub(crate) total: usize,
    pub(crate) selected_index: Option<usize>,
    pub(crate) empty_message: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct RecentFiles {
    state_path: Option<PathBuf>,
    entries: Vec<PathBuf>,
}

impl RecentFiles {
    pub(crate) fn load(state_path: Option<PathBuf>) -> Self {
        let entries = state_path
            .as_deref()
            .and_then(|path| read_entries(path).ok())
            .unwrap_or_default();
        Self {
            state_path,
            entries,
        }
    }

    pub(crate) fn entries(&self) -> &[PathBuf] {
        &self.entries
    }

    #[cfg(test)]
    pub(crate) fn is_persistent(&self) -> bool {
        self.state_path.is_some()
    }

    pub(crate) fn record(&mut self, path: &Path) {
        let path = normalize_recent_path(path);
        if move_to_front(&mut self.entries, path) {
            self.entries.truncate(RECENT_FILE_LIMIT);
            let _ = self.save();
        }
    }

    pub(crate) fn prune(&mut self, path: &Path) {
        let path = normalize_recent_path(path);
        let original_len = self.entries.len();
        self.entries.retain(|entry| entry != &path);
        if self.entries.len() != original_len {
            let _ = self.save();
        }
    }

    fn save(&self) -> io::Result<()> {
        let Some(path) = &self.state_path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let temp_path = atomic_temp_path(path);
        fs::write(&temp_path, serialize_entries(&self.entries))?;
        fs::rename(temp_path, path)
    }
}

/// Owns the recent-files panel state. `panel = None` is closed; `Some` is open
/// with all per-session state (query, selection, previews, in-flight searches).
#[derive(Clone, Debug)]
pub(crate) struct RecentView {
    files: RecentFiles,
    panel: Option<RecentPanel>,
}

#[derive(Clone, Debug)]
struct RecentPanel {
    query: String,
    visible_count: usize,
    selection: Option<PathBuf>,
    card_bounds: Vec<Bounds<Pixels>>,
    previews: HashMap<PathBuf, RecentPreviewState>,
    preview_jobs: HashSet<PathBuf>,
    content_matches: HashSet<PathBuf>,
    content_search_inflight: HashSet<String>,
    content_search_generation: u64,
    content_search_pending: bool,
}

impl RecentPanel {
    fn fresh(query: String) -> Self {
        Self {
            query,
            visible_count: RECENT_BATCH_SIZE,
            selection: None,
            card_bounds: Vec::new(),
            previews: HashMap::new(),
            preview_jobs: HashSet::new(),
            content_matches: HashSet::new(),
            content_search_inflight: HashSet::new(),
            content_search_generation: 0,
            content_search_pending: false,
        }
    }
}

impl RecentView {
    pub(crate) fn load(state_path: Option<PathBuf>) -> Self {
        Self {
            files: RecentFiles::load(state_path),
            panel: None,
        }
    }

    pub(crate) fn record(&mut self, path: &Path) {
        self.files.record(path);
    }

    pub(crate) fn entries(&self) -> &[PathBuf] {
        self.files.entries()
    }

    #[cfg(test)]
    pub(crate) fn is_persistent(&self) -> bool {
        self.files.is_persistent()
    }

    pub(crate) fn is_open(&self) -> bool {
        self.panel.is_some()
    }

    /// Opens the panel, preserving any prior query so reopening keeps the
    /// filter. Returns `Some(generation)` if the preserved query is non-empty
    /// and the caller should schedule a debounced content search.
    pub(crate) fn open(&mut self) -> Option<u64> {
        let prior_query = self
            .panel
            .as_ref()
            .map(|p| p.query.clone())
            .unwrap_or_default();
        let mut panel = RecentPanel::fresh(prior_query);
        Self::reset_selection_in(&mut panel, &self.files);
        let pending_search = if panel.query.trim().is_empty() {
            None
        } else {
            panel.content_search_generation = 1;
            panel.content_search_pending = true;
            Some(panel.content_search_generation)
        };
        self.panel = Some(panel);
        pending_search
    }

    pub(crate) fn close(&mut self) {
        self.panel = None;
    }

    pub(crate) fn query(&self) -> &str {
        self.panel.as_ref().map(|p| p.query.as_str()).unwrap_or("")
    }

    /// Updates the query and resets dependent state. Returns
    /// `Some(generation)` if a debounced content search should be scheduled,
    /// `None` when the query is empty or the panel is closed.
    pub(crate) fn set_query(&mut self, text: String) -> Option<u64> {
        let panel = self.panel.as_mut()?;
        panel.query = text;
        panel.visible_count = RECENT_BATCH_SIZE;
        panel.content_matches.clear();
        Self::reset_selection_in(panel, &self.files);
        if panel.query.trim().is_empty() {
            panel.content_search_pending = false;
            None
        } else {
            panel.content_search_generation = panel.content_search_generation.saturating_add(1);
            panel.content_search_pending = true;
            Some(panel.content_search_generation)
        }
    }

    pub(crate) fn content_search_pending(&self) -> bool {
        self.panel
            .as_ref()
            .is_some_and(|panel| panel.content_search_pending)
    }

    pub(crate) fn search_still_relevant(&self, generation: u64, query: &str) -> bool {
        self.panel.as_ref().is_some_and(|panel| {
            panel.content_search_generation == generation
                && panel.query.trim().to_lowercase() == query
        })
    }

    /// Marks `query` as in-flight. Returns true if the caller should spawn
    /// the search task; false if it was already in-flight.
    pub(crate) fn start_content_search(&mut self, query: String) -> bool {
        let Some(panel) = self.panel.as_mut() else {
            return false;
        };
        if panel.content_search_inflight.contains(&query) {
            return false;
        }
        panel.content_search_inflight.insert(query);
        true
    }

    /// Applies search results if they are still relevant; returns whether
    /// they were applied.
    pub(crate) fn finish_content_search(&mut self, query: String, matches: Vec<PathBuf>) -> bool {
        let Some(panel) = self.panel.as_mut() else {
            return false;
        };
        panel.content_search_inflight.remove(&query);
        if panel.query.trim().to_lowercase() != query {
            return false;
        }
        panel.content_search_pending = false;
        panel.content_matches = matches.into_iter().collect();
        Self::ensure_selection_in(panel, &self.files);
        true
    }

    pub(crate) fn selected_path(&mut self) -> Option<PathBuf> {
        let panel = self.panel.as_mut()?;
        Self::ensure_selection_in(panel, &self.files);
        panel.selection.clone()
    }

    #[cfg(test)]
    pub(crate) fn selected_index(&self) -> Option<usize> {
        self.page().selected_index
    }

    /// Moves the keyboard selection. Returns `Some(index)` of the new selection
    /// so callers can scroll the corresponding card into view.
    pub(crate) fn move_selection(&mut self, movement: RecentSelectionMove) -> Option<usize> {
        let panel = self.panel.as_mut()?;
        let visible_paths = Self::visible_paths_for(panel, &self.files);
        if visible_paths.is_empty() {
            panel.selection = None;
            return None;
        }

        let current = panel
            .selection
            .as_ref()
            .and_then(|selected| visible_paths.iter().position(|path| path == selected))
            .unwrap_or(0);
        let last = visible_paths.len() - 1;
        let next = match movement {
            RecentSelectionMove::Previous => current.saturating_sub(1),
            RecentSelectionMove::Next => (current + 1).min(last),
            RecentSelectionMove::RowPrevious => {
                Self::row_target(&panel.card_bounds, current, visible_paths.len(), false)
                    .unwrap_or_else(|| current.saturating_sub(1))
            }
            RecentSelectionMove::RowNext => {
                Self::row_target(&panel.card_bounds, current, visible_paths.len(), true)
                    .unwrap_or_else(|| (current + 1).min(last))
            }
        };

        panel.selection = visible_paths.get(next).cloned();
        Some(next)
    }

    #[cfg(test)]
    pub(crate) fn row_selection_target(
        &self,
        current: usize,
        visible_len: usize,
        row_next: bool,
    ) -> Option<usize> {
        let panel = self.panel.as_ref()?;
        Self::row_target(&panel.card_bounds, current, visible_len, row_next)
    }

    pub(crate) fn card_bounds_for(&self, index: usize) -> Option<Bounds<Pixels>> {
        self.panel
            .as_ref()
            .and_then(|p| p.card_bounds.get(index).copied())
    }

    pub(crate) fn set_card_bounds(&mut self, bounds: Vec<Bounds<Pixels>>) {
        if let Some(panel) = self.panel.as_mut() {
            panel.card_bounds = bounds;
        }
    }

    pub(crate) fn load_more(&mut self) {
        if let Some(panel) = self.panel.as_mut() {
            panel.visible_count = panel.visible_count.saturating_add(RECENT_BATCH_SIZE);
            Self::ensure_selection_in(panel, &self.files);
        }
    }

    pub(crate) fn page(&self) -> RecentPage {
        let Some(panel) = self.panel.as_ref() else {
            return RecentPage {
                visible: Vec::new(),
                total: 0,
                selected_index: None,
                empty_message: None,
            };
        };

        let mut visible = Self::filtered_paths_for(panel, &self.files);
        let total = visible.len();
        visible.truncate(panel.visible_count);

        let selected_index = panel
            .selection
            .as_ref()
            .and_then(|selected| visible.iter().position(|path| path == selected));

        let empty_message = if total == 0 {
            let query = panel.query.trim();
            if query.is_empty() || self.files.entries().is_empty() {
                Some("No recent files".to_string())
            } else {
                Some(format!("No matches for \"{query}\""))
            }
        } else {
            None
        };

        RecentPage {
            visible,
            total,
            selected_index,
            empty_message,
        }
    }

    pub(crate) fn preview(&self, path: &Path) -> Option<&RecentPreviewState> {
        self.panel.as_ref().and_then(|p| p.previews.get(path))
    }

    /// Returns paths needing a preview read, marking each as Loading and
    /// inflight in the same step so callers cannot double-schedule.
    pub(crate) fn paths_to_load_previews(&mut self) -> Vec<PathBuf> {
        let Some(panel) = self.panel.as_mut() else {
            return Vec::new();
        };
        let visible = Self::visible_paths_for(panel, &self.files);
        let mut to_load = Vec::new();
        for path in visible {
            if panel.previews.contains_key(&path) || panel.preview_jobs.contains(&path) {
                continue;
            }
            panel
                .previews
                .insert(path.clone(), RecentPreviewState::Loading);
            panel.preview_jobs.insert(path.clone());
            to_load.push(path);
        }
        to_load
    }

    /// Records the result of a preview read. `Missing` prunes from
    /// `RecentFiles` even when the panel is closed.
    pub(crate) fn apply_preview(
        &mut self,
        path: PathBuf,
        result: RecentPreviewRead,
    ) -> ApplyPreviewOutcome {
        let state = match result {
            RecentPreviewRead::Loaded(preview) => RecentPreviewState::Loaded(preview),
            RecentPreviewRead::Failed(message) => RecentPreviewState::Failed(message),
            RecentPreviewRead::Missing => {
                self.prune_path(&path);
                return ApplyPreviewOutcome::Pruned;
            }
        };
        if let Some(panel) = self.panel.as_mut() {
            panel.preview_jobs.remove(&path);
            panel.previews.insert(path, state);
        }
        ApplyPreviewOutcome::Stored
    }

    /// Drops `path` from history and any cached preview, then re-clamps the
    /// panel selection.
    pub(crate) fn prune_path(&mut self, path: &Path) {
        if let Some(panel) = self.panel.as_mut() {
            panel.previews.remove(path);
        }
        self.files.prune(path);
        if let Some(panel) = self.panel.as_mut() {
            Self::ensure_selection_in(panel, &self.files);
        }
    }

    fn reset_selection_in(panel: &mut RecentPanel, files: &RecentFiles) {
        panel.selection = Self::visible_paths_for(panel, files).into_iter().next();
        panel.card_bounds.clear();
    }

    fn ensure_selection_in(panel: &mut RecentPanel, files: &RecentFiles) {
        let visible_paths = Self::visible_paths_for(panel, files);
        if visible_paths.is_empty() {
            panel.selection = None;
            return;
        }
        if panel
            .selection
            .as_ref()
            .is_some_and(|selected| visible_paths.iter().any(|path| path == selected))
        {
            return;
        }
        panel.selection = visible_paths.into_iter().next();
    }

    fn visible_paths_for(panel: &RecentPanel, files: &RecentFiles) -> Vec<PathBuf> {
        Self::filtered_paths_for(panel, files)
            .into_iter()
            .take(panel.visible_count)
            .collect()
    }

    fn filtered_paths_for(panel: &RecentPanel, files: &RecentFiles) -> Vec<PathBuf> {
        let query = panel.query.trim().to_lowercase();
        if query.is_empty() {
            return files.entries().to_vec();
        }
        files
            .entries()
            .iter()
            .filter(|path| Self::matches_in(panel, path, &query))
            .cloned()
            .collect()
    }

    fn matches_in(panel: &RecentPanel, path: &Path, query: &str) -> bool {
        recent_path_matches(path, query)
            || panel.content_matches.contains(path)
            || matches!(
                panel.previews.get(path),
                Some(RecentPreviewState::Loaded(preview))
                    if preview.to_lowercase().contains(query)
            )
    }

    fn row_target(
        bounds: &[Bounds<Pixels>],
        current: usize,
        visible_len: usize,
        row_next: bool,
    ) -> Option<usize> {
        let current_bounds = *bounds.get(current)?;
        let current_top = current_bounds.top();
        let current_center_x = bounds_center_x(current_bounds);
        let mut best: Option<(usize, f32, f32)> = None;

        for (ix, other) in bounds.iter().copied().take(visible_len).enumerate() {
            if ix == current {
                continue;
            }
            let row_distance = if row_next {
                other.top() - current_top
            } else {
                current_top - other.top()
            };
            if row_distance <= gpui::px(0.5) {
                continue;
            }

            let row_distance = row_distance / gpui::px(1.0);
            let x_distance = (bounds_center_x(other) - current_center_x).abs();
            if best.is_none_or(|(_, best_row, best_x)| {
                row_distance < best_row || (row_distance == best_row && x_distance < best_x)
            }) {
                best = Some((ix, row_distance, x_distance));
            }
        }

        best.map(|(ix, _, _)| ix)
    }
}

pub(crate) fn recent_path_matches(path: &Path, query: &str) -> bool {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_lowercase();
    file_name.contains(query) || path.to_string_lossy().to_lowercase().contains(query)
}

fn bounds_center_x(bounds: Bounds<Pixels>) -> f32 {
    bounds.left() / gpui::px(1.0) + (bounds.size.width / gpui::px(1.0)) / 2.0
}

#[cfg(not(test))]
pub(crate) fn default_recent_files_path() -> Option<PathBuf> {
    if let Some(state_home) = std::env::var_os("XDG_STATE_HOME") {
        return Some(PathBuf::from(state_home).join("lst").join("recent-files"));
    }
    std::env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("lst")
            .join("recent-files")
    })
}

pub(crate) fn normalize_recent_path(path: &Path) -> PathBuf {
    let absolute = absolute_recent_path(path);
    fs::canonicalize(&absolute).unwrap_or_else(|_| clean_path(&absolute))
}

fn normalize_recent_path_without_io(path: &Path) -> PathBuf {
    clean_path(&absolute_recent_path(path))
}

fn absolute_recent_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(path)
    }
}

pub(crate) fn read_recent_preview(path: &Path) -> RecentPreviewRead {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == ErrorKind::NotFound => return RecentPreviewRead::Missing,
        Err(err) => return RecentPreviewRead::Failed(err.to_string()),
    };
    let mut bytes = Vec::new();
    match file.take(PREVIEW_BYTES).read_to_end(&mut bytes) {
        Ok(_) => RecentPreviewRead::Loaded(preview_from_bytes(&bytes)),
        Err(err) => RecentPreviewRead::Failed(err.to_string()),
    }
}

pub(crate) fn search_recent_content(paths: Vec<PathBuf>, query: &str) -> Vec<PathBuf> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }

    paths
        .into_iter()
        .filter(|path| recent_file_content_matches(path, &query))
        .collect()
}

fn recent_file_content_matches(path: &Path, query: &str) -> bool {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return false,
    };
    let mut bytes = Vec::new();
    if file.take(SEARCH_BYTES).read_to_end(&mut bytes).is_err() {
        return false;
    }
    String::from_utf8_lossy(&bytes)
        .to_lowercase()
        .contains(query)
}

fn preview_from_bytes(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = text.lines().take(PREVIEW_LINES).collect::<Vec<_>>();
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    if lines.is_empty() {
        "Blank file".to_string()
    } else {
        lines.join("\n")
    }
}

fn clean_path(path: &Path) -> PathBuf {
    let mut cleaned = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => cleaned.push(prefix.as_os_str()),
            Component::RootDir => cleaned.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                cleaned.pop();
            }
            Component::Normal(part) => cleaned.push(part),
        }
    }
    cleaned
}

fn read_entries(path: &Path) -> io::Result<Vec<PathBuf>> {
    let body = fs::read_to_string(path)?;
    let mut lines = body.lines();
    if lines.next() != Some(RECENT_FILE_HEADER) {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();
    for line in lines {
        let Some(path) = decode_path(line) else {
            return Ok(Vec::new());
        };
        let path = normalize_recent_path_without_io(&path);
        if !entries.iter().any(|entry| entry == &path) {
            entries.push(path);
            entries.truncate(RECENT_FILE_LIMIT);
        }
    }
    Ok(entries)
}

fn serialize_entries(entries: &[PathBuf]) -> String {
    let mut body = String::from(RECENT_FILE_HEADER);
    body.push('\n');
    for path in entries {
        body.push_str(&encode_path(path));
        body.push('\n');
    }
    body
}

fn move_to_front(entries: &mut Vec<PathBuf>, path: PathBuf) -> bool {
    if entries.first() == Some(&path) {
        return false;
    }
    if let Some(index) = entries.iter().position(|entry| entry == &path) {
        entries.remove(index);
    }
    entries.insert(0, path);
    true
}

fn atomic_temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("recent-files");
    path.with_file_name(format!("{name}.tmp-{}", process::id()))
}

fn encode_path(path: &Path) -> String {
    let bytes = path_bytes(path);
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(hex_digit(byte >> 4));
        encoded.push(hex_digit(byte & 0x0f));
    }
    encoded
}

fn decode_path(encoded: &str) -> Option<PathBuf> {
    if !encoded.len().is_multiple_of(2) {
        return None;
    }

    let mut bytes = Vec::with_capacity(encoded.len() / 2);
    for pair in encoded.as_bytes().chunks_exact(2) {
        let high = from_hex(pair[0])?;
        let low = from_hex(pair[1])?;
        bytes.push((high << 4) | low);
    }
    Some(path_from_bytes(bytes))
}

fn hex_digit(value: u8) -> char {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    HEX[value as usize] as char
}

fn from_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> Vec<u8> {
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
fn path_bytes(path: &Path) -> Vec<u8> {
    path.to_string_lossy().as_bytes().to_vec()
}

#[cfg(unix)]
fn path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    std::ffi::OsString::from_vec(bytes).into()
}

#[cfg(not(unix))]
fn path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    String::from_utf8_lossy(&bytes).into_owned().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEST_DIR: AtomicUsize = AtomicUsize::new(0);

    fn temp_dir(label: &str) -> PathBuf {
        let id = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "lst-gpui-recent-tests-{label}-{}-{id}",
            process::id()
        ));
        fs::create_dir(&dir).expect("create recent test temp dir");
        dir
    }

    #[test]
    fn record_dedupes_and_moves_existing_paths_to_front() {
        let dir = temp_dir("dedupe");
        let state_path = dir.join("recent");
        let one = dir.join("one.txt");
        let two = dir.join("two.txt");

        let mut recent = RecentFiles::load(Some(state_path.clone()));
        recent.record(&one);
        recent.record(&two);
        recent.record(&one);

        assert_eq!(
            recent.entries(),
            [normalize_recent_path(&one), normalize_recent_path(&two)]
        );
        assert!(state_path.exists());

        fs::remove_dir_all(dir).expect("remove recent test temp dir");
    }

    #[test]
    fn load_round_trips_paths_without_touching_targets() {
        let dir = temp_dir("roundtrip");
        let state_path = dir.join("recent");
        let path = dir.join("missing\nname.txt");

        let mut recent = RecentFiles::load(Some(state_path.clone()));
        recent.record(&path);
        let loaded = RecentFiles::load(Some(state_path));

        assert_eq!(loaded.entries(), [normalize_recent_path(&path)]);

        fs::remove_dir_all(dir).expect("remove recent test temp dir");
    }

    #[test]
    fn corrupt_state_loads_as_empty() {
        let dir = temp_dir("corrupt");
        let state_path = dir.join("recent");
        fs::write(&state_path, "not the header\n").expect("write corrupt recent state");

        let loaded = RecentFiles::load(Some(state_path));

        assert!(loaded.entries().is_empty());

        fs::remove_dir_all(dir).expect("remove recent test temp dir");
    }

    #[test]
    fn prune_removes_matching_normalized_path() {
        let dir = temp_dir("prune");
        let state_path = dir.join("recent");
        let path = dir.join("gone.txt");

        let mut recent = RecentFiles::load(Some(state_path.clone()));
        recent.record(&path);
        recent.prune(&path);
        let loaded = RecentFiles::load(Some(state_path));

        assert!(recent.entries().is_empty());
        assert!(loaded.entries().is_empty());

        fs::remove_dir_all(dir).expect("remove recent test temp dir");
    }

    #[test]
    fn cap_keeps_the_most_recent_entries() {
        let dir = temp_dir("cap");
        let mut recent = RecentFiles::load(None);

        for index in 0..(RECENT_FILE_LIMIT + 3) {
            recent.record(&dir.join(format!("{index}.txt")));
        }

        assert_eq!(recent.entries().len(), RECENT_FILE_LIMIT);
        assert_eq!(
            recent.entries().first(),
            Some(&normalize_recent_path(
                &dir.join(format!("{}.txt", RECENT_FILE_LIMIT + 2))
            ))
        );
        assert_eq!(
            recent.entries().last(),
            Some(&normalize_recent_path(&dir.join("3.txt")))
        );

        fs::remove_dir_all(dir).expect("remove recent test temp dir");
    }
}
