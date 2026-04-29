use std::{
    fs,
    io::{self, ErrorKind, Read},
    path::{Component, Path, PathBuf},
    process,
};

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
